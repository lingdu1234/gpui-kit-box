use super::not_found_view::NotFoundView;
use crate::app::app_view::AppViewState;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div,
};

pub struct AppContentData {
    state: Entity<AppViewState>,
    search_input: Entity<InputState>,
}

impl AppContentData {
    pub fn new(
        _window: &mut Window,
        _cx: &mut Context<Self>,
        state: Entity<AppViewState>,
        search_input: Entity<InputState>,
    ) -> Self {
        Self { state, search_input }
    }
}

impl Render for AppContentData {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let page = state.active_page.clone();
        // 搜索过滤后一个页面都不剩，用 404 页占住内容区，别让界面一片空白。
        let not_found = page.is_none().then(|| {
            let total_pages = state
                .pages
                .iter()
                .map(|(_, items)| items.len())
                .sum::<usize>();
            NotFoundView::new(
                self.search_input.read(cx).value(),
                total_pages,
                self.search_input.clone(),
                self.state.clone(),
            )
        });

        div()
            .id("page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when_some(page, |this, active_page| this.child(active_page.clone()))
            .when_some(not_found, |this, view| this.child(view))
    }
}
