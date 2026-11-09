use gpui_kit::{
    AnyView, App, AppContext, Context, Entity, EventEmitter, Focusable, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, StatefulInteractiveElement, Styled, Window,
    prelude::FluentBuilder, px,
};
use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::component::{
    Icon,
    dock::{PanelControl, PanelEvent},
    v_flex,
};

pub struct PageViewContainer {
    pub focus_handle: gpui_kit::FocusHandle,
    pub title: &'static str,
    pub icon: Icon,
    pub title_bg: Option<Hsla>,
    pub description: &'static str,
    width: Option<Pixels>,
    height: Option<Pixels>,
    page: Option<AnyView>,
    pub closable: bool,
    pub zoomable: Option<PanelControl>,
    paddings: Pixels,
    on_active: Option<fn(AnyView, bool, &mut Window, &mut App)>,
    pub breadcrumb_visible: bool,
    pub content_header_visible: bool,
    /// 是否只在激活母机上显示（机器管理等授权相关页面）。
    pub mother_only: bool,
}

#[derive(Debug)]
pub enum ContainerEvent {
    Close,
}

pub trait AppView: Render + Sized {
    fn title_key() -> &'static str {
        "default_title"
    }

    fn icon() -> Icon {
        Icon::new(AssetIconName::Settings)
    }

    fn description_key() -> &'static str {
        "default_description"
    }

    fn closable() -> bool {
        true
    }

    fn breadcrumb_visible() -> bool {
        true
    }

    fn content_header_visible() -> bool {
        true
    }

    /// 页面是否只对激活母机可见（漫游机不显示，搜索也搜不到）。
    fn mother_only() -> bool {
        false
    }

    fn zoomable() -> Option<PanelControl> {
        Some(PanelControl::default())
    }

    fn title_bg() -> Option<Hsla> {
        None
    }

    fn paddings() -> Pixels {
        px(16.)
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render>;

    fn on_active(&mut self, active: bool, window: &mut Window, cx: &mut App) {
        let _ = active;
        let _ = window;
        let _ = cx;
    }

    fn on_active_any(view: AnyView, active: bool, window: &mut Window, cx: &mut App)
    where
        Self: 'static,
    {
        if let Ok(page) = view.downcast::<Self>() {
            cx.update_entity(&page, |page, cx| {
                page.on_active(active, window, cx);
            });
        }
    }
}

impl EventEmitter<ContainerEvent> for PageViewContainer {}

impl PageViewContainer {
    pub fn new(_window: &mut Window, cx: &mut App) -> Self {
        let focus_handle = cx.focus_handle();

        Self {
            focus_handle,
            title: "default_title",
            icon: Icon::new(AssetIconName::Settings),
            title_bg: None,
            description: "default_description",
            width: None,
            height: None,
            page: None,
            closable: true,
            zoomable: Some(PanelControl::default()),
            paddings: px(16.),
            on_active: None,
            breadcrumb_visible: true,
            content_header_visible: true,
            mother_only: false,
        }
    }

    pub fn panel<S: AppView>(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let name = S::title_key();
        let description = S::description_key();
        let page = S::new_view(window, cx);

        cx.new(|cx| {
            let mut page = Self::new(window, cx)
                .page(page.into())
                .on_active(S::on_active_any);
            page.focus_handle = cx.focus_handle();
            page.closable = S::closable();
            page.zoomable = S::zoomable();
            page.title = name;
            page.icon = S::icon();
            page.description = description;
            page.title_bg = S::title_bg();
            page.paddings = S::paddings();
            page.breadcrumb_visible = S::breadcrumb_visible();
            page.content_header_visible = S::content_header_visible();
            page.mother_only = S::mother_only();
            page
        })
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn page(mut self, page: AnyView) -> Self {
        self.page = Some(page);
        self
    }

    pub fn on_active(mut self, on_active: fn(AnyView, bool, &mut Window, &mut App)) -> Self {
        self.on_active = Some(on_active);
        self
    }

    pub(crate) fn activate(&mut self, active: bool, window: &mut Window, cx: &mut App) {
        if let (Some(on_active), Some(page)) = (self.on_active, self.page.clone()) {
            on_active(page, active, window, cx);
        }
    }
}

impl EventEmitter<PanelEvent> for PageViewContainer {}
impl Focusable for PageViewContainer {
    fn focus_handle(&self, _: &App) -> gpui_kit::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PageViewContainer {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("page-container")
            .size_full()
            .overflow_y_scroll()
            .track_focus(&self.focus_handle)
            .when_some(self.page.clone(), |this, page| {
                this.child(
                    v_flex()
                        .id("page-children")
                        .w_full()
                        .flex_1()
                        .p(self.paddings)
                        .child(page),
                )
            })
    }
}
