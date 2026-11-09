// Prevent console window
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 演示程序：复用 LooTwo 的应用外壳（标题栏 / 侧边栏 / 页面容器），
//! 但只保留一个 Markdown 文档页，用它展示 `gpui-kit-markdown-extras`
//! 的公式与流程图渲染。

mod app;
mod app_pages;

use gpui_kit::assets::AllAssets;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

use app::{AppConfigState, AppRoot, AppView};

fn main() {
    let application = gpui_kit::application().with_assets(AllAssets);
    application.run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::component::set_locale("zh-CN");
        gpui_kit::component::Theme::update(cx, |theme| {
            theme.font_family = "Microsoft YaHei".into();
        });
        AppConfigState::init(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
            ..TitleBar::window_options()
        };

        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("GPUI Kit Box");
            let view = AppView::view(None, window, cx);
            cx.new(|cx| AppRoot::new(view, window, cx))
        })
        .expect("打开窗口失败");
    });
}
