pub mod app_breadcrumb;
pub mod app_content_data;
pub mod app_content_header;
pub mod not_found_view;

use crate::app::app_content::app_breadcrumb::AppBreadcrumb;
use crate::app::app_content::app_content_data::AppContentData;
use crate::app::app_content::app_content_header::AppContentHeader;
use crate::app::app_view::AppViewState;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};
use gpui_kit::component::v_flex;

pub struct AppContent {
    state: Entity<AppViewState>,
    app_breadcrumb: Entity<AppBreadcrumb>,
    app_content_header: Entity<AppContentHeader>,
    app_content_data: Entity<AppContentData>,
}

impl AppContent {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: Entity<AppViewState>,
        search_input: Entity<InputState>,
    ) -> Self {
        let app_breadcrumb = cx.new(|cx| AppBreadcrumb::new(window, cx, state.clone()));
        let app_content_header = cx.new(|cx| AppContentHeader::new(window, cx, state.clone()));
        let app_content_data =
            cx.new(|cx| AppContentData::new(window, cx, state.clone(), search_input));

        Self {
            state,
            app_breadcrumb,
            app_content_header,
            app_content_data,
        }
    }
}

impl Render for AppContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        v_flex()
            .h_full()
            .flex_1()
            .overflow_x_hidden()
            .when(st.breadcrumb_visible, |el| {
                el.child(self.app_breadcrumb.clone())
            })
            .when(st.content_header_visible, |el| {
                el.child(self.app_content_header.clone())
            })
            .child(self.app_content_data.clone())
            .into_any_element()
    }
}
