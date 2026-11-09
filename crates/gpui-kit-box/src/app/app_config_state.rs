//! 运行期外观配置：主题、深浅模式、滚动条、侧边栏位置。
//!
//! 这是演示程序的精简版：**不落盘**（没有设置页，也没有业务配置），
//! 只在内存里维持当前状态，换主题 / 换模式立即生效。

use std::path::PathBuf;

use gpui_kit::base::ScrollbarMode;
use gpui_kit::component::{Side, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, BorrowAppContext as _, Global, SharedString};
use serde::Deserialize;

const THEME_DIR: &str = "./themes";

#[derive(Debug, Clone)]
pub struct AppConfigState {
    pub theme: SharedString,
    pub theme_mode: ThemeMode,
    pub theme_mode_s: SharedString,
    pub scrollbar_mode: ScrollbarMode,
    pub themes: Vec<String>,
    pub sider_bar_side: SharedString,
    pub auto_switch_theme: bool,
}

impl Default for AppConfigState {
    fn default() -> Self {
        Self {
            theme: "Default Light".into(),
            theme_mode: ThemeMode::Light,
            theme_mode_s: "Light".into(),
            scrollbar_mode: ScrollbarMode::Always,
            themes: Vec::new(),
            sider_bar_side: "Left".into(),
            auto_switch_theme: true,
        }
    }
}

impl Global for AppConfigState {}

impl AppConfigState {
    pub fn init(cx: &mut App) {
        cx.set_global::<AppConfigState>(Self::default());

        // 主题选择器只负责改全局状态，真正套用主题靠这个观察者：
        // 少了它，菜单点下去选中项会变，界面颜色却纹丝不动。
        cx.observe_global::<AppConfigState>(|cx| {
            let state = cx.global::<AppConfigState>().clone();
            apply_theme(&state, cx);
            cx.refresh_windows();
        })
        .detach();

        init_themes(cx);
        let state = cx.global::<AppConfigState>().clone();
        apply_theme(&state, cx);
    }
}

fn init_themes(cx: &mut App) {
    if let Err(err) = ThemeRegistry::watch_dir(PathBuf::from(THEME_DIR), cx, |cx| {
        refresh_theme_names(cx);
    }) {
        // 没有 themes/ 目录时 watch_dir 直接失败，这里不当作错误处理：
        // 下面仍会从注册表取一遍内置主题，保证调色板菜单不是空的。
        eprintln!("themes 目录不可用，仅使用内置主题: {err}");
    }
    refresh_theme_names(cx);
}

fn refresh_theme_names(cx: &mut App) {
    let names: Vec<String> = ThemeRegistry::global(cx)
        .sorted_themes()
        .iter()
        .map(|theme| theme.name.to_string())
        .collect();
    cx.update_global::<AppConfigState, _>(|state, _| {
        state.themes = names;
    });
}

pub fn apply_theme(app_config: &AppConfigState, cx: &mut App) {
    let mut theme_mode = get_theme_mode(app_config.theme_mode_s.clone());
    let theme_name = app_config.theme.clone();
    if let Some(theme) = ThemeRegistry::global(cx)
        .themes()
        .get(&theme_name)
        .cloned()
    {
        // 主题自带明暗属性时以它为准，并回写状态，避免选择器显示与实际不符。
        if theme_mode != theme.mode {
            theme_mode = theme.mode;
            cx.update_global::<AppConfigState, _>(|state, _| {
                state.theme_mode = theme_mode;
                state.theme_mode_s = theme_mode.name().into();
            });
        }
        Theme::global_mut(cx).apply_config(&theme);
    }
    Theme::change(theme_mode, None, cx);
    Theme::global_mut(cx).scrollbar_mode = app_config.scrollbar_mode;
}

pub fn get_theme_mode(theme_mode_s: SharedString) -> ThemeMode {
    if theme_mode_s.to_lowercase() == "dark" {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

pub fn theme_mode(value: &str) -> ThemeMode {
    if value.eq_ignore_ascii_case("dark") {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

pub fn get_side_bar_side(value: &str) -> Side {
    if value.eq_ignore_ascii_case("right") {
        Side::Right
    } else {
        Side::Left
    }
}

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectTheme(pub SharedString);

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectScrollbarMode(pub ScrollbarMode);

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectFont(pub usize);

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectRadius(pub usize);

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectSiderBarSide(pub SharedString);

#[derive(gpui_kit::Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = app_config, no_json)]
pub struct SelectThemeMode(pub ThemeMode);
