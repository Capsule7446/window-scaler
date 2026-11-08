use crate::{
    backend::{Action, AppliedChange, Backend, Event},
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

impl Scaler {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let subscriptions = vec![
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
                    this.status = "托盘不可用；请点击“显示并放大后退出”正常退出".into();
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

impl Render for Scaler {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value();
        let width = valid_width(&self.width.read(cx).value());
        let hidden = hidden_selected(&self.windows, &self.selected, &query);
        let blocked = self.busy || self.refreshing || self.exit_requested;
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let rows = self
            .windows
            .iter()
            .filter(|w| w.matches(&query))
            .map(|w| {
                let id = w.id;
                let checked = self.selected.contains(&id);
                let size = w
                    .size
                    .map(|s| format!("{} × {}", s.width, s.height))
                    .unwrap_or_else(|| "—".into());
                let (result, failed) = self.results.get(&id).cloned().unwrap_or_else(|| {
                    (
                        w.unavailable.clone().unwrap_or_default(),
                        w.unavailable.is_some(),
                    )
                });
                div()
                    .id(("window", id.hwnd))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_3()
                    .border_b_1()
                    .border_color(border)
                    .when(checked, |row| row.bg(cx.theme().accent))
                    .child(
                        Checkbox::new(("check", id.hwnd))
                            .accessibility_label(format!("选择 {} · PID {}", w.title, id.pid))
                            .checked(checked)
                            .disabled(blocked || w.unavailable.is_some())
                            .on_change(cx.listener(move |this, selected: &bool, _, cx| {
                                if *selected {
                                    this.selected.insert(id);
                                } else {
                                    this.selected.remove(&id);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().text_ellipsis().child(if w.hidden {
                                format!("{} · 已隐藏", w.title)
                            } else {
                                w.title.clone()
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{}  ·  PID {}", w.program, id.pid)),
                            ),
                    )
                    .child(div().w(px(104.)).text_sm().text_color(muted).child(size))
                    .child(
                        div()
                            .w(px(192.))
                            .text_xs()
                            .text_color(if failed { rgb(0xb42318).into() } else { muted })
                            .child(result),
                    )
            })
            .collect::<Vec<_>>();
        let visible = rows.len();
        div()
            .id("window-scaler")
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .font_family("Microsoft YaHei UI")
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("窗口缩放器"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child("选择窗口，调整大小或隐藏、显示。"),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child("Rust + GPUI Kit")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.search)))
                    .child(
                        Button::new("refresh")
                            .label("刷新列表")
                            .disabled(blocked)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .child(
                        Button::new("clear")
                            .label("清空勾选")
                            .disabled(blocked || self.selected.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected.clear();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .child(format!(
                        "已选 {} 个，其中 {} 个被过滤隐藏",
                        self.selected.len(),
                        hidden
                    ))
                    .child(
                        div()
                            .text_color(muted)
                            .child(format!("显示 {visible} / {}", self.windows.len())),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .text_color(muted)
                            .child(div().flex_1().child("窗口 / 程序"))
                            .child(div().w(px(104.)).child("内容区尺寸"))
                            .child(div().w(px(192.)).child("操作结果")),
                    )
                    .child(
                        div()
                            .id("windows-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(rows.is_empty(), |list| {
                                list.child(div().p_6().text_color(muted).child(
                                    if self.refreshing {
                                        "正在读取窗口…"
                                    } else {
                                        "没有匹配的窗口"
                                    },
                                ))
                            })
                            .children(rows),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_sm().child("目标宽度"))
                    .child(
                        div()
                            .w(px(112.))
                            .child(Input::new(&self.width).disabled(blocked)),
                    )
                    .child(div().text_sm().text_color(muted).child("像素 · 保持原比例"))
                    .child(div().flex_1())
                    .child(
                        Button::new("shrink")
                            .primary()
                            .label(format!("缩小（{}）", self.selected.len()))
                            .disabled(blocked || self.selected.is_empty() || width.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(width) = valid_width(&this.width.read(cx).value()) {
                                    this.begin(Action::Shrink(width), false, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("maximize")
                            .label(format!("放大（{}）", self.selected.len()))
                            .disabled(blocked || self.selected.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.begin(Action::Maximize, false, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_sm().child("窗口可见性"))
                    .child(
                        Button::new("hide")
                            .label(format!("隐藏（{}）", self.selected.len()))
                            .disabled(blocked || self.selected.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.begin(Action::Hide, false, cx)
                            })),
                    )
                    .child(
                        Button::new("show")
                            .label(format!("显示（{}）", self.selected.len()))
                            .disabled(blocked || self.selected.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.begin(Action::Show, false, cx)
                            })),
                    ),
            )
            .when(width.is_none(), |root| {
                root.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xb42318))
                        .child("请输入有效的正整数宽度。"),
                )
            })
            .when_some(self.tray_error.clone(), |root, error| {
                root.child(div().text_sm().text_color(rgb(0xb42318)).child(error))
                    .child(
                        Button::new("fallback-exit")
                            .label("显示并放大后退出")
                            .disabled(self.busy)
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.begin(Action::Maximize, true, cx)
                                }),
                            ),
                    )
            })
            .when(self.exit_failed, |root| {
                root.child(
                    div()
                        .p_3()
                        .border_1()
                        .border_color(rgb(0xd92d20))
                        .rounded_md()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .child("部分窗口尚未完成显示或放大；未完成的请求仍可能继续生效。"),
                        )
                        .child(
                            div()
                                .id("exit-errors")
                                .max_h(px(96.))
                                .overflow_y_scroll()
                                .text_xs()
                                .children(
                                    self.exit_errors
                                        .iter()
                                        .cloned()
                                        .map(|error| div().child(error)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("retry-exit")
                                        .primary()
                                        .label("重试并退出")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.begin(Action::Maximize, true, cx)
                                        })),
                                )
                                .child(
                                    Button::new("force-exit")
                                        .label("仍然退出")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.quit(cx))),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(self.status.clone())
                    .child("关闭主界面会收起到托盘；正常退出会显示本次隐藏的窗口，并放大本次缩过的窗口。"),
            )
    }
}
