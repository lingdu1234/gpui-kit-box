use gpui_kit::component::label::Label;
use gpui_kit::component::{Icon, v_flex};
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div};

pub struct AppLogo;

impl AppLogo {
    pub fn new(_: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for AppLogo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_start()
            .px_2()
            .gap_2()
            .child(Icon::new(crate::app::app_icon::APP_ICON))
            .child(
                v_flex()
                    .gap_0()
                    .child(Label::new("LooTwo")),
            )
    }
}
