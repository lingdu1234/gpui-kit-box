//! 文档页：按图表类型拆成多个页面，正文在编译期嵌入。
//!
//! 正文随可执行文件发布，不读磁盘：`include_str!` 在编译期把 Markdown 嵌进来。
//! 文档的唯一来源是 `gpui-kit-markdown-extras/showcase/`，应用页与那个 crate 的
//! 回归测试读同一批文件；改文档后需要重新编译。
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

/// 各页面共用的正文渲染：只是嵌入的 Markdown 源码不同。
fn render_doc(
    id: &'static str,
    source: &'static str,
    cx: &mut Context<impl Render>,
) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .gap_3()
        .child(div().text_xs().text_color(theme.muted_foreground).child(
            "内容随软件发布，编辑 gpui-kit-markdown-extras/showcase/ 下的 Markdown 后需重新编译。",
        ))
        .child(
            div()
                .w_full()
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius_lg)
                .p_6()
                .text_color(theme.foreground)
                .child(
                    gpui_kit_markdown_extras::with_plugins(TextView::markdown(id, source))
                        .selectable(true),
                ),
        )
}

/// 展示文档的唯一来源在兄弟 crate 里。用 `CARGO_MANIFEST_DIR` 拼绝对路径，
/// 避免 `include_str!` 的相对层级随本文件移动而失效。
macro_rules! showcase_source {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../gpui-kit-markdown-extras/showcase/",
            $name
        ))
    };
}

/// 每个示例页只需声明标题、图标与正文，其余实现完全一致。
macro_rules! doc_page {
    ($name:ident, $title:expr, $description:expr, $icon:ident, $doc_id:expr, $source:expr) => {
        pub struct $name;

        impl $name {
            const SOURCE: &'static str = $source;
        }

        impl AppView for $name {
            fn title_key() -> &'static str {
                $title
            }

            fn icon() -> Icon {
                Icon::new(IconName::$icon)
            }

            fn description_key() -> &'static str {
                $description
            }

            fn new_view(_window: &mut Window, cx: &mut App) -> Entity<impl Render> {
                cx.new(|_| Self)
            }
        }

        impl Render for $name {
            fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                render_doc($doc_id, Self::SOURCE, cx)
            }
        }
    };
}

doc_page!(
    MarkdownBasicPage,
    "Markdown 基础",
    "列表、引用、代码块、强调与表格",
    BookOpenText,
    "markdown-doc",
    showcase_source!("markdown.md")
);

doc_page!(
    FormulaPage,
    "公式",
    "行内与块级公式：统计、回归、校准曲线与数学记号",
    Sigma,
    "formula-doc",
    showcase_source!("math.md")
);

doc_page!(
    FlowchartPage,
    "流程图",
    "flowchart：分层、扇出扇入、回边与菱形判断",
    Workflow,
    "flowchart-doc",
    showcase_source!("flowchart.md")
);

doc_page!(
    SequencePage,
    "时序图",
    "sequenceDiagram：消息、激活、注释与组合片段",
    ArrowLeftRight,
    "sequence-doc",
    showcase_source!("sequence.md")
);

doc_page!(
    ClassPage,
    "类图",
    "classDiagram：成员、关系符号、多重度与注释",
    Boxes,
    "class-doc",
    showcase_source!("class.md")
);

doc_page!(
    StatePage,
    "状态图",
    "stateDiagram-v2：起止圆、选择/同步与复合状态",
    SquareActivity,
    "state-doc",
    showcase_source!("state.md")
);

doc_page!(
    GanttPage,
    "甘特图",
    "gantt：任务状态、依赖、里程碑与工作日轴",
    ChartGantt,
    "gantt-doc",
    showcase_source!("gantt.md")
);
