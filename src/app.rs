use crate::{
    backend::{Action, AppliedChange, Backend, Event},
    layout::{Corner, Direction, TilingOptions, valid_gap},
    model::{WindowId, WindowInfo, hidden_selected, valid_width},
    native, settings, tray,
};
use gpui_kit::component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

pub struct Scaler {
    backend: Arc<Backend>,
    windows: Vec<WindowInfo>,
    selected: HashSet<WindowId>,
    results: HashMap<WindowId, (String, bool)>,
    search: Entity<InputState>,
    width: Entity<InputState>,
    gap: Entity<InputState>,
    panel: OperationPanel,
    corner: Corner,
    direction: Direction,
    _subscriptions: Vec<Subscription>,
    _event_task: Task<()>,
    tray: Option<tray_icon::TrayIcon>,
    tray_error: Option<String>,
    hwnd: usize,
    busy: bool,
    refreshing: bool,
    exit_failed: bool,
    exit_requested: bool,
    exit_errors: Vec<String>,
    status: String,
    settings_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationPanel {
    Scaling,
    Tiling,
}

impl Scaler {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        view::configure_theme(cx);
        let (tx, rx) = async_channel::unbounded::<Event>();
        let backend = Backend::new(tx.clone());
        let hwnd = match HasWindowHandle::window_handle(window)
            .expect("native window handle")
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as usize,
            _ => unreachable!("Windows application"),
        };
        let settings_path = settings::path();
        let initial_width = settings::load(&settings_path);
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索窗口标题、程序名或 PID"));
        let width =
            cx.new(|cx| InputState::new(window, cx).default_value(initial_width.to_string()));
        let gap = cx.new(|cx| InputState::new(window, cx).default_value("8"));
        let subscriptions = vec![
            cx.subscribe_in(&gap, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &width,
                window,
                |this: &mut Self, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        if let Some(width) = valid_width(&state.read(cx).value())
                            && let Err(error) = settings::save(&this.settings_path, width)
                        {
                            this.status = format!("宽度可用，但保存设置失败：{error}");
                        }
                        cx.notify();
                    }
                },
            ),
        ];
        let (tray, tray_error) = match tray::create(tx) {
            Ok(tray) => (Some(tray), None),
            Err(error) => (
                None,
                Some(format!("托盘不可用：{error}。请使用界面上的退出按钮。")),
            ),
        };
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                if this.tray.is_some() {
                    native::hide_main(this.hwnd);
                } else {
                    this.status = "托盘不可用；请点击底部“正常退出”".into();
                    cx.notify();
                }
            });
            false
        });
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                if this
                    .update(cx, |view, cx| view.handle_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        backend.refresh();
        Self {
            backend,
            windows: Vec::new(),
            selected: HashSet::new(),
            results: HashMap::new(),
            search,
            width,
            gap,
            panel: OperationPanel::Scaling,
            corner: Corner::TopLeft,
            direction: Direction::Horizontal,
            _subscriptions: subscriptions,
            _event_task: task,
            tray,
            tray_error,
            hwnd,
            busy: false,
            refreshing: true,
            exit_failed: false,
            exit_requested: false,
            exit_errors: Vec::new(),
            status: "正在读取桌面窗口…".into(),
            settings_path,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing || self.busy || self.exit_requested {
            return;
        }
        self.refreshing = true;
        self.status = "正在刷新窗口列表…".into();
        self.backend.refresh();
        cx.notify();
    }

    fn begin(&mut self, action: Action, exiting: bool, cx: &mut Context<Self>) {
        if exiting {
            self.exit_requested = true;
            if self.busy || self.refreshing {
                self.status = "已收到退出请求，等待当前操作结束后显示并放大窗口…".into();
                cx.notify();
                return;
            }
        }
        if self.busy
            || (!exiting && (self.refreshing || self.exit_requested || self.selected.is_empty()))
        {
            return;
        }
        self.busy = true;
        self.exit_errors.clear();
        self.status = if exiting {
            "退出前正在显示隐藏窗口，并放大本次缩过的窗口…".into()
        } else {
            format!("正在处理 {} 个已选窗口…", self.selected.len())
        };
        self.backend
            .run(self.selected.iter().copied().collect(), action, exiting);
        cx.notify();
    }

    fn begin_tiling(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.refreshing || self.exit_requested || self.selected.is_empty() {
            return;
        }
        let Some(gap) = valid_gap(&self.gap.read(cx).value()) else {
            self.status = "排列间隔请输入 0 至 2147483647 的整数".into();
            cx.notify();
            return;
        };
        let area = match native::work_area(self.hwnd) {
            Ok(area) => area,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        };
        let selected: Vec<_> = self
            .windows
            .iter()
            .filter(|entry| self.selected.contains(&entry.id))
            .map(|entry| entry.id)
            .collect();
        self.busy = true;
        self.exit_errors.clear();
        self.status = format!("正在排列 {} 个已选窗口…", selected.len());
        self.backend.tile(
            selected,
            area,
            TilingOptions {
                corner: self.corner,
                direction: self.direction,
                gap,
            },
        );
        cx.notify();
    }

    fn handle_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Windows(result) => {
                self.refreshing = false;
                match result {
                    Ok(windows) => {
                        let valid: HashSet<_> = windows.iter().map(|w| w.id).collect();
                        self.selected.retain(|id| valid.contains(id));
                        self.results.retain(|id, _| valid.contains(id));
                        self.status = format!("已找到 {} 个桌面窗口", windows.len());
                        self.windows = windows;
                    }
                    Err(error) => self.status = error,
                }
                if self.exit_requested && !self.busy && !self.exit_failed {
                    self.begin(Action::Maximize, true, cx);
                }
            }
            Event::Batch { outcomes, exiting } => {
                self.busy = false;
                let total = outcomes.len();
                let mut failed = 0;
                for outcome in outcomes {
                    if let Some(window) = self.windows.iter_mut().find(|w| w.id == outcome.id) {
                        window.hidden = self.backend.registry.is_hidden(outcome.id);
                    }
                    match outcome.result {
                        Ok(change) => {
                            if let Some(size) = change.size()
                                && let Some(window) =
                                    self.windows.iter_mut().find(|w| w.id == outcome.id)
                            {
                                window.size = Some(size);
                            }
                            let message = match change {
                                AppliedChange::Shrunk(size) => {
                                    format!("已缩小至 {}×{}", size.width, size.height)
                                }
                                AppliedChange::Maximized(_) => "已最大化".into(),
                                AppliedChange::RestoredOriginal(size) => {
                                    format!("已恢复原尺寸 {}×{}", size.width, size.height)
                                }
                                AppliedChange::Hidden => "已隐藏".into(),
                                AppliedChange::Shown => "已显示".into(),
                                AppliedChange::Tiled(_) => "已排列，大小不变".into(),
                            };
                            self.results.insert(outcome.id, (message, false));
                        }
                        Err(error) => {
                            failed += 1;
                            if exiting {
                                let name = self
                                    .windows
                                    .iter()
                                    .find(|w| w.id == outcome.id)
                                    .map(|w| w.title.clone())
                                    .unwrap_or_else(|| format!("PID {}", outcome.id.pid));
                                self.exit_errors.push(format!("{name}：{error}"));
                            }
                            self.results.insert(outcome.id, (error, true));
                        }
                    }
                }
                self.status = format!("操作完成：{} 个成功，{} 个未完成", total - failed, failed);
                if exiting {
                    if failed == 0 {
                        self.quit(cx);
                        return;
                    }
                    self.exit_failed = true;
                    native::show_main(self.hwnd);
                } else if self.exit_requested {
                    self.begin(Action::Maximize, true, cx);
                }
            }
            Event::Open => {
                native::show_main(self.hwnd);
                self.refresh(cx);
            }
            Event::Exit => self.begin(Action::Maximize, true, cx),
        }
        cx.notify();
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.backend.registry.cleanup();
        self.tray.take();
        cx.quit();
    }
}

#[path = "app_view.rs"]
mod view;
