use crate::app::app_view::AppViewState;
use gpui_kit::assets::IconName;
use gpui_kit::component::sidebar::SidebarFooter;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};

pub struct AppSidebarFooter {
    state: Entity<AppViewState>,
}

impl AppSidebarFooter {
    pub fn new(_: &mut Window, _cx: &mut Context<Self>, state: Entity<AppViewState>) -> Self {
        Self { state }
    }
}

impl Render for AppSidebarFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 演示程序没有授权体系，底部这一行固定显示程序名。
        let username = "GPUI Kit Box";
        let muted = cx.theme().muted_foreground;
        let collapsed = self.state.read(cx).sidebar_collapsed;
        v_flex().w_full().child(
            SidebarFooter::new()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_1()
                        .min_w_0()
                        .child(crate::app::app_icon::USER_ICON)
                        .when(!collapsed, |this| {
                            // 长账号名（工号+姓名）不能撑破侧边栏：让文本自己截断。
                            this.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(username),
                            )
                        }),
                )
                .when(!collapsed, |this| {
                    this.child(Icon::new(IconName::ChevronsUpDown).size_4().flex_shrink_0())
                }),
        )
    }
}
