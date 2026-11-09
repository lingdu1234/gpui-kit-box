use crate::app::app_content::AppContent;
use crate::app::app_sidebar::AppSidebar;
use crate::app::app_view_container::PageViewContainer;
use crate::app::{AppConfigState, app_config_state::get_side_bar_side};
use crate::app_pages::manual_page::{
    ClassPage, FlowchartPage, FormulaPage, GanttPage, MarkdownBasicPage, SequencePage, StatePage,
};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    ReadGlobal, Render, Styled, Subscription, Window,
};

pub struct AppViewState {
    pub title: &'static str,
    pub description: &'static str,
    pub pages: Vec<(&'static str, Vec<Entity<PageViewContainer>>)>,
    pub show_pages: Vec<(&'static str, Vec<Entity<PageViewContainer>>)>,
    pub active_page: Option<Entity<PageViewContainer>>,
    pub active_group_index: Option<usize>,
    pub active_index: Option<usize>,
    pub sidebar_collapsed: bool,
    pub breadcrumb_visible: bool,
    pub content_header_visible: bool,
}

impl AppViewState {
    /// 按关键词过滤页面列表并重算激活页：搜索框的输入回调和 404 页的「清除搜索」
    /// 都走这里，`InputState::set_value` 不会发 Change 事件，不能只清文本。
    pub fn apply_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_lowercase();
        let show_pages: Vec<(&'static str, Vec<Entity<PageViewContainer>>)> = self
            .pages
            .iter()
            .filter_map(|(name, items)| {
                let filtered_items: Vec<_> = items
                    .iter()
                    .filter(|page| page.read(cx).title.to_lowercase().contains(&query))
                    .cloned()
                    .collect();

                if filtered_items.is_empty() {
                    None
                } else {
                    Some((*name, filtered_items))
                }
            })
            .collect();
        let (active_group_index, active_index, active_page) =
            get_active_page_by_search(self, &show_pages);
        let (title, description, breadcrumb_visible, content_header_visible) =
            get_active_page_info(cx, &active_page);

        self.title = title;
        self.description = description;
        self.show_pages = show_pages;
        self.active_page = active_page;
        self.active_group_index = active_group_index;
        self.active_index = active_index;
        self.breadcrumb_visible = breadcrumb_visible;
        self.content_header_visible = content_header_visible;
        cx.notify();
    }
}

pub struct AppView {
    state: Entity<AppViewState>,
    search_input: Entity<InputState>,
    app_sidebar: Entity<AppSidebar>,
    app_content: Entity<AppContent>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(_init_page: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 演示程序只保留 Markdown 示例页：按图表类型拆开，方便逐类验证。
        let pages = vec![
            (
                "文档",
                vec![
                    PageViewContainer::panel::<MarkdownBasicPage>(window, cx),
                    PageViewContainer::panel::<FormulaPage>(window, cx),
                ],
            ),
            (
                "Mermaid 图表",
                vec![
                    PageViewContainer::panel::<FlowchartPage>(window, cx),
                    PageViewContainer::panel::<SequencePage>(window, cx),
                    PageViewContainer::panel::<ClassPage>(window, cx),
                    PageViewContainer::panel::<StatePage>(window, cx),
                    PageViewContainer::panel::<GanttPage>(window, cx),
                ],
            ),
        ];

        let state = cx.new(|cx| {
            let (active_group_index, active_index, active_page) = get_first_page(&pages);
            let (title, description, breadcrumb_visible, content_header_visible) =
                get_active_page_info(cx, &active_page);
            AppViewState {
                title,
                description,
                pages: pages.clone(),
                show_pages: pages.clone(),
                active_page,
                active_group_index,
                active_index,
                sidebar_collapsed: false,
                breadcrumb_visible,
                content_header_visible,
            }
        });

        if let Some(page) = state.read(cx).active_page.clone() {
            page.update(cx, |page, cx| page.activate(true, window, cx));
        }

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索页面…"));
        let app_sidebar =
            cx.new(|cx| AppSidebar::new(window, cx, search_input.clone(), state.clone()));
        let app_content =
            cx.new(|cx| AppContent::new(window, cx, state.clone(), search_input.clone()));
        let _subscriptions = vec![cx.subscribe(
            &search_input,
            |this, _x, event: &gpui_kit::component::input::InputEvent, cx| {
                if !matches!(event, gpui_kit::component::input::InputEvent::Change) {
                    return;
                }
                let query = this.search_input.read(cx).value().to_string();
                let state = this.state.clone();
                state.update(cx, |st, cx| st.apply_search(&query, cx));
            },
        )];

        Self {
            state,
            search_input,
            app_sidebar,
            app_content,
            _subscriptions,
        }
    }

    pub fn view(init_page: Option<&str>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(init_page, window, cx))
    }
}

pub fn get_active_page_by_search(
    state: &AppViewState,
    show_pages: &[(&'static str, Vec<Entity<PageViewContainer>>)],
) -> (
    Option<usize>,
    Option<usize>,
    Option<Entity<PageViewContainer>>,
) {
    let current_show = state
        .show_pages
        .get(state.active_group_index.unwrap_or(0))
        .and_then(|x| x.1.get(state.active_index.unwrap_or(0)).cloned());
    match current_show {
        None => get_first_page(show_pages),
        Some(current) => {
            let found = show_pages
                .iter()
                .position(|(_, xs)| xs.contains(&current));
            match found {
                None => get_first_page(show_pages),
                Some(group_index) => {
                    let index = show_pages
                        .get(group_index)
                        .and_then(|(_, xs)| xs.iter().position(|page| page == &current))
                        .unwrap_or(0);
                    (Some(group_index), Some(index), state.active_page.clone())
                }
            }
        }
    }
}

fn get_first_page(
    show_pages: &[(&'static str, Vec<Entity<PageViewContainer>>)],
) -> (
    Option<usize>,
    Option<usize>,
    Option<Entity<PageViewContainer>>,
) {
    let active_page = show_pages.first().and_then(|x| x.1.first()).cloned();
    (Some(0), Some(0), active_page)
}

pub fn get_active_page_info(
    cx: &mut Context<AppViewState>,
    active_page: &Option<Entity<PageViewContainer>>,
) -> (&'static str, &'static str, bool, bool) {
    match active_page.as_ref().map(|page| page.read(cx)) {
        // 404 由内容区的空状态自己说话，页头再写一遍会重复；面包屑要留着，侧栏折叠按钮在它里面。
        None => ("404", "未找到页面", true, false),
        Some(page) => (
            page.title,
            page.description,
            page.breadcrumb_visible,
            page.content_header_visible,
        ),
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let side = get_side_bar_side(&AppConfigState::global(cx).sider_bar_side);
        h_flex()
            .id("app-container")
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .h_full()
            .when(side.is_right(), |this| this.flex_row_reverse())
            .child(self.app_sidebar.clone())
            .child(self.app_content.clone())
    }
}
