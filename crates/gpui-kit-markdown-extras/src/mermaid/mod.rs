//! Markdown 插件：Mermaid 图表的围栏识别与分发。
//!
//! **每种图一个独立插件**，各自一个 Cargo feature、各自一个 `MarkdownNode`
//! 名字：`FlowchartPlugin` / `SequencePlugin` / `ClassPlugin` / `StatePlugin` /
//! `GanttPlugin`。一段文档要画哪几类图，就在 `TextView` 上链几个 `.plugin(...)`：
//!
//! ```ignore
//! use gpui_kit::component::text::markdown;
//! use gpui_kit_markdown_extras::prelude::*;
//!
//! markdown(SOURCE)
//!     .plugin(FlowchartPlugin::new())
//!     .plugin(GanttPlugin::new())
//! ```
//!
//! 每个插件的 `parse()` 只认自己那一类：先看围栏语言是不是 `mermaid`，再看首个
//! 有效行的关键字（见 [`diagram_kind`]），不是自己的类型就返回 `None`，把节点
//! 让给链上后面的插件。因此挂几个插件就只多跑几个 parser，**没挂的图类型等于
//! 不存在**；没启用的 feature 不会被编译，围栏源码原样回退成代码块，**不会吞
//! 内容**。
//!
//! | 插件 | 关键字 | 模块 | feature |
//! |---|---|---|---|
//! | `FlowchartPlugin` | `flowchart` / `graph` | [`flowchart`] + [`node_link`] | `mermaid-flowchart` |
//! | `SequencePlugin` | `sequenceDiagram` | [`sequence`] | `mermaid-sequence` |
//! | `ClassPlugin` | `classDiagram` | [`class`] | `mermaid-class` |
//! | `StatePlugin` | `stateDiagram-v2` | [`state`] + [`node_link`] | `mermaid-state` |
//! | `GanttPlugin` | `gantt` | [`gantt`] | `mermaid-gantt` |
//!
//! 所有图都用 GPUI 原生绘制：`canvas` 画线、箭头和几何符号，绝对定位的 `div`
//! 画文字。颜色统一由 [`palette`] 从当前主题推导（状态图那套「主题蓝强调色」
//! 是全图共用的一套色阶），因此跟随明暗主题，不依赖浏览器或 JS 运行时。

use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::{App, IntoElement, Window, div};

mod common;

// 图表配色的唯一来源：所有 Mermaid 图共用一套色阶（见 `palette.rs` 顶部说明）。
// 「后续新增的图也必须从这里取色」由 `palette.rs` 的扫描测试守住。
#[cfg(any(
    feature = "mermaid-flowchart",
    feature = "mermaid-state",
    feature = "mermaid-sequence",
    feature = "mermaid-class",
    feature = "mermaid-gantt"
))]
mod palette;

// `layout` / `node_link` 是「节点-连线」两类图共用的排版与渲染层，
// 只有 flowchart 或 stateDiagram 被选中时才需要编译。
#[cfg(any(feature = "mermaid-flowchart", feature = "mermaid-state"))]
mod layout;
#[cfg(any(feature = "mermaid-flowchart", feature = "mermaid-state"))]
mod node_link;

#[cfg(feature = "mermaid-class")]
mod class;
#[cfg(feature = "mermaid-flowchart")]
mod flowchart;
#[cfg(feature = "mermaid-gantt")]
mod gantt;
#[cfg(feature = "mermaid-sequence")]
mod sequence;
#[cfg(feature = "mermaid-state")]
mod state;

/// 认出围栏里的图类型。`%%` 注释与 `%%{init}%%` 指令不影响判定。
fn diagram_kind(source: &str) -> Option<&'static str> {
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let kind = if lower.starts_with("flowchart") || lower.starts_with("graph ") {
            "flowchart"
        } else if lower.starts_with("sequencediagram") {
            "sequence"
        } else if lower.starts_with("classdiagram") {
            "class"
        } else if lower.starts_with("statediagram") {
            "state"
        } else if lower.starts_with("gantt") {
            "gantt"
        } else {
            // 首行不认识就别猜了：交给默认代码块，避免把 erDiagram 之类误画。
            return None;
        };
        return Some(kind);
    }
    None
}

