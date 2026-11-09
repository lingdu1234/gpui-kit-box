use gpui_kit::assets::IconName;
use gpui_kit::base::ScrollbarMode;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Theme};
use gpui_kit::{
    Anchor, BorrowAppContext, Context, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Window, div, px,
};

use crate::app::AppConfigState;
use crate::app::app_config_state::{SelectFont, SelectRadius, SelectScrollbarMode, SelectSiderBarSide};

pub struct FontSizeSelector {
    focus_handle: FocusHandle,
}

impl FontSizeSelector {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }


        fn on_select_scrollbar_mode(
        &mut self,
        show: &SelectScrollbarMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Theme::global_mut(cx).scrollbar_mode = show.0;
        cx.update_global::<AppConfigState, _>(|state, _| {
            state.scrollbar_mode = show.0.clone();
        });
        window.refresh();
    }

    fn on_select_font(
        &mut self,
        font_size: &SelectFont,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Theme::global_mut(cx).font_size = px(font_size.0 as f32);
        window.refresh();
    }
        fn on_select_radius(
        &mut self,
        radius: &SelectRadius,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Theme::global_mut(cx).radius = px(radius.0 as f32);

        window.refresh();
    }

        fn on_select_side_bar_side(
        &mut self,
        side: &SelectSiderBarSide,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.update_global::<AppConfigState, _>(|state, _| {
            state.sider_bar_side = side.0.clone();
        });
        cx.notify();
        // window.refresh();
    }
      
}

impl Render for FontSizeSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Theme::global_mut(cx).scrollbar_mode = ScrollbarMode::Hover; //先将滚动条设置为悬停显示
        let focus_handle = self.focus_handle.clone();
        let font_size = cx.theme().font_size.as_f32() as i32;
        let radius = cx.theme().radius.as_f32() as i32;
        let scroll_show = cx.theme().scrollbar_mode; //默认为滚动式才滚动条显示
        let side = cx.global::<AppConfigState>().sider_bar_side.clone();

        div()
            .id("font-size-selector")
            .track_focus(&focus_handle)
            .on_action(cx.listener(Self::on_select_font))
            .on_action(cx.listener(Self::on_select_radius))
            .on_action(cx.listener(Self::on_select_scrollbar_mode))
            .on_action(cx.listener(Self::on_select_side_bar_side))
            .child(
                Button::new("layout-settings-btn")
                    .small()
                    .ghost()
                    .icon(IconName::Settings2)
                    .dropdown_menu(move |this, _, _| {
                        this.scrollable(true)
                            .max_h(px(480.))
                            .label("布局")
                            .menu_with_check("侧边栏在左", side == "Left", Box::new(SelectSiderBarSide("Left".into())))
                            .menu_with_check("侧边栏在右", side == "Right", Box::new(SelectSiderBarSide("Right".into())))
                            .separator()
                            .label("字号")
                            .menu_with_check("大", font_size == 18, Box::new(SelectFont(18)))
                            .menu_with_check(
                                "中（默认）",
                                font_size == 16,
                                Box::new(SelectFont(16)),
                            )
                            .menu_with_check("小", font_size == 14, Box::new(SelectFont(14)))
                            .separator()
                            
                            .label("Border Radius")
                            .menu_with_check("8px", radius == 8, Box::new(SelectRadius(8)))
                            .menu_with_check(
                                "6px (default)",
                                radius == 6,
                                Box::new(SelectRadius(6)),
                            )
                            .menu_with_check("4px", radius == 4, Box::new(SelectRadius(4)))
                            .menu_with_check("0px", radius == 0, Box::new(SelectRadius(0)))
                            .separator()
                            .label("Scrollbar")
                            .menu_with_check(
                                "Scrolling to show",
                                scroll_show == ScrollbarMode::Scrolling,
                                Box::new(SelectScrollbarMode(ScrollbarMode::Scrolling)),
                            )
                            .menu_with_check(
                                "Hover to show",
                                scroll_show == ScrollbarMode::Hover,
                                Box::new(SelectScrollbarMode(ScrollbarMode::Hover)),
                            )
                            .menu_with_check(
                                "Always show",
                                scroll_show == ScrollbarMode::Always,
                                Box::new(SelectScrollbarMode(ScrollbarMode::Always)),
                            )
                    })
                    .anchor(Anchor::TopRight),
            )
    }
}
