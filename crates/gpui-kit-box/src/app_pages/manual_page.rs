//! 文档页：把 `docs/usage-guide.md` 编译进程序后渲染。
//!
//! 正文随可执行文件发布，不读磁盘：`include_str!` 在编译期把 Markdown 嵌进来。
//! 改文档后需要重新编译。
//!
//! 滚动交给 `PageViewContainer`（面板本身已 `overflow_y_scroll`），
//! 这里让 TextView 按内容固有高度铺开，避免在弹性容器里再套一层滚动。
//!
//! 公式与流程图由 `gpui-kit-markdown-extras` 提供（`with_plugins`）。

use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};

use crate::app::app_view_container::AppView;

/// 验证文档正文（编译期嵌入）。内容只用于验证 crate 的渲染能力，
/// 不包含任何实际项目的业务资料。
const MANUAL: &str = include_str!("../../docs/showcase.md");

pub struct ManualPage;

impl AppView for ManualPage {
    fn title_key() -> &'static str {
        "Markdown 验证"
    }

    fn icon() -> Icon {
        Icon::new(IconName::BookOpenText)
    }

    fn description_key() -> &'static str {
        "Markdown 文档示例：公式与 Mermaid 流程图"
    }

    fn new_view(_window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        cx.new(|_| Self)
    }
}

impl Render for ManualPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("内容随软件发布，与仓库中的 docs/usage-guide.md 一致。"),
            )
            .child(
                div()
                    .w_full()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius_lg)
                    .p_6()
                    .text_color(theme.foreground)
                    .child(
                        gpui_kit_markdown_extras::with_plugins(TextView::markdown(
                            "manual-body",
                            MANUAL,
                        ))
                        .selectable(true),
                    ),
            )
    }
}
