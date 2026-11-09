use super::*;
use gpui_kit::base::{Checkbox as SelectionCheckbox, CheckboxState};
use gpui_kit::component::{
    Icon, IconName, Selectable, Sizable, Theme, ThemeMode, tooltip::Tooltip,
};

pub(super) fn configure_theme(cx: &mut App) {
    Theme::change(ThemeMode::Light, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = "Microsoft YaHei UI".into();
        theme.foreground = rgb(0x263954).into();
        theme.muted_foreground = rgb(0x64748b).into();
        theme.background = rgb(0xffffff).into();
        theme.muted = rgb(0xf8fafc).into();
        theme.border = rgb(0xe2e8f0).into();
        theme.input = rgb(0xcbd5e1).into();
        theme.primary = rgb(0x3563e9).into();
        theme.primary_foreground = rgb(0xffffff).into();
        theme.button_primary = rgb(0x3563e9).into();
        theme.button_primary_hover = rgb(0x2852cc).into();
        theme.button_primary_active = rgb(0x2448b5).into();
        theme.button_primary_foreground = rgb(0xffffff).into();
        theme.accent = rgb(0xeef3ff).into();
        theme.accent_foreground = rgb(0x3563e9).into();
        theme.ring = rgb(0x3563e9).into();
    });
}

impl Scaler {
    fn controls_blocked(&self) -> bool {
        self.busy || self.refreshing || self.exit_requested
    }