/// 取出属于 `kind` 的 Mermaid 围栏源码。
///
/// 五个插件共用的唯一判定入口：语言标记必须是 `mermaid`（不区分大小写），
/// 图类型再由 [`diagram_kind`] 按首个有效行判定。不是自己的类型就返回 `None`，
/// 节点留给链上后面的插件——这也是「挂几个插件才多跑几个 parser」的原因。
fn fence_of_kind<'a>(node: &'a markdown_ast::Node, kind: &str) -> Option<&'a str> {
    let markdown_ast::Node::Code(code) = node else {
        return None;
    };
    let lang = code.lang.as_deref().unwrap_or_default();
    if !lang.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    (diagram_kind(&code.value) == Some(kind)).then_some(code.value.as_str())
}

// 两个「节点-连线」图的渲染入口：同一条绘制路径，只是观感参数不同。
// 返回类型写全路径，避免只开 sequence/class/gantt 时 `AnyElement` 成为未使用导入。
#[cfg(feature = "mermaid-flowchart")]
fn render_flowchart(graph: &layout::Graph, cx: &App) -> gpui_kit::AnyElement {
    node_link::render_node_link(graph, node_link::NodeLinkStyle::Flowchart, cx)
}

#[cfg(feature = "mermaid-state")]
fn render_state(graph: &layout::Graph, cx: &App) -> gpui_kit::AnyElement {
    node_link::render_node_link(graph, node_link::NodeLinkStyle::State, cx)
}

