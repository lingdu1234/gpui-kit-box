//! Markdown 插件：Mermaid 图表渲染。
//!
//! 一个 [`MermaidPlugin`] 统一识别 ` ```mermaid ` 围栏，按首个有效行的关键字
//! 把源码分发给对应图类型的解析/渲染模块。每种图是一个独立文件，也是一个独立
//! Cargo feature：没启用的类型不会被编译，`parse()` 直接返回 `None`，围栏内容
//! 交给默认代码块渲染，**不会吞内容**。
//!
//! | 关键字 | 模块 | feature |
//! |---|---|---|
//! | `flowchart` / `graph` | [`flowchart`] + [`node_link`] | `mermaid-flowchart` |
//! | `sequenceDiagram` | [`sequence`] | `mermaid-sequence` |
//! | `classDiagram` | [`class`] | `mermaid-class` |
//! | `stateDiagram-v2` | [`state`] + [`node_link`] | `mermaid-state` |
//! | `gantt` | [`gantt`] | `mermaid-gantt` |
//!
//! 所有图都用 GPUI 原生绘制：`canvas` 画线、箭头和几何符号，绝对定位的 `div`
//! 画文字，颜色取自 `cx.theme()`，因此跟随明暗主题，不依赖浏览器或 JS 运行时。

use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::{App, IntoElement, Window, div};

mod common;

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

/// 解析结果存进 `MarkdownNode::data`，渲染时按变体分发。
///
/// 每个变体都带 `#[cfg]`：未启用的图类型在这里完全不存在，编译产物里也不会
/// 留下对应的解析/布局/绘制代码。
#[derive(Clone)]
enum Diagram {
    #[cfg(feature = "mermaid-flowchart")]
    Flowchart(layout::Graph),
    #[cfg(feature = "mermaid-sequence")]
    Sequence(sequence::Sequence),
    #[cfg(feature = "mermaid-class")]
    Class(class::ClassDiagram),
    #[cfg(feature = "mermaid-state")]
    State(layout::Graph),
    #[cfg(feature = "mermaid-gantt")]
    Gantt(gantt::GanttChart),
}

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

fn parse_diagram(source: &str) -> Option<Diagram> {
    let kind = diagram_kind(source)?;

    #[cfg(feature = "mermaid-flowchart")]
    if kind == "flowchart" {
        return flowchart::parse_graph(source).map(Diagram::Flowchart);
    }
    #[cfg(feature = "mermaid-sequence")]
    if kind == "sequence" {
        return sequence::parse(source).map(Diagram::Sequence);
    }
    #[cfg(feature = "mermaid-class")]
    if kind == "class" {
        return class::parse(source).map(Diagram::Class);
    }
    #[cfg(feature = "mermaid-state")]
    if kind == "state" {
        return state::parse(source).map(Diagram::State);
    }
    #[cfg(feature = "mermaid-gantt")]
    if kind == "gantt" {
        return gantt::parse(source).map(Diagram::Gantt);
    }

    // 类型合法但对应 feature 没开：保留源码，让默认代码块显示。
    None
}

/// 挂到 `TextView` 上的 Mermaid 插件；只在至少一个 `mermaid-*` feature 启用时存在。
pub struct MermaidPlugin;

impl MarkdownPlugin for MermaidPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "mermaid"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        let lang = code.lang.as_deref().unwrap_or_default();
        if !lang.eq_ignore_ascii_case("mermaid") {
            return None;
        }
        let diagram = parse_diagram(&code.value)?;
        Some(
            MarkdownNode::new("mermaid", diagram)
                .text(code.value.clone())
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(diagram) = node.data::<Diagram>() else {
            return div().into_any_element();
        };
        match diagram {
            #[cfg(feature = "mermaid-flowchart")]
            Diagram::Flowchart(graph) => node_link::render_node_link(graph, cx),
            #[cfg(feature = "mermaid-sequence")]
            Diagram::Sequence(seq) => sequence::render(seq, cx),
            #[cfg(feature = "mermaid-class")]
            Diagram::Class(class) => class::render(class, cx),
            #[cfg(feature = "mermaid-state")]
            Diagram::State(graph) => node_link::render_node_link(graph, cx),
            #[cfg(feature = "mermaid-gantt")]
            Diagram::Gantt(chart) => gantt::render(chart, cx),
        }
    }
}

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
            19,
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
                parse_diagram(block).is_some(),
                enabled,
                "第 {ix} 张图（{kind}）的 feature 开关与分发结果不一致"
            );
        }
    }

    #[test]
    fn unknown_kinds_are_not_swallowed() {
        let other = "erDiagram\n  CUSTOMER ||--o{ ORDER : places";
        assert!(diagram_kind(other).is_none());
        assert!(parse_diagram(other).is_none());
        assert!(diagram_kind("%%{init: {}}%%\nflowchart TD\nA-->B") == Some("flowchart"));
    }
}
