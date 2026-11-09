use gpui_kit::{
    AnyView, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Window, div,
};
use gpui_kit::component::v_flex;

use crate::app::app_title_bar::AppTitleBar;

/// Window content: custom title bar on top, the page shell below.
pub struct AppRoot {
    focus_handle: FocusHandle,
    title_bar: Entity<AppTitleBar>,
    view: AnyView,
}

impl AppRoot {
    pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title_bar = cx.new(|cx| AppTitleBar::new(window, cx));
        Self {
            focus_handle: cx.focus_handle(),
            title_bar,
            view: view.into(),
        }
    }
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("page-root")
            .size_full()
            .child(
                v_flex().size_full().child(self.title_bar.clone()).child(
                    div()
                        .track_focus(&self.focus_handle)
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.view.clone()),
                ),
            )
    }
}
