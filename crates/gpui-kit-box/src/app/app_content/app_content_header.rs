use crate::app::app_view::AppViewState;
use gpui_kit::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};

pub struct AppContentHeader {
    state: Entity<AppViewState>,
}

impl AppContentHeader {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>, state: Entity<AppViewState>) -> Self {
        Self { state }
    }
}

impl Render for AppContentHeader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        // 只说「当前在哪个页面」，不再挂数据库文件名的徽标：
        // 数据库名属于连接细节，界面上由「选择本地数据库」入口负责，重复显示只会干扰。
        h_flex()
            .id("header")
            .p_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .justify_between()
            .items_start()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xl().child(state.title))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(state.description),
                    ),
            )
    }
}
