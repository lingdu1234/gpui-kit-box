//! `flowchart` / `graph` 语法解析。
//!
//! 支持 `ID[文本]`、`ID{文本}`、`ID(文本)` 三种形状，`-->` / `-.->` / `==>`
//! 三种箭头，以及 `-->|文字|` 和 `-- 文字 -->` 两种边标注。

use std::collections::HashMap;

use super::layout::*;

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

pub(super) fn parse_graph(source: &str) -> Option<Graph> {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("flowchart")
            || lower.starts_with("graph")
            || lower.starts_with("subgraph")
            || lower.starts_with("style")
            || lower.starts_with("class")
            || lower.starts_with("linkstyle")
            || lower.starts_with("click")
            || lower == "end"
        {
            continue;
        }
        for statement in line.split(';') {
            let statement = statement.trim();
            if !statement.is_empty() {
                parse_statement(statement, &mut nodes, &mut index, &mut edges);
            }
        }
    }

    if nodes.is_empty() || edges.is_empty() {
        return None;
    }
    mark_back_edges(&mut edges, nodes.len());
    Some(Graph {
        nodes,
        edges,
        overlays: Vec::new(),
        pin_edges: Vec::new(),
        notes: Vec::new(),
    })
}

fn parse_statement(
    statement: &str,
    nodes: &mut Vec<GraphNode>,
    index: &mut HashMap<String, usize>,
    edges: &mut Vec<GraphEdge>,
) {
    let mut chunks: Vec<String> = Vec::new();
    let mut labels: Vec<Option<String>> = Vec::new();
    let mut rest = statement;

    while let Some((at, len)) = find_arrow(rest) {
        let head = rest[..at].trim();
        let mut tail = &rest[at + len..];
        // `B -- 已激活 --> C`：标签写在箭头**前**，要从节点文本里切掉。
        let (head, mut label) = split_trailing_label(head);
        // `-->|文字|`：标签写在箭头**后**。
        tail = tail.trim_start();
        if let Some(after) = tail.strip_prefix('|')
            && let Some(end) = after.find('|')
        {
            label = Some(clean_text(after[..end].trim()));
            tail = &after[end + 1..];
        }
        chunks.push(head);
        labels.push(label);
        rest = tail;
    }
    chunks.push(rest.trim().to_string());

    let mut previous: Option<usize> = None;
    for (ix, chunk) in chunks.iter().enumerate() {
        let Some((id, label, shape)) = parse_node(chunk) else {
            previous = None;
            continue;
        };
        let current = match index.get(&id) {
            Some(&existing) => {
                // 同一个节点后面再出现时，以第一次的文字为准（手册里写法一致）。
                existing
            }
            None => {
                nodes.push(GraphNode { label, shape });
                index.insert(id, nodes.len() - 1);
                nodes.len() - 1
            }
        };
        if let Some(from) = previous {
            edges.push(GraphEdge {
                from,
                to: current,
                label: labels.get(ix - 1).cloned().flatten(),
                back: false,
            });
        }
        previous = Some(current);
    }
}

/// 找出最早出现的箭头，返回 `(起始字节, 长度)`。
fn find_arrow(text: &str) -> Option<(usize, usize)> {
    const ARROWS: [&str; 6] = ["-.->", "==>", "-->", "-.-", "===", "---"];
    let mut best: Option<(usize, usize)> = None;
    for arrow in ARROWS {
        if let Some(at) = text.find(arrow)
            && best.is_none_or(|(best_at, _)| at < best_at)
        {
            best = Some((at, arrow.len()));
        }
    }
    best
}

/// `B -- 已激活` → `("B", Some("已激活"))`：`--` 之后是边标签，不是节点文本。
fn split_trailing_label(chunk: &str) -> (String, Option<String>) {
    match chunk.rfind("--") {
        Some(at) => {
            let label = clean_text(chunk[at + 2..].trim());
            (
                chunk[..at].trim().to_string(),
                (!label.is_empty()).then_some(label),
            )
        }
        None => (chunk.trim().to_string(), None),
    }
}

fn parse_node(chunk: &str) -> Option<(String, String, Shape)> {
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return None;
    }
    let open_at = chunk.find(['[', '{', '(']);
    let (id, rest) = match open_at {
        Some(at) => (&chunk[..at], &chunk[at..]),
        None => (chunk, ""),
    };
    let id = clean_text(id.trim());
    if id.is_empty() {
        return None;
    }
    let mut label = id.clone();
    let mut shape = Shape::Rect;
    if !rest.is_empty() {
        let (open, close, kind) = if rest.starts_with("((") {
            ("((", "))", Shape::Round)
        } else if rest.starts_with('(') {
            ("(", ")", Shape::Round)
        } else if rest.starts_with('{') {
            ("{", "}", Shape::Diamond)
        } else if rest.starts_with("[[") {
            ("[[", "]]", Shape::Rect)
        } else {
            ("[", "]", Shape::Rect)
        };
        let inner = &rest[open.len()..];
        let inner = inner.strip_suffix(close).unwrap_or(inner);
        let text = clean_text(inner.trim());
        if !text.is_empty() {
            label = text;
        }
        shape = kind;
    }
    Some((id, label, shape))
}

