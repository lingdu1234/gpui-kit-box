//! 文档页：按图表类型拆成多个页面，正文在编译期嵌入。
//!
//! 正文随可执行文件发布，不读磁盘：`include_str!` 在编译期把 Markdown 嵌进来。
//! 文档的唯一来源是 `gpui-kit-markdown-extras/showcase/`，应用页与那个 crate 的
//! 回归测试读同一批文件；改文档后需要重新编译。
//!
//! 滚动交给 `PageViewContainer`（面板本身已 `overflow_y_scroll`），
//! 这里让 TextView 按内容固有高度铺开，避免在弹性容器里再套一层滚动。
//!
//! 每个页面用链式 `.plugin(...)` 只挂自己需要的插件：公式页不挂 Mermaid，各图
//! 页只挂对应那一类。这样每个视图的解析器数量都是最小的，也演示了
//! `gpui-kit-markdown-extras` 的按需用法（`with_plugins` 是「全挂上」的便捷写法）。

use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_kit_markdown_extras::prelude::*;

use crate::app::app_view_container::AppView;

/// 各页面共用的正文渲染：嵌入的 Markdown 源码与要挂的插件不同。
///
/// 插件在 render 里构造是零成本的：`gpui-base` 判断扩展能否影响解析树时只比
/// parser 的形状（数量与名字），不比重建出来的对象身份，所以每帧新建插件不会
/// 触发重新解析。
fn render_doc(
    id: &'static str,
    source: &'static str,
    plugins: fn(TextView) -> TextView,
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
                .child(plugins(TextView::markdown(id, source)).selectable(true)),
        )
}

// ---------------------------------------------------------------------------
// 各页挂哪些插件
// ---------------------------------------------------------------------------

/// 基础页不挂插件：`TextView` 保持原生 Markdown 行为。
fn no_plugins(view: TextView) -> TextView {
    view
}

/// 公式页只挂公式插件：解析时不会去试 Mermaid 围栏。
fn math_plugins(view: TextView) -> TextView {
    view.plugin(MathBlockPlugin::new())
        .plugin(InlineMathPlugin::new())
}

/// 每类图只挂自己的插件，解析器数量降到 1。
fn flowchart_plugins(view: TextView) -> TextView {
    view.plugin(FlowchartPlugin::new())
}

fn sequence_plugins(view: TextView) -> TextView {
    view.plugin(SequencePlugin::new())
}

fn class_plugins(view: TextView) -> TextView {
    view.plugin(ClassPlugin::new())
}

fn state_plugins(view: TextView) -> TextView {
    view.plugin(StatePlugin::new())
}

fn gantt_plugins(view: TextView) -> TextView {
    view.plugin(GanttPlugin::new())
}

// ---------------------------------------------------------------------------
// 页面定义
// ---------------------------------------------------------------------------

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

/// 每个示例页只需声明标题、图标、正文与插件，其余实现完全一致。
macro_rules! doc_page {
    ($name:ident, $title:expr, $description:expr, $icon:ident, $doc_id:expr, $source:expr, $plugins:path) => {
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
                render_doc($doc_id, Self::SOURCE, $plugins, cx)
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
    showcase_source!("markdown.md"),
    no_plugins
);

doc_page!(
    FormulaPage,
    "公式",
    "行内与块级公式：统计、回归、校准曲线与数学记号",
    Sigma,
    "formula-doc",
    showcase_source!("math.md"),
    math_plugins
);

doc_page!(
    FlowchartPage,
    "流程图",
    "flowchart：分层、扇出扇入、回边与菱形判断",
    Workflow,
    "flowchart-doc",
    showcase_source!("flowchart.md"),
    flowchart_plugins
);

doc_page!(
    SequencePage,
    "时序图",
    "sequenceDiagram：消息、激活、注释与组合片段",
    ArrowLeftRight,
    "sequence-doc",
    showcase_source!("sequence.md"),
    sequence_plugins
);

doc_page!(
    ClassPage,
    "类图",
    "classDiagram：成员、关系符号、多重度与注释",
    Boxes,
    "class-doc",
    showcase_source!("class.md"),
    class_plugins
);

doc_page!(
    StatePage,
    "状态图",
    "stateDiagram-v2：起止圆、选择/同步与复合状态",
    SquareActivity,
    "state-doc",
    showcase_source!("state.md"),
    state_plugins
);

doc_page!(
    GanttPage,
    "甘特图",
    "gantt：任务状态、依赖、里程碑与周末标灰",
    ChartGantt,
    "gantt-doc",
    showcase_source!("gantt.md"),
    gantt_plugins
);