/// 定义一个「只认某一类 Mermaid 围栏」的块级插件。
///
/// 五个插件除了类型名、`MarkdownNode` 名、产物类型和渲染函数之外完全同构，
/// 用宏生成可以保证它们的 `parse()` 行为一字不差；将来新增图类型照抄一行调用
/// 即可，不需要再维护一个「按变体分发」的中心枚举。
///
/// 参数依次是：插件类型名、feature、node 名、`diagram_kind` 关键字、解析产物
/// 类型、解析函数、渲染函数（签名 `fn(&产物, &App) -> AnyElement`）。
macro_rules! diagram_plugin {
    (
        $(#[$meta:meta])*
        $plugin:ident,
        $feature:literal,
        $name:literal,
        $kind:literal,
        $data:ty,
        $parse:path,
        $render:path
    ) => {
        $(#[$meta])*
        #[cfg(feature = $feature)]
        pub struct $plugin;

        #[cfg(feature = $feature)]
        impl $plugin {
            /// 插件没有可配置项，构造是零成本的。保留 `new()` 是为了调用点写法
            /// 统一（`FlowchartPlugin::new()`），也给将来加配置留位置。
            pub fn new() -> Self {
                Self
            }
        }

        #[cfg(feature = $feature)]
        impl Default for $plugin {
            fn default() -> Self {
                Self::new()
            }
        }

        #[cfg(feature = $feature)]
        impl MarkdownPlugin for $plugin {
            /// 围栏是块级节点，必须声明；漏了会被当成行内插件去解析。
            fn is_block(&self) -> bool {
                true
            }

            fn name(&self) -> &str {
                $name
            }

            fn parse(
                &self,
                node: &markdown_ast::Node,
                cx: &MarkdownParseContext<'_>,
            ) -> Option<MarkdownNode> {
                let source = fence_of_kind(node, $kind)?;
                let data = $parse(source)?;
                // `text` / `markdown` 都存围栏原文：选中复制、无障碍朗读以及
                // 节点被别的视图复用时的回退显示都靠它。
                Some(
                    MarkdownNode::new($name, data)
                        .text(source.to_string())
                        .markdown(cx.node_source(node).unwrap_or_default().to_string()),
                )
            }

            fn render(
                &self,
                node: &MarkdownNode,
                _window: &mut Window,
                cx: &mut App,
            ) -> impl IntoElement {
                match node.data::<$data>() {
                    Some(data) => $render(data, cx),
                    // 产物类型对不上（例如别的插件用了同名节点）时画空，不 panic。
                    None => div().into_any_element(),
                }
            }
        }
    };
}

diagram_plugin!(
    /// `flowchart` / `graph` 围栏（`mermaid-flowchart`）。
    FlowchartPlugin,
    "mermaid-flowchart",
    "mermaid-flowchart",
    "flowchart",
    layout::Graph,
    flowchart::parse_graph,
    render_flowchart
);

diagram_plugin!(
    /// `sequenceDiagram` 围栏（`mermaid-sequence`）。
    SequencePlugin,
    "mermaid-sequence",
    "mermaid-sequence",
    "sequence",
    sequence::Sequence,
    sequence::parse,
    sequence::render
);

diagram_plugin!(
    /// `classDiagram` 围栏（`mermaid-class`）。
    ClassPlugin,
    "mermaid-class",
    "mermaid-class",
    "class",
    class::ClassDiagram,
    class::parse,
    class::render
);

diagram_plugin!(
    /// `stateDiagram-v2` 围栏（`mermaid-state`）。
    StatePlugin,
    "mermaid-state",
    "mermaid-state",
    "state",
    layout::Graph,
    state::parse,
    render_state
);

diagram_plugin!(
    /// `gantt` 围栏（`mermaid-gantt`）。
    GanttPlugin,
    "mermaid-gantt",
    "mermaid-gantt",
    "gantt",
    gantt::GanttChart,
    gantt::parse,
    gantt::render
);

/// 从 Markdown 文本里抽出所有 mermaid 围栏，供各模块的布局回归测试复用。
#[cfg(test)]
pub(crate) fn mermaid_blocks(source: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(buffer) = current.as_mut() {
            if trimmed.starts_with("```") {
                blocks.push(std::mem::take(buffer));
                current = None;
            } else {
                buffer.push_str(line);
                buffer.push('\n');
            }
        } else if trimmed.starts_with("```mermaid") {
            current = Some(String::new());
        }
    }
    blocks
}

/// 只取某一种图的围栏：flowchart 的布局回归不能拿时序图源码去解析。
#[cfg(test)]
#[allow(dead_code)] // 只被部分图类型的模块测试用到
pub(crate) fn mermaid_blocks_of(source: &str, kind: &str) -> Vec<String> {
    mermaid_blocks(source)
        .into_iter()
        .filter(|block| diagram_kind(block).is_some_and(|k| k == kind))
        .collect()
}

/// 展示文档的唯一来源：crate 根下的 `showcase/`。
///
/// `gpui-kit-box` 的七个示例页通过跨 crate `include_str!` 读同一批文件，
/// 本 crate 的所有布局回归也只读这一份，不再维护任何副本。
#[cfg(test)]
const SHOWCASE_PAGES: &[&str] = &[
    include_str!("../../showcase/markdown.md"),
    include_str!("../../showcase/math.md"),
    include_str!("../../showcase/flowchart.md"),
    include_str!("../../showcase/sequence.md"),
    include_str!("../../showcase/class.md"),
    include_str!("../../showcase/state.md"),
    include_str!("../../showcase/gantt.md"),
];

/// 取全部展示页里的某一类图，供各图类型的模块测试遍历。
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn showcase_blocks(kind: &str) -> Vec<String> {
    SHOWCASE_PAGES
        .iter()
        .flat_map(|page| mermaid_blocks_of(page, kind))
        .collect()
}

#[cfg(test)]
fn all_showcase_blocks() -> Vec<String> {
    SHOWCASE_PAGES
        .iter()
        .flat_map(|page| mermaid_blocks(page))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_covers_all_five_diagram_kinds() {
        let blocks = all_showcase_blocks();
        assert_eq!(
            blocks.len(),
            20,
            "展示页的图数量变了，需要重新确认测试覆盖范围"
        );
        let mut kinds: Vec<&str> = blocks.iter().filter_map(|b| diagram_kind(b)).collect();
        kinds.sort();
        kinds.dedup();
        assert_eq!(
            kinds,
            vec!["class", "flowchart", "gantt", "sequence", "state"]
        );
    }

    /// 测试用：按 `diagram_kind` 直接走各图的解析入口，等价于「对应插件挂上后
    /// 能否认出这张图」。不构造 mdast 节点，避免测试跟着插件注册细节走。
    fn dispatch_parses(source: &str) -> bool {
        let Some(kind) = diagram_kind(source) else {
            return false;
        };
        match kind {
            #[cfg(feature = "mermaid-flowchart")]
            "flowchart" => flowchart::parse_graph(source).is_some(),
            #[cfg(feature = "mermaid-sequence")]
            "sequence" => sequence::parse(source).is_some(),
            #[cfg(feature = "mermaid-class")]
            "class" => class::parse(source).is_some(),
            #[cfg(feature = "mermaid-state")]
            "state" => state::parse(source).is_some(),
            #[cfg(feature = "mermaid-gantt")]
            "gantt" => gantt::parse(source).is_some(),
            // 类型合法但对应 feature 没开：插件根本不存在，围栏回退成代码块。
            _ => false,
        }
    }

    /// feature 化的核心保证：启用的类型必须能画，未启用的必须原样回退。
    #[test]
    fn enabled_kinds_render_disabled_kinds_fall_back() {
        for (ix, block) in all_showcase_blocks().iter().enumerate() {
            let kind = diagram_kind(block).unwrap_or_else(|| panic!("第 {ix} 张图没有类型头"));
            let enabled = match kind {
                "flowchart" => cfg!(feature = "mermaid-flowchart"),
                "sequence" => cfg!(feature = "mermaid-sequence"),
                "class" => cfg!(feature = "mermaid-class"),
                "state" => cfg!(feature = "mermaid-state"),
                "gantt" => cfg!(feature = "mermaid-gantt"),
                _ => false,
            };
            assert_eq!(
                dispatch_parses(block),
                enabled,
                "第 {ix} 张图（{kind}）的 feature 开关与分发结果不一致"
            );
        }
    }

    #[test]
    fn unknown_kinds_are_not_swallowed() {
        let other = "erDiagram\n  CUSTOMER ||--o{ ORDER : places";
        assert!(diagram_kind(other).is_none());
        assert!(!dispatch_parses(other));
        assert!(diagram_kind("%%{init: {}}%%\nflowchart TD\nA-->B") == Some("flowchart"));
    }

    /// 插件名是 `MarkdownExtensions` 里 renderer 表的键：同名会互相覆盖，
    /// 等于只挂上了后一个。这里守住「一个图类型一个名字」。
    #[test]
    fn plugin_names_are_unique_and_tied_to_their_kind() {
        // `name()` 借用插件本身，拷成 `String` 再往表里放。
        let mut plugins: Vec<(String, &str)> = Vec::new();
        #[cfg(feature = "mermaid-flowchart")]
        plugins.push((FlowchartPlugin::new().name().to_string(), "flowchart"));
        #[cfg(feature = "mermaid-sequence")]
        plugins.push((SequencePlugin::new().name().to_string(), "sequence"));
        #[cfg(feature = "mermaid-class")]
        plugins.push((ClassPlugin::new().name().to_string(), "class"));
        #[cfg(feature = "mermaid-state")]
        plugins.push((StatePlugin::new().name().to_string(), "state"));
        #[cfg(feature = "mermaid-gantt")]
        plugins.push((GanttPlugin::new().name().to_string(), "gantt"));

        let mut names: Vec<&str> = plugins.iter().map(|(name, _)| name.as_str()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "插件名重复，后挂的会覆盖先挂的");

        // 名字里带图类型关键字，出问题时能直接从节点名看出是谁画的。
        for (name, kind) in &plugins {
            assert!(name.contains(kind), "插件名 {name} 与图类型 {kind} 对不上");
        }
    }
}
