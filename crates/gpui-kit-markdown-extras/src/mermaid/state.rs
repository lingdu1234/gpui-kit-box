//! `stateDiagram-v2` 状态图：语法解析。
//!
//! 状态图与流程图同属「节点 + 连线」，因此解析结果就是 [`layout::Graph`]，
//! 直接复用 `layout.rs` 的分层布线与 `node_link.rs` 的绘制，不另造一套布局器。
//! `[*]` 画成起止圆，`<<choice>>` 画菱形，`<<fork>>`/`<<join>>` 画同步条；
//! 复合状态的外框按成员包围盒在渲染层描边，不参与分层。
//!
//! ```text
//! stateDiagram-v2
//!     [*] --> Idle
//!     Idle --> Running : start
//!     state Running {
//!         [*] --> Booting
//!         Booting --> Ready
//!     }
//!     state check <<choice>>
//!     note right of Idle : 等待指令
//!     Running --> [*] : done
//! ```
//!
//! `direction` 会被解析但不改变排布方向：共用布局器是自上而下的。

use std::collections::HashMap;

use super::common::clean_label;
use super::layout::{Graph, GraphEdge, GraphNode, Overlay, Shape, mark_back_edges};

/// `[*]` 每次出现都是独立的一个圆（起点与终点不能共用一个节点）。
/// 其余 id 复用同一个节点，后写的 `<<choice>>` 等声明会覆盖形状。
fn node_ix(
    nodes: &mut Vec<GraphNode>,
    index: &mut HashMap<String, usize>,
    id: &str,
    label: &str,
    shape: Shape,
) -> usize {
    if id == "[*]" {
        let ix = nodes.len();
        nodes.push(GraphNode {
            label: String::new(),
            shape: Shape::Terminal,
        });
        return ix;
    }
    if let Some(&ix) = index.get(id) {
        if shape != Shape::Round {
            nodes[ix].shape = shape;
        }
        return ix;
    }
    let ix = nodes.len();
    nodes.push(GraphNode {
        label: clean_label(if label.is_empty() { id } else { label }),
        shape,
    });
    index.insert(id.to_string(), ix);
    ix
}

/// `A --> B : 事件` → `("A", "B", Some("事件"))`。
fn parse_transition(line: &str) -> Option<(String, String, Option<String>)> {
    let at = line.find("-->")?;
    let left = line[..at].trim();
    let right = &line[at + 3..];
    let (right, label) = match right.split_once(':') {
        Some((head, text)) => (head.trim(), Some(clean_label(text))),
        None => (right.trim(), None),
    };
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((
        left.to_string(),
        right.to_string(),
        label.filter(|s| !s.is_empty()),
    ))
}

fn add_edge(
    nodes: &mut Vec<GraphNode>,
    index: &mut HashMap<String, usize>,
    edges: &mut Vec<GraphEdge>,
    a: &str,
    b: &str,
    label: Option<String>,
) -> (usize, usize) {
    let from = node_ix(nodes, index, a, "", Shape::Round);
    let to = node_ix(nodes, index, b, "", Shape::Round);
    edges.push(GraphEdge {
        from,
        to,
        label,
        back: false,
    });
    (from, to)
}

/// 多行 note 一直读到 `end note`；单行 note 取冒号后的文字。
fn note_text(rest: &str, lines: &[&str], i: &mut usize) -> String {
    if let Some((_, text)) = rest.split_once(':') {
        return clean_label(text);
    }
    let mut buf = Vec::new();
    while *i < lines.len() {
        let next = lines[*i].trim();
        *i += 1;
        if next.to_ascii_lowercase().starts_with("end note") {
            break;
        }
        if !next.is_empty() {
            buf.push(clean_label(next));
        }
    }
    buf.join(" ")
}

