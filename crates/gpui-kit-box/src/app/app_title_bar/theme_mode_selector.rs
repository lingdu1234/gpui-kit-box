use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{Sizable as _, ThemeMode};
use gpui_kit::{
    Anchor, BorrowAppContext, Context, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Window, div, px,
};

use crate::app::AppConfigState;
use crate::app::app_config_state::SelectThemeMode;


pub struct ThemeModeSelector {
    focus_handle: FocusHandle,
}

impl ThemeModeSelector {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    fn on_select_theme_mode(
        &mut self,
        theme_mode: &SelectThemeMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = theme_mode.0;
        let t = if mode.is_dark() {
            "Default Dark"
        } else {
            "Default Light"
        };
        cx.update_global::<AppConfigState, _>(|state, _| {
            state.theme = t.into();
            state.theme_mode = mode;
            state.theme_mode_s = mode.name().into();
        });
    }
}

impl Render for ThemeModeSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let dark = crate::app::app_config_state::theme_mode(&cx.global::<AppConfigState>().theme_mode_s)
            == ThemeMode::Dark;
        div()
            .id("theme-mode-selector")
            .track_focus(&focus_handle)
            .on_action(cx.listener(Self::on_select_theme_mode))
            .child(
                Button::new("theme-mode-btn")
                    .small()
                    .ghost()
                    .icon(if dark { IconName::Moon } else { IconName::Sun })
                    .dropdown_menu(move |this, _, _| {
                        this.scrollable(true)
                            .max_h(px(480.))
                            .label("主题模式")
                            .menu_with_check("浅色", !dark, Box::new(SelectThemeMode(ThemeMode::Light)))
                            .menu_with_check("深色", dark, Box::new(SelectThemeMode(ThemeMode::Dark)))
                            .separator()
                    })
                    .anchor(Anchor::TopRight),
            )
    }
}
