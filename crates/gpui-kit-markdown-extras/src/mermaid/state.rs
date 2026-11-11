//! `stateDiagram-v2` 状态图：语法解析。
//!
//! 状态图与流程图同属「节点 + 连线」，因此解析结果就是 [`layout::Graph`]，
//! 直接复用 `layout.rs` 的分层布线与 `node_link.rs` 的绘制，不另造一套布局器。
//!
//! 符号约定（对齐 Mermaid 官方状态图的观感）：
//!
//! - `[*]` 画出起点实心圆 / 终点圆环，靠出入度区分，不靠书写顺序；
//! - `<<choice>>` 是空心菱形；
//! - `<<fork>>` / `<<join>>`：名字是 `fork1`、`join1` 这类内部标识符时画成
//!   同步条（Mermaid 的默认行为），名字是有意义的词（`Fork` / `Join`）时
//!   画成普通状态节点——不然名字只能压在条子底下，既看不清也没意义；
//! - 复合状态以自身节点为锚点参与分层，内部成员经虚拟边钉到锚点下方，
//!   外框按成员包围盒描边，顶部留出标题栏，成员名省掉 `Running::` 前缀。
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
use super::layout::{
    Direction, EdgeEnd, EdgeStyle, Graph, GraphEdge, GraphNode, Overlay, Shape, mark_back_edges,
};

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
        style: EdgeStyle::Solid,
        left: EdgeEnd::Bare,
        right: EdgeEnd::Arrow,
        from_sg: None,
        to_sg: None,
        invisible: false,
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
    let mut pins: Vec<(usize, usize)> = Vec::new();
    let mut notes: Vec<(usize, String)> = Vec::new();
    let mut direction = Direction::Td;
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        // `direction LR` 等：改排布方向（共用布局器按方向做几何变换）。
        if lower.starts_with("direction ") {
            if let Some(tok) = line[9..].split_whitespace().next() {
                direction = Direction::parse(tok);
            }
            continue;
        }
        if lower.starts_with("statediagram")
            || lower.starts_with("classdef ")
            || lower.starts_with("style ")
            || lower.starts_with("click ")
            || lower == "end"
        {
            continue;
        }

        // 复合状态：内部转移的前缀隔离成 `X::子状态`，外框包住整组节点。
        // 锚点（复合状态自身节点）留在主链里接对外转移；没有内部入边的
        // 成员（如内部的 `[*]`）用虚拟边钉到锚点下一层，内部链因此排在
        // 锚点正下方，外框是它们整体的包围盒。
        if lower.starts_with("state ") && line.ends_with('{') {
            let name = line[6..line.len() - 1].trim();
            let parent = node_ix(&mut nodes, &mut index, name, name, Shape::Rect);
            let mut members = vec![parent];
            let mut inner_pairs: Vec<(usize, usize)> = Vec::new();
            while i < lines.len() {
                let inner = lines[i].trim();
                i += 1;
                if inner.starts_with('}') {
                    break;
                }
                if inner.is_empty() || inner.starts_with("%%") {
                    continue;
                }
                // 并发区域分隔符 `--`：独立成行时跳过；行内 ` -- ` 把多条链拆开。
                // 各链都解析进同一个复合状态（近似并排），不丢节点、不 panic。
                if inner == "--" {
                    continue;
                }
                for part in inner.split(" -- ") {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let Some((a, b, label)) = parse_transition(part) else {
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
                    inner_pairs.push((from, to));
                }
            }
            for &m in members.iter().skip(1) {
                if !inner_pairs.iter().any(|&(_, t)| t == m) {
                    pins.push((parent, m));
                }
            }
            overlays.push(Overlay {
                label: clean_label(name),
                anchor: Some(parent),
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
            // `state X: 描述文字`：冒号左边是状态 id，右边作为它的显示文字。
            if let Some((id, desc)) = rest.split_once(':') {
                let ix = node_ix(&mut nodes, &mut index, id.trim(), "", Shape::Round);
                let desc = clean_label(desc);
                if !desc.is_empty() {
                    nodes[ix].label = desc;
                }
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
        pin_edges: pins,
        notes,
        direction,
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

    /// 复合状态是「超节点」：内部链钉在锚点正下方，锚点的对外出边落到整个
    /// 外框之下，外框只包住成员——框外节点（Created）在框上方、出边目标
    /// （paid_check）在框下方，外框顶部也不能越出画布。
    #[test]
    fn composite_wraps_only_its_members() {
        let src = super::super::showcase_blocks("state")
            .into_iter()
            .find(|b| b.contains("state Paying {"))
            .expect("展示页应有 Paying 复合状态");
        let g = parse(&src).unwrap();
        let l = super::super::layout::layout(&g);
        let anchor = g.nodes.iter().position(|n| n.label == "Paying").unwrap();
        let created = g.nodes.iter().position(|n| n.label == "Created").unwrap();
        let paid_check = g
            .nodes
            .iter()
            .position(|n| n.label == "paid_check")
            .unwrap();
        let ov = g
            .overlays
            .iter()
            .find(|ov| ov.anchor == Some(anchor))
            .unwrap();
        let inner: Vec<usize> = ov.members.iter().copied().filter(|&m| m != anchor).collect();
        assert!(!inner.is_empty(), "复合状态应有内部成员");
        for &m in &inner {
            assert!(
                l.ranks[m] > l.ranks[anchor],
                "内部成员应钉在锚点下方"
            );
        }
        for e in &g.edges {
            if e.from == anchor && !inner.contains(&e.to) {
                for &m in &inner {
                    assert!(
                        l.ranks[e.to] > l.ranks[m],
                        "锚点的对外出边应落到所有内部成员之下"
                    );
                }
            }
        }
        let Some(&(x1, y1, x2, y2, _)) = l.overlays.first() else {
            panic!("布局应输出外框矩形");
        };
        assert!(y1 >= 0., "外框顶部不能越出画布");
        let node_h = super::super::layout::NODE_H;
        let cr = l.nodes[created];
        assert!(cr.y + node_h <= y1, "Created 被外框吞掉了");
        let pr = l.nodes[paid_check];
        assert!(pr.y >= y2, "paid_check 应在外框下方");
        for &m in &inner {
            let p = l.nodes[m];
            assert!(
                p.x >= x1 && p.x + p.w <= x2 && p.y >= y1 && p.y + node_h <= y2,
                "内部成员应全部落在外框内"
            );
        }
    }

    /// 复合状态标题栏下面不能空一大片。
    ///
    /// 锚点自己是不画框的，内部第一层若仍按整行间距（节点高 + 层间距 = 150px）
    /// 往下排，标题栏到第一个内部节点之间就会空出 130px 上下，整张图被白白拉长。
    /// 这里锁住「标题栏 → 第一层」的净空，顺带确认外框仍然包得住成员。
    #[test]
    fn composite_first_layer_hugs_the_header() {
        use super::super::layout::{NODE_H, OVERLAY_HEADER};
        let mut checked = 0;
        for src in super::super::showcase_blocks("state") {
            let g = parse(&src).unwrap();
            let l = super::super::layout::layout(&g);
            for (oi, ov) in g.overlays.iter().enumerate() {
                let Some(anchor_ix) = ov.anchor else { continue; };
                let (x1, y1, _, y2, _) = l.overlays[oi];
                let anchor_y = l.nodes[anchor_ix].y;
                let first = ov
                    .members
                    .iter()
                    .filter(|&&m| m != anchor_ix)
                    .map(|&m| l.nodes[m])
                    .min_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
                    .expect("复合状态应有内部成员");
                let gap = first.y - (y1 + OVERLAY_HEADER);
                assert!(
                    (0. ..=40.).contains(&gap),
                    "「{}」标题栏到第一层的净空是 {gap:.0}px，太浪费了",
                    ov.label
                );
                assert!(
                    first.y < anchor_y + NODE_H + 8.,
                    "「{}」第一层应紧贴锚点行，而不是被整行间距推下去",
                    ov.label
                );
                assert!(first.y >= y1 && first.y + NODE_H <= y2, "内部成员被挤出外框");
                assert!(x1 < first.x, "外框左边界异常");
                checked += 1;
            }
        }
        assert!(checked > 0, "展示页应至少有一张带复合状态的状态图");
    }

    #[test]
    fn colon_description_becomes_display_label() {
        let g = parse("stateDiagram-v2\nstate X: 描述文字\nX --> Y\n").unwrap();
        assert!(
            g.nodes.iter().any(|n| n.label == "描述文字"),
            "冒号后的描述应成为显示文字"
        );
        assert!(
            !g.nodes.iter().any(|n| n.label.contains("X:")),
            "不应把整串 `X: 描述` 当成标签"
        );
        // 已存在的状态再写描述：覆盖显示文字，不新建节点。
        let g2 = parse("stateDiagram-v2\nA --> B\nstate B: 完成\n").unwrap();
        assert_eq!(g2.nodes.iter().filter(|n| n.label == "完成").count(), 1);
        // `state "desc" as X` 仍照旧。
        let g3 = parse("stateDiagram-v2\nstate \"别名\" as Z\nZ --> [*]\n").unwrap();
        assert!(g3.nodes.iter().any(|n| n.label == "别名"));
    }

    #[test]
    fn concurrent_regions_keep_both_chains() {
        let node_h = super::super::layout::NODE_H;
        // 独立成行的 `--` 分隔符。
        let src = "stateDiagram-v2\nstate X {\n A --> B\n --\n C --> D\n}\n";
        let g = parse(src).unwrap();
        for label in ["X::A", "X::B", "X::C", "X::D"] {
            assert!(g.nodes.iter().any(|n| n.label == label), "缺少 {label}");
        }
        let l = super::super::layout::layout(&g);
        for p in &l.nodes {
            assert!(p.x >= 0. && p.x + p.w <= l.width + 0.5);
            assert!(p.y >= 0. && p.y + node_h <= l.height + 0.5);
        }
        // 行内 ` -- ` 分隔同样要解析出两条链。
        let src2 = "stateDiagram-v2\nstate X {\n A --> B -- C --> D\n}\n";
        let g2 = parse(src2).unwrap();
        for label in ["X::A", "X::B", "X::C", "X::D"] {
            assert!(g2.nodes.iter().any(|n| n.label == label), "行内区域缺少 {label}");
        }
    }

    #[test]
    fn parses_direction() {
        for (tok, want) in [
            ("LR", Direction::Lr),
            ("RL", Direction::Rl),
            ("BT", Direction::Bt),
            ("TB", Direction::Td),
        ] {
            let src = format!("stateDiagram-v2\ndirection {tok}\n[*] --> A\nA --> [*]\n");
            let g = parse(&src).unwrap();
            assert_eq!(g.direction, want, "direction {tok}");
            // 方向变换后布局不炸、节点都在画布内。
            let l = super::super::layout::layout(&g);
            let node_h = super::super::layout::NODE_H;
            for p in &l.nodes {
                assert!(p.x >= -0.5 && p.x + p.w <= l.width + 0.5);
                assert!(p.y >= -0.5 && p.y + node_h <= l.height + 0.5);
            }
        }
    }

    /// 冒号描述 + direction 组合：描述成为显示文字，方向变换后布局不炸。
    #[test]
    fn colon_description_combines_with_direction() {
        let src = "stateDiagram-v2\ndirection LR\nstate Idle: 空闲\nstate Run: 运行\nIdle --> Run : start\nRun --> [*]\n";
        let g = parse(src).unwrap();
        assert_eq!(g.direction, Direction::Lr);
        assert!(g.nodes.iter().any(|n| n.label == "空闲"));
        assert!(g.nodes.iter().any(|n| n.label == "运行"));
        let l = super::super::layout::layout(&g);
        let node_h = super::super::layout::NODE_H;
        for p in &l.nodes {
            assert!(p.x >= -0.5 && p.x + p.w <= l.width + 0.5);
            assert!(p.y >= -0.5 && p.y + node_h <= l.height + 0.5);
        }
    }

    /// 并发区域的两条链都钉在复合状态锚点下方，外框包住全部四个成员。
    #[test]
    fn concurrent_regions_pin_under_anchor_and_wrap() {
        let src = "stateDiagram-v2\n[*] --> X\nstate X {\n A --> B\n --\n C --> D\n}\nX --> [*]\n";
        let g = parse(src).unwrap();
        let anchor = g.nodes.iter().position(|n| n.label == "X").unwrap();
        let ov = g.overlays.iter().find(|o| o.anchor == Some(anchor)).unwrap();
        let inner: Vec<usize> = ov.members.iter().copied().filter(|&m| m != anchor).collect();
        assert_eq!(inner.len(), 4, "两条并发链共四个内部成员");
        let l = super::super::layout::layout(&g);
        for &m in &inner {
            assert!(l.ranks[m] > l.ranks[anchor], "内部成员应钉在锚点下方");
        }
        let node_h = super::super::layout::NODE_H;
        let (x1, y1, x2, y2, _) = l.overlays[0];
        for &m in &inner {
            let p = l.nodes[m];
            assert!(
                p.x >= x1 && p.x + p.w <= x2 && p.y >= y1 && p.y + node_h <= y2,
                "并发成员被挤出外框"
            );
        }
    }

    /// 行内 ` -- ` 分隔的并发区域同样排进外框，不越界。
    #[test]
    fn concurrent_inline_regions_wrap_in_frame() {
        let src = "stateDiagram-v2\n[*] --> X\nstate X {\n A --> B -- C --> D\n}\nX --> [*]\n";
        let g = parse(src).unwrap();
        let anchor = g.nodes.iter().position(|n| n.label == "X").unwrap();
        let ov = g.overlays.iter().find(|o| o.anchor == Some(anchor)).unwrap();
        let inner: Vec<usize> = ov.members.iter().copied().filter(|&m| m != anchor).collect();
        assert_eq!(inner.len(), 4);
        let l = super::super::layout::layout(&g);
        let node_h = super::super::layout::NODE_H;
        let (x1, y1, x2, y2, _) = l.overlays[0];
        for &m in &inner {
            let p = l.nodes[m];
            assert!(p.x >= x1 && p.x + p.w <= x2 && p.y >= y1 && p.y + node_h <= y2);
        }
        for p in &l.nodes {
            assert!(p.x >= -0.5 && p.x + p.w <= l.width + 0.5);
            assert!(p.y >= -0.5 && p.y + node_h <= l.height + 0.5);
        }
    }
}
