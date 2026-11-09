use crate::app::AppConfigState;
use crate::app::app_config_state::get_side_bar_side;
use crate::app::app_view::AppViewState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window};
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::h_flex;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::sidebar::SidebarToggleButton;

pub struct AppBreadcrumb {
    state: Entity<AppViewState>,
}

impl AppBreadcrumb {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>, state: Entity<AppViewState>) -> Self {
        Self { state }
    }
}

impl Render for AppBreadcrumb {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let side = get_side_bar_side(&cx.global::<AppConfigState>().sider_bar_side);
        let state = self.state.read(cx);
        h_flex()
            .pt_2()
            .px_2()
            .items_center()
            .gap_3()
            .when(side.is_right(), |this| {
                this.flex_row_reverse().justify_between()
            })
            .child(
                SidebarToggleButton::new()
                    .side(side)
                    .collapsed(state.sidebar_collapsed)
                    .accessibility_label("折叠侧边栏")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.update(cx, |st, cx| {
                            st.sidebar_collapsed = !st.sidebar_collapsed;
                            cx.notify();
                        });
                    })),
            )
            .child(Separator::vertical().h_4())
            .child(
                Breadcrumb::new()
                    .child(BreadcrumbItem::new("首页"))
                    .child(BreadcrumbItem::new(state.title)),
            )
    }
}
