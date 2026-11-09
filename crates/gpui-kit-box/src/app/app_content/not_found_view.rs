//! 搜索没有命中任何页面时的 404 空状态：图标、命中的关键词、清除搜索的入口。
//! 只在 `AppViewState.active_page` 为空（即过滤后一个页面都不剩）时挂在内容区。

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    button::{Button, ButtonVariants as _},
    h_flex, input::InputState, v_flex,
};
use gpui_kit::{
    App, Entity, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px,
    rems, relative,
};

use crate::app::app_view::AppViewState;

#[derive(IntoElement)]
pub struct NotFoundView {
    query: SharedString,
    total_pages: usize,
    search_input: Entity<InputState>,
    state: Entity<AppViewState>,
}

impl NotFoundView {
    pub fn new(
        query: impl Into<SharedString>,
        total_pages: usize,
        search_input: Entity<InputState>,
        state: Entity<AppViewState>,
    ) -> Self {
        Self {
            query: query.into(),
            total_pages,
            search_input,
            state,
        }
    }
}

impl RenderOnce for NotFoundView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        // 关键词为空却查不到页面（例如还没有任何页面注册），别让引用里是空串。
        let query = if self.query.is_empty() {
            SharedString::from("(空)")
        } else {
            self.query.clone()
        };
        let hint = SharedString::from(format!("共 {} 个页面可选", self.total_pages));

        let input = self.search_input.clone();
        let state = self.state.clone();
        let clear = Button::new("nf-clear")
            .label("清除搜索")
            .primary()
            .on_click(move |_, window, cx| {
                // set_value 不会发 InputEvent::Change，过滤要自己再跑一遍。
                input.update(cx, |input, cx| input.set_value("", window, cx));
                state.update(cx, |st, cx| st.apply_search("", cx));
            });

        v_flex()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(96.))
                    .rounded_full()
                    .bg(theme.muted.opacity(0.45))
                    .text_size(rems(2.75))
                    .text_color(muted)
                    .child(Icon::new(IconName::SearchX)),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(3.5))
                            .line_height(relative(1.))
                            .text_color(theme.foreground)
                            .child("404"),
                    )
                    .child(
                        div()
                            .text_xl()
                            .text_color(theme.foreground)
                            .child("没有匹配的页面"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_sm()
                            .text_color(muted)
                            .child("搜索")
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_color(theme.foreground)
                                    .child(format!("“{query}”")),
                            )
                            .child("没有命中任何页面，换个关键词试试。"),
                    ),
            )
            .child(clear)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(hint),
            )
    }
}
