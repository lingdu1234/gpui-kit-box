use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::DropdownMenu as _,
};
use gpui_kit::{
    Anchor, BorrowAppContext, Context, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Window, div, px,
};

use crate::app::{AppConfigState, app_config_state::SelectTheme};

pub struct ThemeSelector {
    focus_handle: FocusHandle,
}

impl ThemeSelector {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    fn on_select_theme(&mut self, theme: &SelectTheme, _window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<AppConfigState, _>(|state, _| {
            state.theme = theme.0.clone();
        });
    }
}

impl Render for ThemeSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let themes = cx.global::<AppConfigState>().themes.clone();
        let theme = cx.theme().theme_name().to_string();
        div()
            .id("theme-selector")
            .track_focus(&focus_handle)
            .on_action(cx.listener(Self::on_select_theme))
            .child(
                Button::new("theme_btn")
                    .small()
                    .ghost()
                    .icon(IconName::Palette)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |this, _, _| {
                        let mut this = this.scrollable(true).max_h(px(580.)).label("主题");
                        for th in &themes {
                            this = this.menu_with_check(
                                th.clone(),
                                theme == th.clone(),
                                Box::new(SelectTheme(SharedString::from(th.clone()))),
                            );
                        }
                        this.min_w(px(100.))
                    }),
            )
    }
}