/// 去掉引号与 `<br/>`，让标签能直接当纯文本画。
fn clean_text(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .replace("<br/>", " ")
        .replace("<br>", " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::estimate_width;

    /// 使用手册里的 5 张图：插件必须能吃下的真实样本。
    const GUIDE: &str = include_str!("../../showcase/diagrams.md");

    /// 展示页里的 3 张流程图（含三段回边的「发布流水线」），
    /// 和应用侧流程图页是同一份 `showcase/flowchart.md`。
    fn showcase_blocks() -> Vec<String> {
        super::super::showcase_blocks("flowchart")
    }

    fn mermaid_blocks(source: &str) -> Vec<String> {
        super::super::mermaid_blocks_of(source, "flowchart")
    }

    #[test]
    fn parses_every_diagram_in_the_manual() {
        let blocks = mermaid_blocks(GUIDE);
        assert_eq!(blocks.len(), 5, "手册里的流程图数量变了，需要重新确认插件");

        let mut saw_back_edge = false;
        for (ix, block) in blocks.iter().enumerate() {
            let graph = parse_graph(block).unwrap_or_else(|| panic!("第 {ix} 张图解析失败"));
            assert!(graph.nodes.len() >= 3, "第 {ix} 张图节点太少");
            assert!(graph.edges.len() >= 2, "第 {ix} 张图边太少");
            for edge in &graph.edges {
                assert!(edge.from < graph.nodes.len());
                assert!(edge.to < graph.nodes.len());
            }
            saw_back_edge |= graph.edges.iter().any(|edge| edge.back);
        }
        assert!(saw_back_edge, "「生成 → 保存 → 自动重查」应被识别成回边");
    }

    #[test]
    fn layout_keeps_nodes_inside_and_rows_apart() {
        for block in mermaid_blocks(GUIDE) {
            let graph = parse_graph(&block).expect("图应可解析");
            let layout = layout(&graph);
            assert!(layout.width > 0. && layout.height > 0.);

            for placed in &layout.nodes {
                assert!(placed.w > 0.);
                assert!(placed.x >= 0. && placed.x + placed.w <= layout.width + 0.5);
                assert!(placed.y >= 0. && placed.y + NODE_H <= layout.height + 0.5);
            }
            // 同一行不能横向重叠。
            for (ix, a) in layout.nodes.iter().enumerate() {
                for b in &layout.nodes[ix + 1..] {
                    if (a.y - b.y).abs() < 0.5 {
                        assert!(
                            a.x + a.w <= b.x + 0.5 || b.x + b.w <= a.x + 0.5,
                            "同一层的节点重叠了"
                        );
                    }
                }
            }
            // 连线的控制点必须落在画布内。
            for edge in &layout.edges {
                for (x, y) in edge.path.points() {
                    assert!(x >= 0. && x <= layout.width + 0.5);
                    assert!(y >= 0. && y <= layout.height + 0.5);
                }
            }
        }
    }

    #[test]
    fn long_edges_do_not_cross_intermediate_rows() {
        for block in mermaid_blocks(GUIDE) {
            let graph = parse_graph(&block).expect("图应可解析");
            let layout = layout(&graph);
            let left = layout
                .nodes
                .iter()
                .map(|node| node.x)
                .fold(f32::MAX, f32::min);
            let right = layout
                .nodes
                .iter()
                .map(|node| node.x + node.w)
                .fold(0., f32::max);

            for (ix, edge) in graph.edges.iter().enumerate() {
                match &layout.edges[ix].path {
                    // 绕行的边必须真的绕到节点区外面。
                    EdgePath::Elbow { points, .. } => assert!(
                        points.iter().any(|(x, _)| *x < left || *x > right),
                        "第 {ix} 条边绕了通道却没离开节点区"
                    ),
                    // 曲线自己采样，确认没压到中间那几层的节点。
                    EdgePath::Curve { p0, c1, c2, p3 } => {
                        let lo = layout.ranks[edge.from] + 1;
                        let hi = layout.ranks[edge.to];
                        for step in 0..=20 {
                            let t = step as f32 / 20.;
                            let mt = 1. - t;
                            let x = mt * mt * mt * p0.0
                                + 3. * mt * mt * t * c1.0
                                + 3. * mt * t * t * c2.0
                                + t * t * t * p3.0;
                            let y = mt * mt * mt * p0.1
                                + 3. * mt * mt * t * c1.1
                                + 3. * mt * t * t * c2.1
                                + t * t * t * p3.1;
                            for (node_ix, node) in layout.nodes.iter().enumerate() {
                                if layout.ranks[node_ix] < lo || layout.ranks[node_ix] >= hi {
                                    continue;
                                }
                                assert!(
                                    !(x > node.x + 1.
                                        && x < node.x + node.w - 1.
                                        && y > node.y + 1.
                                        && y < node.y + NODE_H - 1.),
                                    "第 {ix} 条曲线穿过了「{}」",
                                    graph.nodes[node_ix].label
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn edges_leave_and_enter_independently() {
        for block in mermaid_blocks(GUIDE) {
            let graph = parse_graph(&block).expect("图应可解析");
            let layout = layout(&graph);
            let mut exits: std::collections::HashMap<usize, Vec<f32>> = Default::default();
            let mut entries: std::collections::HashMap<usize, Vec<f32>> = Default::default();

            for (ix, edge) in graph.edges.iter().enumerate() {
                // 曲线的两端就是首尾控制点；折线的两端就是首尾折点。
                // 不管走通道还是走曲线，同一个节点上的锚点都不允许重合。
                let points = layout.edges[ix].path.points();
                let start = points.first().expect("每条边都有起点");
                let end = points.last().expect("每条边都有终点");
                exits.entry(edge.from).or_default().push(start.0);
                entries.entry(edge.to).or_default().push(end.0);
            }

            for (from, mut xs) in exits {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for pair in xs.windows(2) {
                    assert!(
                        pair[1] - pair[0] > 1.,
                        "节点 {from} 的两条出线共用了同一个锚点"
                    );
                }
            }
            for (to, mut xs) in entries {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for pair in xs.windows(2) {
                    assert!(
                        pair[1] - pair[0] > 1.,
                        "节点 {to} 的两条入线共用了同一个锚点"
                    );
                }
            }
        }
    }

    #[test]
    fn single_child_ranks_align_under_their_parent() {
        let blocks = mermaid_blocks(GUIDE);
        let graph = parse_graph(&blocks[0]).expect("激活流程图应可解析");
        let layout = layout(&graph);
        let find = |label: &str| {
            let ix = graph
                .nodes
                .iter()
                .position(|node| node.label == label)
                .unwrap_or_else(|| panic!("找不到节点 {label}"));
            layout.nodes[ix]
        };
        let parent = find("激活向导");
        let child = find("填注册码 + 授权账号密码");
        let parent_center = parent.x + parent.w / 2.;
        let child_center = child.x + child.w / 2.;
        assert!(
            (parent_center - child_center).abs() < 30.,
            "子节点没落在父节点正下方：{parent_center} vs {child_center}"
        );
    }

    #[test]
    fn edge_labels_do_not_overlap() {
        for block in mermaid_blocks(GUIDE) {
            let graph = parse_graph(&block).expect("图应可解析");
            let layout = layout(&graph);
            // 标签在界面上是「估算宽 + 左右各 3px 内边距、高约 16px」的小块。
            let rects: Vec<(&str, f32, f32, f32, f32)> = layout
                .edges
                .iter()
                .filter_map(|edge| edge.label.as_ref())
                .map(|(x, y, text)| {
                    let w = estimate_width(text, 11.) + 6.;
                    (text.as_str(), x - w / 2., y - 8., w, 16.)
                })
                .collect();
            for (i, a) in rects.iter().enumerate() {
                for b in rects.iter().skip(i + 1) {
                    let hit_x = a.1 < b.1 + b.3 && b.1 < a.1 + a.3;
                    let hit_y = a.2 < b.2 + b.4 && b.2 < a.2 + a.4;
                    assert!(
                        !(hit_x && hit_y),
                        "标签「{}」和「{}」叠在一起了: {:?} vs {:?}",
                        a.0,
                        b.0,
                        a,
                        b
                    );
                }
            }
        }
    }

    /// 把曲线采样成折线、折线保持折点，供「交叉 / 穿框」检测用。
    fn sample_path(path: &EdgePath) -> Vec<(f32, f32)> {
        match path {
            EdgePath::Curve { p0, c1, c2, p3 } => (0..=24)
                .map(|i| cubic_point(*p0, *c1, *c2, *p3, i as f32 / 24.))
                .collect(),
            EdgePath::Elbow { points, .. } => points.clone(),
        }
    }

    /// 两条线段是否真交叉（共端点不算）。
    fn segments_cross(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) -> bool {
        let o = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
        (d1 > 0.) != (d2 > 0.) && (d3 > 0.) != (d4 > 0.)
    }

    /// 列出一张图里所有「边 × 边」交叉，返回可读的描述。
    fn crossings(graph: &Graph, layout: &Layout) -> Vec<String> {
        let paths: Vec<Vec<(f32, f32)>> = layout
            .edges
            .iter()
            .map(|edge| sample_path(&edge.path))
            .collect();
        let mut hits = Vec::new();
        for i in 0..paths.len() {
            for j in i + 1..paths.len() {
                let hit = paths[i].windows(2).any(|a| {
                    paths[j]
                        .windows(2)
                        .any(|b| segments_cross(a[0], a[1], b[0], b[1]))
                });
                if hit {
                    hits.push(format!(
                        "{} → {}  ×  {} → {}",
                        graph.nodes[graph.edges[i].from].label,
                        graph.nodes[graph.edges[i].to].label,
                        graph.nodes[graph.edges[j].from].label,
                        graph.nodes[graph.edges[j].to].label
                    ));
                }
            }
        }
        hits
    }

    fn assert_no_crossings(source: &str, name: &str) {
        for (bi, block) in mermaid_blocks(source).iter().enumerate() {
            let graph = parse_graph(block).unwrap_or_else(|| panic!("{name} 第 {bi} 张图解析失败"));
            let layout = layout(&graph);
            let hits = crossings(&graph, &layout);
            assert!(
                hits.is_empty(),
                "{name} 第 {bi} 张图有 {} 处连线交叉:\n{}",
                hits.len(),
                hits.join("\n")
            );
        }
    }

    /// 连线是否从节点内部穿过；贴着边框出发 / 进入不算。
    fn node_hits(graph: &Graph, layout: &Layout) -> Vec<String> {
        let inside = |x: f32, y: f32, ix: usize| -> bool {
            let node = layout.nodes[ix];
            let cx = node.x + node.w / 2.;
            let cy = node.y + NODE_H / 2.;
            if graph.nodes[ix].shape == Shape::Diamond {
                let hw = node.w / 2. + 22.;
                let hh = NODE_H / 2. + 16.;
                // 内缩 4px：斜边上的锚点正好在轮廓上，不该算穿框。
                (x - cx).abs() / (hw - 4.) + (y - cy).abs() / (hh - 4.) < 1.
            } else {
                x > node.x + 3.
                    && x < node.x + node.w - 3.
                    && y > node.y + 3.
                    && y < node.y + NODE_H - 3.
            }
        };
        let mut hits = Vec::new();
        for (ix, edge) in layout.edges.iter().enumerate() {
            let points = sample_path(&edge.path);
            for segment in points.windows(2) {
                for step in 0..=10 {
                    let t = step as f32 / 10.;
                    let x = segment[0].0 + (segment[1].0 - segment[0].0) * t;
                    let y = segment[0].1 + (segment[1].1 - segment[0].1) * t;
                    for (ni, node) in graph.nodes.iter().enumerate() {
                        if inside(x, y, ni) {
                            hits.push(format!(
                                "{} → {}  穿过了「{}」",
                                graph.nodes[graph.edges[ix].from].label,
                                graph.nodes[graph.edges[ix].to].label,
                                node.label
                            ));
                            break;
                        }
                    }
                }
            }
        }
        hits.sort();
        hits.dedup();
        hits
    }

    #[test]
    fn fixture_edges_do_not_cross() {
        assert_no_crossings(GUIDE, "使用手册夹具");
    }

    #[test]
    fn showcase_edges_do_not_cross() {
        for (bi, block) in showcase_blocks().iter().enumerate() {
            assert_no_crossings(block, &format!("展示页第 {bi} 张"));
        }
    }

    #[test]
    fn edges_do_not_cross_nodes() {
        let mut sources = mermaid_blocks(GUIDE);
        sources.extend(showcase_blocks());
        for (bi, block) in sources.iter().enumerate() {
            let graph = parse_graph(block).expect("图应可解析");
            let layout = layout(&graph);
            let hits = node_hits(&graph, &layout);
            assert!(
                hits.is_empty(),
                "第 {bi} 张图有连线穿过节点:\n{}",
                hits.join("\n")
            );
        }
    }

    #[test]
    fn parses_labels_and_shapes() {
        let graph = parse_graph(
            "flowchart TD\n\
             A[启动软件] --> B{授权检查}\n\
             B -- 已激活 --> C[进入主界面]\n\
             B -->|首次| D(激活向导)\n\
             C -.-> D\n",
        )
        .expect("应可解析");

        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.nodes[1].label, "授权检查");
        assert!(graph.nodes[1].shape == Shape::Diamond);
        assert!(graph.nodes[3].shape == Shape::Round);
        let labels: Vec<_> = graph
            .edges
            .iter()
            .filter_map(|edge| edge.label.as_deref())
            .collect();
        assert!(labels.contains(&"已激活"));
        assert!(labels.contains(&"首次"));
    }
}