    fn render_window_row(
        &self,
        entry: &WindowInfo,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = entry.id;
        let selected = self.selected.contains(&id);
        let muted = cx.theme().muted_foreground;
        let size = entry
            .size
            .map(|size| format!("{} × {}", size.width, size.height))
            .unwrap_or_else(|| "尺寸未知".into());
        let (result, failed) = self.results.get(&id).cloned().unwrap_or_else(|| {
            (
                entry.unavailable.clone().unwrap_or_default(),
                entry.unavailable.is_some(),
            )
        });
        let (state, state_color, state_background) = if entry.unavailable.is_some() {
            ("不可操作", rgb(0x9a5c18), rgb(0xfff5e8))
        } else if entry.hidden {
            ("已隐藏", rgb(0x6556a4), rgb(0xf0edfa))
        } else {
            ("未隐藏", rgb(0x357566), rgb(0xeef7f3))
        };
        let detail = format!(
            "{}\n{} · PID {} · {}\n{}",
            entry.title, entry.program, id.pid, size, result
        );
        div()
            .id(("window", id.hwnd))
            .flex()
            .items_start()
            .gap_3()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(if selected {
                cx.theme().accent
            } else {
                cx.theme().background
            })
            .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
            .child(
                Checkbox::new(("check", id.hwnd))
                    .accessibility_label(format!("选择 {} · PID {}", entry.title, id.pid))
                    .checked(selected)
                    .disabled(self.controls_blocked() || entry.unavailable.is_some())
                    .mt(px(3.))
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_ellipsis()
                                    .child(entry.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_sm()
                                    .text_size(px(10.))
                                    .text_color(state_color)
                                    .bg(state_background)
                                    .child(state),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(format!("PID {}  ·  {}  ·  {}", id.pid, size, entry.program)),
                    )
                    .when(!result.is_empty(), |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(if failed { rgb(0xb42318).into() } else { muted })
                                .child(result),
                        )
                    }),
            )
    }

    fn render_window_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let query = self.search.read(cx).value();
        let visible: Vec<_> = self
            .windows
            .iter()
            .filter(|entry| entry.matches(&query))
            .collect();
        let available: Vec<_> = visible
            .iter()
            .filter(|entry| entry.unavailable.is_none())
            .collect();
        let selected_count = available
            .iter()
            .filter(|entry| self.selected.contains(&entry.id))
            .count();
        let selection_state = if selected_count == 0 {
            CheckboxState::Unchecked
        } else if selected_count == available.len() {
            CheckboxState::Checked
        } else {
            CheckboxState::Indeterminate
        };
        let blocked = self.controls_blocked();
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let selected_color = cx.theme().primary;
        let checked = selection_state != CheckboxState::Unchecked;
        let selection_disabled = blocked || available.is_empty();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p_5()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.search).disabled(self.exit_requested)),
                    )
                    .child(
                        Button::new("refresh")
                            .label("刷新")
                            .icon(IconName::RefreshCw)
                            .disabled(blocked)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(muted)
                    .child(format!(
                        "当前列表 {} / {}",
                        visible.len(),
                        self.windows.len()
                    ))
                    .child(
                        Button::new("clear")
                            .ghost()
                            .small()
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
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .overflow_hidden()
                    .bg(cx.theme().background)
                    .child(
                        div()
                            .h(px(36.))
                            .flex_shrink_0()
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(cx.theme().muted)
                            .border_b_1()
                            .border_color(border)
                            .child(
                                SelectionCheckbox::new("select-visible")
                                    .state(selection_state)
                                    .accessibility_label("全选当前列表")
                                    .disabled(selection_disabled)
                                    .rounded_sm()
                                    .focus(|style| style.border_color(selected_color).border_1())
                                    .when(selection_disabled, |control| control.opacity(0.5))
                                    .child(
                                        div()
                                            .size(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .border_1()
                                            .rounded_sm()
                                            .border_color(if checked {
                                                selected_color
                                            } else {
                                                cx.theme().input
                                            })
                                            .bg(if checked {
                                                selected_color
                                            } else {
                                                cx.theme().background
                                            })
                                            .when(checked, |indicator| {
                                                indicator.child(
                                                    Icon::new(
                                                        if selection_state
                                                            == CheckboxState::Indeterminate
                                                        {
                                                            IconName::Minus
                                                        } else {
                                                            IconName::Check
                                                        },
                                                    )
                                                    .size(px(12.))
                                                    .text_color(rgb(0xffffff)),
                                                )
                                            }),
                                    )
                                    .on_change({
                                        let listener =
                                            cx.listener(|this, selected: &bool, _, cx| {
                                                let query = this.search.read(cx).value();
                                                for entry in this.windows.iter().filter(|entry| {
                                                    entry.matches(&query)
                                                        && entry.unavailable.is_none()
                                                }) {
                                                    if *selected {
                                                        this.selected.insert(entry.id);
                                                    } else {
                                                        this.selected.remove(&entry.id);
                                                    }
                                                }
                                                cx.notify();
                                            });
                                        move |state, _, window, cx| {
                                            listener(&(state == CheckboxState::Checked), window, cx)
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("窗口 / 进程 / 内容区尺寸"),
                            ),
                    )
                    .child(
                        div()
                            .id("windows-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(visible.is_empty(), |list| {
                                list.child(
                                    div()
                                        .p_6()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap_2()
                                        .text_sm()
                                        .text_color(muted)
                                        .child(Icon::new(IconName::Search).size(px(24.)))
                                        .child(if self.refreshing {
                                            "正在读取窗口…"
                                        } else {
                                            "没有匹配的窗口"
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .child("搜索标题、程序名或 PID，或刷新列表。"),
                                        ),
                                )
                            })
                            .children(
                                visible
                                    .into_iter()
                                    .map(|entry| self.render_window_row(entry, cx)),
                            ),
                    ),
            )
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let query = self.search.read(cx).value();
        let outside = hidden_selected(&self.windows, &self.selected, &query);
        let blocked = self.controls_blocked();
        let disabled = blocked || self.selected.is_empty();
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let selected: Vec<_> = self
            .windows
            .iter()
            .filter(|entry| self.selected.contains(&entry.id))
            .take(2)
            .collect();
        div().id("operations-scroll").w(px(260.)).flex_shrink_0().min_h_0()
            .border_l_1().border_color(border).bg(cx.theme().background)
            .overflow_y_scroll()
            .child(div().p_5().flex().flex_col().gap_4()
                .child(div().flex().flex_col().gap_2()
                    .child(div().text_xs().text_color(muted).child("当前选择"))
                    .child(div().flex().items_baseline().gap_2()
                        .child(div().text_size(px(30.)).font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x405b88)).child(self.selected.len().to_string()))
                        .child(div().text_sm().text_color(muted).child("个窗口")))
                    .child(div().flex().flex_col().gap_1()
                        .when(selected.is_empty(), |chips| chips.child(div().text_xs().text_color(muted).child("从左侧列表勾选窗口")))
                        .children(selected.iter().map(|entry| div().px_2().py_1().rounded_sm()
                            .bg(cx.theme().accent).text_color(rgb(0x3f5e9e)).text_xs().text_ellipsis().child(entry.title.clone())))
                        .when(self.selected.len() > 2, |chips| chips.child(div().text_xs().text_color(muted)
                            .child(format!("另有 {} 个窗口", self.selected.len() - 2))))))
                .when(outside > 0, |panel| panel.child(div().p_2().rounded_md()
                    .bg(rgb(0xfff7e8)).text_color(rgb(0x8a5b1d)).text_xs()
                    .child(format!("{outside} 个已选窗口不在当前列表，仍参与本次操作。"))))
                .child(div().border_t_1().border_color(border).pt_3().flex().gap_1()
                    .child(Button::new("scaling-panel").small().flex_1().label("缩放/恢复")
                        .selected(self.panel == OperationPanel::Scaling).disabled(blocked)
                        .on_click(cx.listener(|this, _, _, cx| { this.panel = OperationPanel::Scaling; cx.notify(); })))
                    .child(Button::new("tiling-panel").small().flex_1().label("窗口排列")
                        .selected(self.panel == OperationPanel::Tiling).disabled(blocked)
                        .on_click(cx.listener(|this, _, _, cx| { this.panel = OperationPanel::Tiling; cx.notify(); }))))
                .child(match self.panel {
                    OperationPanel::Scaling => self.render_scaling(cx).into_any_element(),
                    OperationPanel::Tiling => self.render_tiling(cx).into_any_element(),
                })
                .child(div().border_t_1().border_color(border).pt_3().flex().flex_col().gap_3()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("窗口可见性"))
                    .child(div().flex().gap_2()
                        .child(Button::new("hide").flex_1().label("隐藏").icon(IconName::EyeOff)
                            .disabled(disabled).on_click(cx.listener(|this, _, _, cx| this.begin(Action::Hide, false, cx))))
                        .child(Button::new("show").flex_1().label("显示").icon(IconName::Eye)
                            .disabled(disabled).on_click(cx.listener(|this, _, _, cx| this.begin(Action::Show, false, cx))))))
                .child(div().text_xs().text_color(muted).child("显示保留隐藏前的位置、大小和状态。正常退出会显示本次隐藏的窗口，并放大本次缩过的窗口。"))
                .child(div().text_xs().text_color(rgb(0x405b88)).child(
                    if self.busy { "正在处理，请稍候…" } else { "操作对全部已选窗口生效" }
                )))
    }

    fn render_scaling(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let blocked = self.controls_blocked();
        let disabled = blocked || self.selected.is_empty();
        let width_valid = valid_width(&self.width.read(cx).value()).is_some();
        let muted = cx.theme().muted_foreground;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("窗口大小"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_xs().text_color(muted).child("目标宽度"))
                    .child(
                        div()
                            .w(px(116.))
                            .child(Input::new(&self.width).suffix("px").disabled(blocked)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("内容区像素 · 保持原比例"),
            )
            .when(!width_valid, |panel| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xb42318))
                        .child("请输入有效的正整数宽度。"),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("shrink")
                            .primary()
                            .flex_1()
                            .label("缩小")
                            .icon(IconName::Minimize)
                            .disabled(disabled || !width_valid)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(width) = valid_width(&this.width.read(cx).value()) {
                                    this.begin(Action::Shrink(width), false, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("maximize")
                            .flex_1()
                            .label("放大")
                            .icon(IconName::Maximize)
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.begin(Action::Maximize, false, cx)
                            })),
                    ),
            )
    }

    fn render_tiling(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let blocked = self.controls_blocked();
        let disabled = blocked || self.selected.is_empty();
        let gap_valid = valid_gap(&self.gap.read(cx).value()).is_some();
        let muted = cx.theme().muted_foreground;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("排列起点"),
            )
            .children(
                [
                    [
                        ("top-left", "↖ 左上", Corner::TopLeft),
                        ("top-right", "右上 ↗", Corner::TopRight),
                    ],
                    [
                        ("bottom-left", "↙ 左下", Corner::BottomLeft),
                        ("bottom-right", "右下 ↘", Corner::BottomRight),
                    ],
                ]
                .into_iter()
                .map(|row| {
                    div()
                        .flex()
                        .gap_2()
                        .children(row.into_iter().map(|(id, label, corner)| {
                            Button::new(id)
                                .small()
                                .flex_1()
                                .label(label)
                                .selected(self.corner == corner)
                                .disabled(blocked)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.corner = corner;
                                    cx.notify();
                                }))
                        }))
                }),
            )
            .child(
                div().flex().gap_2().children(
                    [
                        ("horizontal", "水平平铺", Direction::Horizontal),
                        ("vertical", "垂直平铺", Direction::Vertical),
                    ]
                    .into_iter()
                    .map(|(id, label, direction)| {
                        Button::new(id)
                            .small()
                            .flex_1()
                            .label(label)
                            .selected(self.direction == direction)
                            .disabled(blocked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.direction = direction;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_xs().text_color(muted).child("窗口间隔"))
                    .child(
                        div()
                            .w(px(116.))
                            .child(Input::new(&self.gap).suffix("px").disabled(blocked)),
                    ),
            )
            .when(!gap_valid, |panel| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xb42318))
                        .child("请输入 0 至 2147483647 的整数。"),
                )
            })
            .child(
                Button::new("tile-windows")
                    .primary()
                    .w_full()
                    .label("一键平铺")
                    .disabled(disabled || !gap_valid)
                    .on_click(cx.listener(|this, _, _, cx| this.begin_tiling(cx))),
            )
            .child(
                div().text_xs().text_color(muted).child(
                    "只移动，保持当前大小。在本工具所在屏幕排列，放不下一行/列时自动换行/列。",
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("隐藏、最小化或最大化窗口需先手动显示或恢复为普通窗口。"),
            )
    }

    fn render_exit_failure(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(rgba(0x26395455))
            .child(
                div()
                    .w(px(560.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_5()
                    .rounded_lg()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("退出前恢复未完成"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child("部分窗口尚未完成显示或放大；未完成的请求仍可能继续生效。"),
                    )
                    .child(
                        div()
                            .id("exit-errors")
                            .min_h_0()
                            .max_h(px(180.))
                            .overflow_y_scroll()
                            .text_sm()
                            .text_color(rgb(0xb42318))
                            .children(
                                self.exit_errors
                                    .iter()
                                    .map(|error| div().py_1().child(error.clone())),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("force-exit")
                                    .label("仍然退出")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.quit(cx))),
                            )
                            .child(
                                Button::new("retry-exit")
                                    .primary()
                                    .label("重试并退出")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.begin(Action::Maximize, true, cx)
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for Scaler {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        div().id("window-scaler").relative().size_full().flex().flex_col()
            .font_family("Microsoft YaHei UI").bg(rgb(0xf8fafd)).text_color(cx.theme().foreground)
            .child(div().h(px(80.)).flex_shrink_0().px_5().flex().items_center().justify_between()
                .bg(cx.theme().background).border_b_1().border_color(border)
                .child(div().flex().items_center().gap_3()
                    .child(div().size(px(34.)).rounded_lg().bg(cx.theme().accent)
                        .flex().items_center().justify_center().text_color(cx.theme().primary)
                        .child(Icon::new(IconName::WindowRestore).size(px(20.))))
                    .child(div().flex().flex_col().gap_1()
                        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("窗口操作台"))
                        .child(div().text_xs().text_color(muted).child("列表选择，右侧集中操作"))))
                .child(div().text_xs().text_color(muted).child(format!("{} 个桌面窗口", self.windows.len()))))
            .when_some(self.tray_error.clone(), |root, error| root.child(div().px_5().py_2()
                .flex_shrink_0().text_xs().text_color(rgb(0xb42318)).bg(rgb(0xfff3f0)).child(error)))
            .child(div().flex_1().min_h_0().flex()
                .child(self.render_window_list(cx)).child(self.render_inspector(cx)))
            .child(div().h(px(42.)).flex_shrink_0().px_5().flex().items_center().gap_3()
                .bg(cx.theme().background).border_t_1().border_color(border)
                .child(div().size(px(6.)).rounded_full().bg(if self.busy || self.refreshing { rgb(0x3563e9) } else { rgb(0x648e7f) }))
                .child(div().flex_1().min_w_0().text_xs().text_color(muted).text_ellipsis().child(self.status.clone()))
                .child(Button::new("normal-exit").ghost().small().label("正常退出")
                    .icon(IconName::ArrowRight).disabled(self.exit_requested)
                    .tooltip("显示本次隐藏的窗口，并放大本次缩过的窗口后退出；关闭主界面会收起到托盘。")
                    .on_click(cx.listener(|this, _, _, cx| this.begin(Action::Maximize, true, cx)))))
            .when(self.exit_failed, |root| root.child(self.render_exit_failure(cx)))
    }
}
