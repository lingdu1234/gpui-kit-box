use crate::app::app_sidebar::app_sidebar_footer::AppSidebarFooter;
use crate::app::app_sidebar::app_sidebar_header::AppSidebarHeader;
use crate::app::app_view::{AppViewState, get_active_page_info};
use gpui_kit::component::input::InputState;
use gpui_kit::component::sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem};
use gpui_kit::{AppContext, ClickEvent, Context, Entity, IntoElement, Render, Styled, Window, px};

pub mod app_sidebar_footer;
pub mod app_sidebar_header;

pub struct AppSidebar {
    state: Entity<AppViewState>,
    app_sidebar_header: Entity<AppSidebarHeader>,
    app_sidebar_footer: Entity<AppSidebarFooter>,
}

impl AppSidebar {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        search_input: Entity<InputState>,
        state: Entity<AppViewState>,
    ) -> Self {
        let app_sidebar_header =
            cx.new(|cx| AppSidebarHeader::new(window, cx, state.clone(), search_input.clone()));
        let app_sidebar_footer = cx.new(|cx| AppSidebarFooter::new(window, cx, state.clone()));
        Self {
            state,
            app_sidebar_header,
            app_sidebar_footer,
        }
    }
}

impl Render for AppSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let sidebar: Sidebar<SidebarGroup<SidebarMenu>> = Sidebar::new("app-sidebar")
            .w(px(180.0))
            .collapsed(state.sidebar_collapsed)
            .header(self.app_sidebar_header.clone())
            .children(state.show_pages.clone().into_iter().enumerate().map(
                |(group_ix, (group_name, sub_pages))| {
                    SidebarGroup::new(group_name).child(SidebarMenu::new().children(
                        sub_pages.iter().enumerate().map(|(ix, page)| {
                            SidebarMenuItem::new(page.read(cx).title)
                                .icon(page.read(cx).icon.clone())
                                .active(
                                    state.active_group_index == Some(group_ix)
                                        && state.active_index == Some(ix),
                                )
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    let active_page = this.state.update(cx, |st, cx| {
                                        let active_page = st
                                            .show_pages
                                            .get(group_ix)
                                            .and_then(|x| x.1.get(ix))
                                            .cloned();
                                        let (
                                            title,
                                            description,
                                            breadcrumb_visible,
                                            content_header_visible,
                                        ) = get_active_page_info(cx, &active_page);
                                        st.active_group_index = Some(group_ix);
                                        st.active_index = Some(ix);
                                        st.title = title;
                                        st.description = description;
                                        st.active_page = active_page.clone();
                                        st.breadcrumb_visible = breadcrumb_visible;
                                        st.content_header_visible = content_header_visible;
                                        cx.notify();
                                        active_page
                                    });
                                    if let Some(page) = active_page {
                                        page.update(cx, |page, cx| {
                                            page.activate(true, window, cx);
                                        });
                                    }
                                }))
                        }),
                    ))
                },
            ))
            .footer(self.app_sidebar_footer.clone());
        sidebar
    }
}
