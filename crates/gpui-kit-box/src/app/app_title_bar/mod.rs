mod app_logo;
mod font_size_selector;
pub mod theme_mode_selector;
pub mod theme_selector;

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, TitleBar, WindowExt as _};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, Styled as _, Window, div,
};

use crate::app::app_title_bar::app_logo::AppLogo;
use crate::app::app_title_bar::font_size_selector::FontSizeSelector;
use crate::app::app_title_bar::theme_mode_selector::ThemeModeSelector;
use crate::app::app_title_bar::theme_selector::ThemeSelector;

pub struct AppTitleBar {
    app_logo: Entity<AppLogo>,
    font_size_selector: Entity<FontSizeSelector>,
    theme_mode_selector: Entity<ThemeModeSelector>,
    theme_selector: Entity<ThemeSelector>,
    child: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}

impl AppTitleBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app_logo = cx.new(|cx| AppLogo::new(window, cx));
        let font_size_selector = cx.new(|cx| FontSizeSelector::new(window, cx));
        let theme_mode_selector = cx.new(|cx| ThemeModeSelector::new(window, cx));
        let theme_selector = cx.new(|cx| ThemeSelector::new(window, cx));

        Self {
            app_logo,
            font_size_selector,
            theme_mode_selector,
            theme_selector,
            child: Rc::new(|_, _| div().into_any_element()),
        }
    }

    pub fn child<F, E>(mut self, f: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut Window, &mut App) -> E + 'static,
    {
        self.child = Rc::new(move |window, cx| f(window, cx).into_any_element());
        self
    }
}

impl Render for AppTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notifications_count = window.notifications(cx).len();

        TitleBar::new()
            .child(div().flex().items_center().child(self.app_logo.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px_2()
                    .gap_2()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child((self.child.clone())(window, cx))
                    .child(self.font_size_selector.clone())
                    .child(self.theme_mode_selector.clone())
                    .child(self.theme_selector.clone())
                    .child(
                        div().relative().child(
                            Badge::new().count(notifications_count).max(99).child(
                                Button::new("bell")
                                    .small()
                                    .ghost()
                                    .compact()
                                    .icon(IconName::Bell),
                            ),
                        ),
                    ),
            )
    }
}