pub(super) fn parse(source: &str) -> Option<Graph> {
    let lines: Vec<&str> = source.lines().collect();
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut overlays: Vec<Overlay> = Vec::new();
    let mut notes: Vec<(usize, String)> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("statediagram")
            || lower.starts_with("direction ")
            || lower.starts_with("classdef ")
            || lower.starts_with("style ")
            || lower.starts_with("click ")
            || lower == "end"
        {
            continue;
        }

        // 复合状态：内部转移的前缀隔离成 `X::子状态`，外框包住整组节点。
        if lower.starts_with("state ") && line.ends_with('{') {
            let name = line[6..line.len() - 1].trim();
            let parent = node_ix(&mut nodes, &mut index, name, name, Shape::Rect);
            let mut members = vec![parent];
            while i < lines.len() {
                let inner = lines[i].trim();
                i += 1;
                if inner.starts_with('}') {
                    break;
                }
                if inner.is_empty() || inner.starts_with("%%") {
                    continue;
                }
                let Some((a, b, label)) = parse_transition(inner) else {
                    continue;
                };
                let ka = if a == "[*]" {
                    a.clone()
                } else {
                    format!("{name}::{a}")
                };
                let kb = if b == "[*]" {
                    b.clone()
                } else {
                    format!("{name}::{b}")
                };
                let (from, to) = add_edge(&mut nodes, &mut index, &mut edges, &ka, &kb, label);
                if !members.contains(&from) {
                    members.push(from);
                }
                if !members.contains(&to) {
                    members.push(to);
                }
            }
            overlays.push(Overlay {
                label: clean_label(name),
                members,
            });
            continue;
        }

        if lower.starts_with("state ") {
            let rest = &line[6..];
            if let Some((display, id)) = rest.split_once(" as ") {
                let ix = node_ix(&mut nodes, &mut index, id.trim(), "", Shape::Round);
                nodes[ix].label = clean_label(display);
                continue;
            }
            if lower.contains("<<") {
                let id = rest.split_whitespace().next().unwrap_or("");
                let shape = if lower.contains("<<choice>>") {
                    Shape::Diamond
                } else if lower.contains("<<fork>>") || lower.contains("<<join>>") {
                    Shape::Bar
                } else {
                    Shape::Round
                };
                let ix = node_ix(&mut nodes, &mut index, id, "", shape);
                nodes[ix].shape = shape;
                continue;
            }
            node_ix(&mut nodes, &mut index, rest.trim(), "", Shape::Round);
            continue;
        }

        if lower.starts_with("note left of ") || lower.starts_with("note right of ") {
            let rest = &line[5..];
            let head = rest.split(':').next().unwrap_or(rest);
            let id = head.split_whitespace().last().unwrap_or("");
            let text = note_text(rest, &lines, &mut i);
            if !text.is_empty() {
                let ix = node_ix(&mut nodes, &mut index, id, "", Shape::Round);
                notes.push((ix, text));
            }
            continue;
        }

        if let Some((a, b, label)) = parse_transition(line) {
            add_edge(&mut nodes, &mut index, &mut edges, &a, &b, label);
        }
    }

    if nodes.is_empty() || edges.is_empty() {
        return None;
    }
    mark_back_edges(&mut edges, nodes.len());
    Some(Graph {
        nodes,
        edges,
        overlays,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"stateDiagram-v2
    direction TB
    [*] --> Idle
    Idle --> Running : start
    state Running {
        [*] --> Booting
        Booting --> Ready
    }
    state check <<choice>>
    Running --> check
    state fork1 <<fork>>
    note right of Idle : 等待指令
    Running --> [*] : done
"#;

    #[test]
    fn parses_states_transitions_and_symbols() {
        let g = parse(SAMPLE).expect("状态图应可解析");
        assert!(g.nodes.iter().any(|n| n.shape == Shape::Terminal));
        assert!(g.nodes.iter().any(|n| n.shape == Shape::Diamond));
        assert!(g.nodes.iter().any(|n| n.shape == Shape::Bar));
        assert_eq!(g.overlays.len(), 1);
        assert_eq!(g.overlays[0].label, "Running");
        assert_eq!(g.notes.len(), 1);
        assert_eq!(g.notes[0].1, "等待指令");
        assert!(g.edges.iter().any(|e| e.label.as_deref() == Some("start")));
        assert!(
            g.edges
                .iter()
                .all(|e| e.from < g.nodes.len() && e.to < g.nodes.len())
        );
    }

    #[test]
    fn start_and_end_are_distinct_terminals() {
        let g = parse("[*] --> A\nA --> [*]\n").unwrap();
        let terminals: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.shape == Shape::Terminal)
            .collect();
        assert_eq!(terminals.len(), 2);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[0].from, 0);
        assert_eq!(g.edges[1].to, 2);
    }

    #[test]
    fn shared_layout_stays_inside_canvas() {
        let g = parse(SAMPLE).unwrap();
        let l = super::super::layout::layout(&g);
        assert!(l.width > 0. && l.height > 0.);
        for p in &l.nodes {
            assert!(p.x >= 0. && p.x + p.w <= l.width + 0.5);
            assert!(p.y >= 0. && p.y + super::super::layout::NODE_H <= l.height + 0.5);
        }
    }
}
