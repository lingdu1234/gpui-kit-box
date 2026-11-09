use crate::app::app_view::AppViewState;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::sidebar::SidebarHeader;
use gpui_kit::component::{ActiveTheme, Icon, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    ClickEvent, Context, Entity, IntoElement, IsZero, ParentElement, Render, Styled, Window, div,
    px, relative,
};

pub struct AppSidebarHeader {
    state: Entity<AppViewState>,
    search_input: Entity<InputState>,
}

impl AppSidebarHeader {
    pub fn new(
        _: &mut Window,
        _cx: &mut Context<Self>,
        state: Entity<AppViewState>,
        search_input: Entity<InputState>,
    ) -> Self {
        Self {
            state,
            search_input,
        }
    }
}

impl Render for AppSidebarHeader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        v_flex()
            .w_full()
            .gap_4()
            .child(
                SidebarHeader::new()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .size_8()
                            .flex_shrink_0()
                            .when(!state.sidebar_collapsed, |this| {
                                this.child(Icon::new(crate::app::app_icon::APP_ICON))
                            })
                            .when(state.sidebar_collapsed, |this| {
                                this.size_4()
                                    .bg(cx.theme().transparent)
                                    .text_color(cx.theme().foreground)
                                    .child(
                                        Button::new("logo-button")
                                            .icon(Icon::new(crate::app::app_icon::APP_ICON))
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, _, cx| {
                                                    this.state.update(cx, |st, cx| {
                                                        st.sidebar_collapsed = false;
                                                        cx.notify();
                                                    });
                                                },
                                            )),
                                    )
                            })
                            .rounded_lg(),
                    )
                    .when(!state.sidebar_collapsed, |this| {
                        this.child(
                            v_flex()
                                .gap_0()
                                .text_sm()
                                .flex_1()
                                .line_height(relative(1.25))
                                .overflow_hidden()
                                .text_ellipsis()
                                .child("LooTwo")
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("X · tool")
                                        .text_xs(),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .bg(cx.theme().sidebar_accent)
                    .rounded_full()
                    .when(cx.theme().radius.is_zero(), |this| this.rounded(px(0.)))
                    .flex_1()
                    .mx_1()
                    .when(state.sidebar_collapsed, |this| {
                        this.flex().items_center().justify_center().child(
                            Button::new("search-icon-button")
                                .icon(crate::app::app_icon::SEARCH_ICON)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.state.update(cx, |st, cx| {
                                        st.sidebar_collapsed = false;
                                        cx.notify();
                                    });
                                })),
                        )
                    })
                    .when(!state.sidebar_collapsed, |this| {
                        this.child(
                            Input::new(&self.search_input)
                                .appearance(false)
                                .cleanable(true),
                        )
                    }),
            )
    }
}
