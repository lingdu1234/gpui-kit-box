//! `flowchart` / `graph` 语法解析。
//!
//! 支持 `ID[文本]`、`ID{文本}`、`ID(文本)` 三种形状；连线按「左端 + 线身 + 右端」
//! 结构化解析：线身含 `.` 为虚线、含 `=` 为粗线，端点是 `>`（箭头）、`o`（圆圈）、
//! `x`（叉）或没有（`---` 开放连线），覆盖 `-->` / `-.->` / `==>` / `---` /
//! `--o` / `--x` / `<-->` 及其组合；边标注支持 `-->|文字|`、`-- 文字 -->`、
//! `-. 文字 .->`、`== 文字 ==>`。

use std::collections::HashMap;

use super::layout::*;

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

pub(super) fn parse_graph(source: &str) -> Option<Graph> {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    // 先扫一遍子图声明：边可以引用还没写到的子图（前向引用）。
    let mut sg_ids: HashMap<String, usize> = HashMap::new();
    let mut sg_labels: Vec<String> = Vec::new();
    for raw in source.lines() {
        let line = raw.trim();
        if !line.to_ascii_lowercase().starts_with("subgraph") {
            continue;
        }
        let (id, label) = subgraph_head(&line["subgraph".len()..]);
        let label = if label.is_empty() { id.clone() } else { label };
        sg_labels.push(label);
        if !id.is_empty() {
            sg_ids.insert(id, sg_labels.len() - 1);
        }
    }
    let mut overlays: Vec<Overlay> = sg_labels
        .iter()
        .map(|label| Overlay {
            label: label.clone(),
            anchor: None,
            members: Vec::new(),
        })
        .collect();

    // 正在解析的子图栈：块里的节点要同时记进本层和所有祖先子图的成员。
    let mut sg_stack: Vec<usize> = Vec::new();
    let mut decl_ix = 0usize;
    let mut direction = Direction::Td;

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        // 首行 `flowchart TD` / `graph LR`：取关键字后的方向 token。
        if lower.starts_with("flowchart") || lower.starts_with("graph") {
            let rest = if lower.starts_with("flowchart") { &line[9..] } else { &line[5..] };
            if let Some(tok) = rest.split_whitespace().next() {
                direction = Direction::parse(tok);
            }
            continue;
        }
        // 独立的 `direction LR` 语句同样改排布方向。
        if lower.starts_with("direction") {
            if let Some(tok) = line[9..].split_whitespace().next() {
                direction = Direction::parse(tok);
            }
            continue;
        }
        if lower.starts_with("style")
            || lower.starts_with("class")
            || lower.starts_with("linkstyle")
            || lower.starts_with("click")
        {
            continue;
        }
        if lower.starts_with("subgraph") {
            sg_stack.push(decl_ix);
            decl_ix += 1;
            continue;
        }
        if lower == "end" {
            sg_stack.pop();
            continue;
        }
        for statement in line.split(';') {
            let statement = statement.trim();
            if !statement.is_empty() {
                parse_statement(
                    statement,
                    &mut nodes,
                    &mut index,
                    &mut edges,
                    &mut overlays,
                    &sg_stack,
                    &sg_ids,
                );
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
        overlays,
        pin_edges: Vec::new(),
        notes: Vec::new(),
        direction,
    })
}

/// `subgraph` 后面的头部：`(id, 标题)`。`subgraph id [标题]` 取括号里的标题；
/// 没有括号时 id 与标题相同（带空格的纯标题没法被边引用，无所谓）。
fn subgraph_head(rest: &str) -> (String, String) {
    let rest = rest.trim();
    match rest.find('[') {
        Some(at) => {
            let close = rest[at..].find(']').map(|e| at + e).unwrap_or(rest.len());
            (
                rest[..at].trim().to_string(),
                clean_text(&rest[at + 1..close]),
            )
        }
        None => {
            let text = clean_text(rest);
            (text.clone(), text)
        }
    }
}

/// 边的端点：真实节点或子图外框。
enum Endpoint {
    Node(usize),
    Frame(usize),
}

fn parse_statement(
    statement: &str,
    nodes: &mut Vec<GraphNode>,
    index: &mut HashMap<String, usize>,
    edges: &mut Vec<GraphEdge>,
    overlays: &mut Vec<Overlay>,
    sg_stack: &[usize],
    sg_ids: &HashMap<String, usize>,
) {
    let mut chunks: Vec<String> = Vec::new();
    let mut labels: Vec<Option<String>> = Vec::new();
    let mut styles: Vec<(EdgeStyle, EdgeEnd, EdgeEnd, bool)> = Vec::new();
    let mut rest = statement;

    while let Some((at, len, style, l_end, r_end, invisible)) = parse_link(rest) {
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
        styles.push((style, l_end, r_end, invisible));
        rest = tail;
    }
    chunks.push(rest.trim().to_string());

    // 本语句碰到的节点：要记进所在子图（含祖先）的成员。
    let mut touched: Vec<usize> = Vec::new();
    let mut previous: Vec<Endpoint> = Vec::new();
    for (ix, chunk) in chunks.iter().enumerate() {
        let mut currents: Vec<Endpoint> = Vec::new();
        for piece in split_amp(chunk) {
            // 裸写且已声明的子图 id：端点连到外框，不是节点。
            if let Some(&sg) = sg_ids.get(piece.trim()) {
                currents.push(Endpoint::Frame(sg));
                continue;
            }
            let Some((id, label, shape)) = parse_node(&piece) else {
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
            currents.push(Endpoint::Node(current));
            touched.push(current);
        }
        if currents.is_empty() {
            // 整段都解析不出节点：链在这里断开。
            previous.clear();
            continue;
        }
        if !previous.is_empty() {
            let (style, l_end, r_end, invisible) = styles.get(ix - 1).copied().unwrap_or((
                EdgeStyle::Solid,
                EdgeEnd::Bare,
                EdgeEnd::Arrow,
                false,
            ));
            for from in &previous {
                for to in &currents {
                    edges.push(make_edge(
                        from,
                        to,
                        labels.get(ix - 1).cloned().flatten(),
                        style,
                        l_end,
                        r_end,
                        invisible,
                    ));
                }
            }
        }
        previous = currents;
    }
    for &oi in sg_stack {
        for &node in &touched {
            if !overlays[oi].members.contains(&node) {
                overlays[oi].members.push(node);
            }
        }
    }
}

/// 框边任一端连在子图外框上：占位端记 0，真实框号记在 from_sg / to_sg。
fn make_edge(
    from: &Endpoint,
    to: &Endpoint,
    label: Option<String>,
    style: EdgeStyle,
    left: EdgeEnd,
    right: EdgeEnd,
    invisible: bool,
) -> GraphEdge {
    let (from, from_sg) = match from {
        Endpoint::Node(ix) => (*ix, None),
        Endpoint::Frame(sg) => (0, Some(*sg)),
    };
    let (to, to_sg) = match to {
        Endpoint::Node(ix) => (*ix, None),
        Endpoint::Frame(sg) => (0, Some(*sg)),
    };
    GraphEdge {
        from,
        to,
        label,
        back: false,
        style,
        left,
        right,
        from_sg,
        to_sg,
        invisible,
    }
}

/// 按括号外的 `&` 切分多路节点：`A --> B & C`、`A & B --> C`。
/// 引号与括号里的 `&`（如 `A["AT&T"]`）不算分隔符。
fn split_amp(chunk: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut start = 0usize;
    for (i, ch) in chunk.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '[' | '{' | '(' if !in_quote => depth += 1,
            ']' | '}' | ')' if !in_quote => depth -= 1,
            '&' if depth == 0 && !in_quote => {
                out.push(chunk[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(chunk[start..].trim().to_string());
    out
}

/// 结构化解析连线记号 `[左端][线身][右端]`，返回
/// `(起始字节, 长度, 样式, 左端, 右端, 隐形)`，`at` 指向左端字符（没有左端时指向线首）。
///
/// 线身是连续的 `-` / `.` / `=` / `~`（含 `.` 为虚线、含 `=` 为粗线、含 `~` 为隐形）；
/// 左端是 `o` / `x` / `<`，右端是 `>` / `o` / `x`。实线的裸线身要三段以上（`---`）才算
/// 连线——两段的 `--` 是「`-- 文字 -->`」这种前标签的分隔符，不能吞掉。
fn parse_link(text: &str) -> Option<(usize, usize, EdgeStyle, EdgeEnd, EdgeEnd, bool)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for k in 0..chars.len() {
        let (at, ch) = chars[k];
        let mut i = k;
        let left = match ch {
            'o' => {
                i += 1;
                EdgeEnd::Circle
            }
            'x' => {
                i += 1;
                EdgeEnd::Cross
            }
            '<' => {
                i += 1;
                EdgeEnd::Arrow
            }
            _ => EdgeEnd::Bare,
        };
        let (mut dotted, mut thick, mut invisible, mut body) = (false, false, false, 0usize);
        while let Some(&(_, c)) = chars.get(i) {
            match c {
                '-' => {}
                '.' => dotted = true,
                '=' => thick = true,
                '~' => invisible = true,
                _ => break,
            }
            body += 1;
            i += 1;
        }
        let (right, had_end) = match chars.get(i) {
            Some(&(_, '>')) => (EdgeEnd::Arrow, true),
            Some(&(_, 'o')) => (EdgeEnd::Circle, true),
            Some(&(_, 'x')) => (EdgeEnd::Cross, true),
            _ => (EdgeEnd::Bare, false),
        };
        // 统一规则：裸线身要三段以上（`---` / `-.-` / `===` / `~~~`）才算连线；
        // 两段的 `--` / `-.` / `==` 是「`-- 文字 -->`」这类前标签的分隔符，
        // 只有紧跟右端符号（`-->` / `-.->` / `==>`）时才按两段算。
        let ok = body >= 3 || (body >= 2 && had_end);
        if ok && body > 0 {
            let end_at = chars
                .get(i + usize::from(had_end))
                .map(|&(at, _)| at)
                .unwrap_or(text.len());
            let style = if thick {
                EdgeStyle::Thick
            } else if dotted {
                EdgeStyle::Dotted
            } else {
                EdgeStyle::Solid
            };
            return Some((at, end_at - at, style, left, right, invisible));
        }
    }
    None
}

/// `B -- 已激活` → `("B", Some("已激活"))`：`--` 之后是边标签，不是节点文本。
/// 虚线 / 粗线的前标签（`A -. 文本 .-> B`）同理，分隔符取括号外**最靠后**的
/// `--` / `-.` / `==`，免得把 `A[x-.y]` 这种节点文字切坏。
fn split_trailing_label(chunk: &str) -> (String, Option<String>) {
    let bytes = chunk.as_bytes();
    let mut depth = 0i32;
    let mut cut: Option<usize> = None;
    for i in 0..bytes.len() {
        let two = bytes.len() >= i + 2;
        // 分隔符都是 ASCII；ASCII 字节必然落在字符边界上，这里才能安全切片。
        if bytes[i].is_ascii()
            && depth == 0
            && two
            && (chunk[i..].starts_with("--") || chunk[i..].starts_with("-.") || chunk[i..].starts_with("=="))
        {
            cut = Some(i);
        }
        match bytes[i] {
            b'[' | b'{' | b'(' => depth += 1,
            b']' | b'}' | b')' => depth -= 1,
            _ => {}
        }
    }
    match cut {
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
    // `>旗形]`：没有左括号可找，按右尖括号 + 右中括号识别。
    if !chunk.contains(['[', '{', '('])
        && let Some(at) = chunk.find('>')
        && chunk.ends_with(']')
    {
        let id = clean_text(chunk[..at].trim());
        if id.is_empty() {
            return None;
        }
        let text = clean_text(chunk[at + 1..chunk.len() - 1].trim());
        let label = if text.is_empty() { id.clone() } else { text };
        return Some((id, label, Shape::Flag));
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
        let (open, close, kind) = shape_delims(rest);
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

/// 括号对到形状的判别：先认最长的前缀（`(((` 在 `((` 之前、`([` 在 `(` 之前），
/// 平行四边形 / 梯形共用 `[/` 前缀，靠**结尾**两个字符区分斜的方向。
fn shape_delims(rest: &str) -> (&'static str, &'static str, Shape) {
    if rest.starts_with("(((") {
        ("(((", ")))", Shape::DoubleCircle)
    } else if rest.starts_with("((") {
        ("((", "))", Shape::Circle)
    } else if rest.starts_with("([") {
        ("([", "])", Shape::Stadium)
    } else if rest.starts_with("[(") {
        ("[(", ")]", Shape::Database)
    } else if rest.starts_with("[[") {
        ("[[", "]]", Shape::Subroutine)
    } else if rest.starts_with("{{") {
        ("{{", "}}", Shape::Hexagon)
    } else if rest.starts_with('{') {
        ("{", "}", Shape::Diamond)
    } else if rest.starts_with('(') {
        ("(", ")", Shape::Round)
    } else if rest.starts_with("[/") && rest.ends_with("\\]") {
        ("[/", "\\]", Shape::Trap)
    } else if rest.starts_with("[/") {
        ("[/", "/]", Shape::Para)
    } else if rest.starts_with("[\\") && rest.ends_with("/]") {
        ("[\\", "/]", Shape::TrapAlt)
    } else if rest.starts_with("[\\") {
        ("[\\", "\\]", Shape::ParaAlt)
    } else {
        ("[", "]", Shape::Rect)
    }
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

    /// 展示页里的 7 张流程图（含三段回边的「发布流水线」），
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

    #[test]
    fn parses_edge_styles_and_ends() {
        let graph = parse_graph(
            "flowchart TD\n\
             A --> B\n\
             B -.-> C\n\
             C ==> D\n\
             D --- E\n\
             E --o F\n\
             F --x G\n\
             H <--> I\n",
        )
        .expect("应可解析");
        let edges = &graph.edges;
        assert_eq!(edges.len(), 7, "七条连线都应解析出来");
        assert_eq!(edges[0].style, EdgeStyle::Solid);
        assert_eq!(edges[0].right, EdgeEnd::Arrow);
        assert_eq!(edges[1].style, EdgeStyle::Dotted);
        assert_eq!(edges[2].style, EdgeStyle::Thick);
        assert_eq!(edges[3].right, EdgeEnd::Bare, "`---` 是开放连线");
        assert_eq!(edges[4].right, EdgeEnd::Circle);
        assert_eq!(edges[5].right, EdgeEnd::Cross);
        assert_eq!(edges[6].left, EdgeEnd::Arrow, "`<-->` 两端都有箭头");
        assert_eq!(edges[6].right, EdgeEnd::Arrow);

        // 布局后样式落到 EdgeLine：开放连线没有三角，o/x 变成端点记号。
        let lay = layout(&graph);
        assert!(lay.edges[0].head.is_some());
        assert!(lay.edges[3].head.is_none());
        assert_eq!(lay.edges[1].style, EdgeStyle::Dotted);
        assert!(lay.edges[4].marks.iter().any(|&(_, _, m)| m == EdgeEnd::Circle));
        assert!(lay.edges[5].marks.iter().any(|&(_, _, m)| m == EdgeEnd::Cross));
        assert!(lay.edges[6].left_head.is_some());
    }

    #[test]
    fn ampersand_fans_out_and_in() {
        let graph = parse_graph(
            "flowchart TD\n\
             A --> B & C\n\
             B & C --> D\n\
             E & F --> G & H --> I\n\
             J[\"AT&T\"] --> K\n",
        )
        .expect("应可解析");
        assert_eq!(graph.nodes.len(), 11, "不应出现 & 拼成的怪节点");
        assert_eq!(graph.nodes[9].label, "AT&T", "引号里的 & 不是分隔符");
        assert_eq!(graph.edges.len(), 11);
        assert_eq!((graph.edges[0].from, graph.edges[0].to), (0, 1));
        assert_eq!((graph.edges[1].from, graph.edges[1].to), (0, 2));
        let hits = |a: usize, b: usize| {
            graph
                .edges
                .iter()
                .any(|edge| edge.from == a && edge.to == b)
        };
        assert!(hits(1, 3) && hits(2, 3), "B & C --> D 是两条入边");
        for (a, b) in [(4, 6), (4, 7), (5, 6), (5, 7), (6, 8), (7, 8)] {
            assert!(hits(a, b), "缺少 {a} → {b}");
        }
        assert!(hits(9, 10));
    }

    #[test]
    fn parses_all_node_shapes() {
        let src = r#"flowchart TD
    A([药丸]) --> B[(圆柱)]
    B --> C((圆))
    C --> D(((双圆)))
    D --> E[[子程序]]
    E --> F{{六边形}}
    F --> G[/平行四边形/]
    G --> H[\反平行四边形\]
    H --> I[/梯形\]
    I --> J[\反梯形/]
    J --> K>旗形]
"#;
        let graph = parse_graph(src).expect("应可解析");
        assert_eq!(graph.nodes.len(), 11);
        let want = [
            (Shape::Stadium, "药丸"),
            (Shape::Database, "圆柱"),
            (Shape::Circle, "圆"),
            (Shape::DoubleCircle, "双圆"),
            (Shape::Subroutine, "子程序"),
            (Shape::Hexagon, "六边形"),
            (Shape::Para, "平行四边形"),
            (Shape::ParaAlt, "反平行四边形"),
            (Shape::Trap, "梯形"),
            (Shape::TrapAlt, "反梯形"),
            (Shape::Flag, "旗形"),
        ];
        for (ix, (shape, label)) in want.iter().enumerate() {
            assert_eq!(graph.nodes[ix].shape, *shape, "第 {ix} 个节点形状不对");
            assert_eq!(graph.nodes[ix].label, *label);
        }
    }

    #[test]
    fn parses_subgraphs_and_frame_edges() {
        let src = r#"flowchart TD
    A[入口] --> B[处理]
    subgraph 盒子 [处理单元]
        B --> C[子步骤]
    end
    C --> D[出口]
    A --> 盒子
"#;
        let graph = parse_graph(src).expect("应可解析");
        assert_eq!(graph.nodes.len(), 4, "盒子是外框不是节点");
        assert_eq!(graph.overlays.len(), 1);
        let ov = &graph.overlays[0];
        assert_eq!(ov.label, "处理单元");
        assert!(ov.anchor.is_none(), "子图没有锚点");
        let node_ix = |label: &str| {
            graph
                .nodes
                .iter()
                .position(|node| node.label == label)
                .unwrap()
        };
        let (b, c) = (node_ix("处理"), node_ix("子步骤"));
        assert!(
            ov.members.contains(&b) && ov.members.contains(&c),
            "块内节点是成员"
        );
        assert!(
            !ov.members.contains(&node_ix("入口")) && !ov.members.contains(&node_ix("出口")),
            "块外节点不是成员"
        );
        // `A --> 盒子` 是框边：真实端 + 框端，占位端记 0。
        let a = node_ix("入口");
        let frame_in = graph
            .edges
            .iter()
            .find(|edge| edge.is_frame())
            .expect("A --> 盒子 应是框边");
        assert_eq!(frame_in.from, a);
        assert_eq!(frame_in.to_sg, Some(0));
        assert_eq!(frame_in.from_sg, None);
        // 布局不炸、框边不参与回边标记。
        let lay = layout(&graph);
        assert!(!lay.edges.is_empty());
    }

    #[test]
    fn frame_edges_anchor_on_the_border() {
        let src = r#"flowchart TD
    A[输入] --> 盒子
    subgraph 盒子 [处理]
        B --> C
    end
    盒子 --> D[输出]
"#;
        let graph = parse_graph(src).expect("应可解析");
        let lay = layout(&graph);
        assert_eq!(graph.overlays.len(), 1);
        let (x1, y1, x2, y2, label) = lay.overlays[0].clone();
        assert_eq!(label, "处理");
        // 成员全在框内。
        for &m in &graph.overlays[0].members {
            let p = lay.nodes[m];
            assert!(
                p.x >= x1 && p.x + p.w <= x2 && p.y >= y1 && p.y + NODE_H <= y2,
                "成员被挤出外框"
            );
        }
        // 框边贴着框边起止：起点/终点落在四条边之一上。
        let on_border = |(x, y): (f32, f32)| -> bool {
            const T: f32 = 0.5;
            ((x - x1).abs() <= T || (x - x2).abs() <= T) && y >= y1 - T && y <= y2 + T
                || ((y - y1).abs() <= T || (y - y2).abs() <= T) && x >= x1 - T && x <= x2 + T
        };
        let mut checked = 0;
        for (ix, edge) in graph.edges.iter().enumerate() {
            if !edge.is_frame() {
                continue;
            }
            let points = lay.edges[ix].path.points();
            if edge.to_sg.is_some() {
                assert!(
                    on_border(*points.last().expect("框边有终点")),
                    "进入子图的线要落在框边上"
                );
            }
            if edge.from_sg.is_some() {
                assert!(
                    on_border(*points.first().expect("框边有起点")),
                    "离开子图的线要从框边出发"
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 2, "进、出各一条框边");
        // 框后的节点落在外框下方。
        let d = graph
            .nodes
            .iter()
            .position(|node| node.label == "输出")
            .unwrap();
        assert!(lay.nodes[d].y >= y2, "框后的节点应在外框下方");
    }

    /// 展示页里带框边的图（服务拓扑）：框边端点必须贴在某个外框边上。
    #[test]
    fn showcase_subgraph_frame_edges_anchor_on_border() {
        let mut checked = 0;
        for block in showcase_blocks() {
            let Some(graph) = parse_graph(&block) else {
                continue;
            };
            if !graph.edges.iter().any(|edge| edge.is_frame()) {
                continue;
            }
            let lay = layout(&graph);
            let on_border = |(x, y): (f32, f32)| -> bool {
                const T: f32 = 0.5;
                lay.overlays.iter().any(|&(x1, y1, x2, y2, _)| {
                    ((x - x1).abs() <= T || (x - x2).abs() <= T)
                        && y >= y1 - T
                        && y <= y2 + T
                        || ((y - y1).abs() <= T || (y - y2).abs() <= T)
                            && x >= x1 - T
                            && x <= x2 + T
                })
            };
            for (ix, edge) in graph.edges.iter().enumerate() {
                if !edge.is_frame() {
                    continue;
                }
                let points = lay.edges[ix].path.points();
                if edge.to_sg.is_some() {
                    assert!(
                        on_border(*points.last().expect("框边有终点")),
                        "展示页第 {ix} 条框边的入点不在任何外框边上"
                    );
                }
                if edge.from_sg.is_some() {
                    assert!(
                        on_border(*points.first().expect("框边有起点")),
                        "展示页第 {ix} 条框边的出点不在任何外框边上"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked >= 3, "展示页应有一张带 3 条框边的子图");
    }

    #[test]
    fn nested_subgraphs_union_members() {        let src = r#"flowchart TD
    A --> B
    subgraph 外 [外层]
        subgraph 内 [内层]
            B --> C
        end
        C --> D
    end
"#;
        let graph = parse_graph(src).expect("应可解析");
        assert_eq!(graph.overlays.len(), 2);
        let (outer, inner) = (&graph.overlays[0], &graph.overlays[1]);
        let ix = |label: &str| {
            graph
                .nodes
                .iter()
                .position(|node| node.label == label)
                .unwrap()
        };
        let (b, c, d) = (ix("B"), ix("C"), ix("D"));
        assert_eq!(inner.members, vec![b, c], "内层只含自己的成员");
        assert_eq!(outer.members, vec![b, c, d], "祖先子图收下全部成员");
        assert_eq!(inner.label, "内层");
        assert_eq!(outer.label, "外层");
    }

    #[test]
    fn parses_labels_on_every_link_style() {
        let graph = parse_graph(
            "flowchart TD\n\
             A -- 文本 --> B\n\
             B -->|标注| C\n\
             C -. 虚字 .-> D\n\
             D == 粗字 ==> E\n",
        )
        .expect("应可解析");
        assert_eq!(graph.nodes.len(), 5, "标签不能被当成节点");
        let labels: Vec<_> = graph
            .edges
            .iter()
            .filter_map(|edge| edge.label.clone())
            .collect();
        for text in ["文本", "标注", "虚字", "粗字"] {
            assert!(labels.contains(&text.to_string()), "缺少标签「{text}」");
        }
        assert_eq!(graph.edges[2].style, EdgeStyle::Dotted);
        assert_eq!(graph.edges[3].style, EdgeStyle::Thick);
    }

    #[test]
    fn parses_direction_from_header_and_statement() {
        for (src, want) in [
            ("flowchart TD\nA-->B", Direction::Td),
            ("flowchart TB\nA-->B", Direction::Td),
            ("graph LR\nA-->B", Direction::Lr),
            ("flowchart RL\nA-->B", Direction::Rl),
            ("flowchart BT\nA-->B", Direction::Bt),
            ("flowchart TD\nA-->B\ndirection LR", Direction::Lr),
        ] {
            let g = parse_graph(src).expect("应可解析");
            assert_eq!(g.direction, want, "{src}");
        }
    }

    #[test]
    fn invisible_links_rank_but_are_flagged() {
        let g = parse_graph("flowchart TD\nA ~~~ B\n").expect("应可解析");
        assert_eq!(g.edges.len(), 1);
        assert!(g.edges[0].invisible, "`~~~` 应是隐形连线");
        let l = layout(&g);
        assert!(l.ranks[1] > l.ranks[0], "隐形连线仍要把 B 排在 A 之后");
        // 普通连线不带隐形标记。
        let g2 = parse_graph("flowchart TD\nA --> B\n").unwrap();
        assert!(!g2.edges[0].invisible);
    }

    #[test]
    fn bt_layout_is_exact_vertical_mirror() {
        let g = parse_graph(
            "flowchart TD\nA[甲]-->B{乙}\nB-->C[丙]\nB-->D[丁]\nC-->E\nD-->E",
        )
        .unwrap();
        let td = layout(&g);
        let mut gb = g.clone();
        gb.direction = Direction::Bt;
        let bt = layout(&gb);
        assert_eq!(td.width, bt.width);
        assert_eq!(td.height, bt.height);
        let h = td.height;
        for (a, b) in td.nodes.iter().zip(&bt.nodes) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.w, b.w);
            assert!((b.y - (h - a.y - NODE_H)).abs() < 1e-3);
        }
        for (da, db) in td.diamonds.iter().zip(&bt.diamonds) {
            assert_eq!(da.cx, db.cx);
            assert!((db.cy - (h - da.cy)).abs() < 1e-3);
        }
        for (a, b) in td.overlays.iter().zip(&bt.overlays) {
            assert!((b.1 - (h - a.3)).abs() < 1e-3);
            assert!((b.3 - (h - a.1)).abs() < 1e-3);
        }
        for (ea, eb) in td.edges.iter().zip(&bt.edges) {
            let (pa, pb) = (ea.path.points(), eb.path.points());
            assert_eq!(pa.len(), pb.len());
            for ((x, y), (x2, y2)) in pa.iter().zip(&pb) {
                assert!((x - x2).abs() < 1e-3);
                assert!((y2 - (h - y)).abs() < 1e-3);
            }
        }
    }

    /// 方框两两不重叠（按节点框，不含菱形外扩），供方向变换回归共用。
    fn assert_no_box_overlap(l: &Layout) {
        for (i, a) in l.nodes.iter().enumerate() {
            for b in &l.nodes[i + 1..] {
                let ox = a.x < b.x + b.w && b.x < a.x + a.w;
                let oy = a.y < b.y + NODE_H && b.y < a.y + NODE_H;
                assert!(!(ox && oy), "节点方框重叠了");
            }
        }
    }

    #[test]
    fn lr_and_rl_layouts_stay_inside_canvas() {
        let src = "flowchart TD\nA[入口]-->B{判断}\nB-->C[左]\nB-->D[右]\nC-->E[汇合]\nD-->E\nE-->A";
        for dir in [Direction::Lr, Direction::Rl] {
            let mut g = parse_graph(src).unwrap();
            g.direction = dir;
            let l = layout(&g);
            assert!(l.width > 0. && l.height > 0.);
            for p in &l.nodes {
                assert!(p.x >= 0. && p.x + p.w <= l.width + 0.5);
                assert!(p.y >= 0. && p.y + NODE_H <= l.height + 0.5);
            }
            assert_no_box_overlap(&l);
            for e in &l.edges {
                for (x, y) in e.path.points() {
                    assert!(x >= -0.5 && x <= l.width + 0.5, "{dir:?} 连线探出画布");
                    assert!(y >= -0.5 && y <= l.height + 0.5, "{dir:?} 连线探出画布");
                }
            }
            // 正向边沿排布方向推进：LR 向右、RL 向左。
            let cx = |i: usize| l.nodes[i].x + l.nodes[i].w / 2.;
            for e in &g.edges {
                if e.is_frame() || l.ranks[e.to] <= l.ranks[e.from] {
                    continue;
                }
                if dir == Direction::Lr {
                    assert!(cx(e.to) > cx(e.from), "LR 正向边应向右");
                } else {
                    assert!(cx(e.to) < cx(e.from), "RL 正向边应向左");
                }
            }
        }
    }

    #[test]
    fn showcase_flowcharts_reorient_without_overlap() {
        for block in showcase_blocks() {
            let base = parse_graph(&block).expect("展示页流程图应可解析");
            for dir in [Direction::Lr, Direction::Rl, Direction::Bt] {
                let mut g = base.clone();
                g.direction = dir;
                let l = layout(&g);
                for p in &l.nodes {
                    assert!(p.x >= -0.5 && p.x + p.w <= l.width + 0.5, "{dir:?} 节点探出画布");
                    assert!(p.y >= -0.5 && p.y + NODE_H <= l.height + 0.5, "{dir:?} 节点探出画布");
                }
                assert_no_box_overlap(&l);
            }
        }
    }

    #[test]
    fn lr_natural_size_is_wide_and_short() {
        // 方向变换在 layout() 内完成，node_link_size 据此报告自然尺寸：
        // LR 链应横向展开（宽 > 高），供等比适配包装算缩放。
        let g = parse_graph("flowchart LR\nA[甲]-->B[乙]-->C[丙]").unwrap();
        let (w, h) = super::super::node_link::node_link_size(&g);
        assert!(w > h, "LR 链应横向展开：{w} x {h}");
    }

    #[test]
    fn lr_fan_out_anchors_are_spread() {
        let mut g = parse_graph("flowchart TD\nA-->B\nA-->C\nA-->D").unwrap();
        g.direction = Direction::Lr;
        let l = layout(&g);
        // A 的三条出线都从右边出发，锚点纵向错开、不重合。
        let mut ys: Vec<f32> = l.edges.iter().map(|e| e.path.points()[0].1).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 1., "LR 扇出锚点重合了");
        }
    }

    /// RL 是 LR 的精确水平镜像：先共用同一套 LR 重排，再整体水平翻转。
    /// 节点、菱形、外框、连线路径都应逐点镜像，y 不变。
    #[test]
    fn rl_is_exact_horizontal_mirror_of_lr() {
        let g = parse_graph(
            "flowchart TD\nA[甲]-->B{乙}\nB-->C[丙]\nB-->D[丁]\nC-->E\nD-->E\nE-->A",
        )
        .unwrap();
        let mut gl = g.clone();
        gl.direction = Direction::Lr;
        let lr = layout(&gl);
        let mut gr = g.clone();
        gr.direction = Direction::Rl;
        let rl = layout(&gr);
        assert_eq!(lr.width, rl.width);
        assert_eq!(lr.height, rl.height);
        let w = lr.width;
        for (a, b) in lr.nodes.iter().zip(&rl.nodes) {
            assert_eq!(a.y, b.y);
            assert_eq!(a.w, b.w);
            assert!((b.x - (w - a.x - a.w)).abs() < 1e-3);
        }
        for (da, db) in lr.diamonds.iter().zip(&rl.diamonds) {
            assert_eq!(da.cy, db.cy);
            assert!((db.cx - (w - da.cx)).abs() < 1e-3);
        }
        for (a, b) in lr.overlays.iter().zip(&rl.overlays) {
            assert!((b.0 - (w - a.2)).abs() < 1e-3);
            assert!((b.2 - (w - a.0)).abs() < 1e-3);
        }
        for (ea, eb) in lr.edges.iter().zip(&rl.edges) {
            let (pa, pb) = (ea.path.points(), eb.path.points());
            assert_eq!(pa.len(), pb.len());
            for ((x, y), (x2, y2)) in pa.iter().zip(&pb) {
                assert!((y - y2).abs() < 1e-3);
                assert!((x2 - (w - x)).abs() < 1e-3);
            }
        }
    }

    /// BT 链自下而上：起点在最底、终点在最顶，单链横向居中成一行。
    #[test]
    fn bt_chain_reverses_visual_order() {
        let g = parse_graph("flowchart BT\nA[甲]-->B[乙]-->C[丙]").unwrap();
        let l = layout(&g);
        assert!(l.nodes[0].y > l.nodes[1].y, "BT 起点应在下方");
        assert!(l.nodes[1].y > l.nodes[2].y, "BT 终点应在上方");
        assert!(
            (l.nodes[0].x - l.nodes[2].x).abs() < 1.0,
            "单链应横向居中对齐"
        );
    }

    /// 隐形连线与可见连线 / 标签混用：`~~~` 与 `~~~~` 都隐形，仍参与分层，
    /// 但不带标签；可见边的标签照常保留。
    #[test]
    fn invisible_links_combine_with_visible_and_labels() {
        let g = parse_graph("flowchart TD\nA ~~~ B\nB --> C\nA -->|x| D\nA ~~~~ E\n").unwrap();
        assert_eq!(g.edges.len(), 4);
        assert_eq!(
            g.edges.iter().filter(|e| e.invisible).count(),
            2,
            "~~~ 与 ~~~~ 都是隐形连线"
        );
        let l = layout(&g);
        let rank = |label: &str| {
            let ix = g.nodes.iter().position(|n| n.label == label).unwrap();
            l.ranks[ix]
        };
        assert!(rank("B") > rank("A"));
        assert!(rank("E") > rank("A"));
        assert!(rank("C") > rank("B"));
        assert!(rank("D") > rank("A"));
        assert!(g.edges.iter().any(|e| !e.invisible && e.label.as_deref() == Some("x")));
        assert!(g.edges.iter().filter(|e| e.invisible).all(|e| e.label.is_none()));
    }

    /// 方向变换 + 子图：节点不越界、不重叠，外框在画布内且包住全部成员。
    #[test]
    fn direction_with_subgraph_stays_inside() {
        let src = "flowchart TD\nA[入口]-->B\nsubgraph 盒 [处理]\nB-->C\nC-->D\nend\nD-->E[出口]\nA-->盒";
        let base = parse_graph(src).unwrap();
        for dir in [Direction::Lr, Direction::Rl, Direction::Bt] {
            let mut g = base.clone();
            g.direction = dir;
            let l = layout(&g);
            for p in &l.nodes {
                assert!(p.x >= -0.5 && p.x + p.w <= l.width + 0.5, "{dir:?} 节点探出画布");
                assert!(p.y >= -0.5 && p.y + NODE_H <= l.height + 0.5, "{dir:?} 节点探出画布");
            }
            assert_no_box_overlap(&l);
            for (oi, ov) in g.overlays.iter().enumerate() {
                let (x1, y1, x2, y2, _) = l.overlays[oi];
                assert!(
                    x1 >= -0.5 && y1 >= -0.5 && x2 <= l.width + 0.5 && y2 <= l.height + 0.5,
                    "{dir:?} 外框探出画布"
                );
                for &m in &ov.members {
                    let p = l.nodes[m];
                    assert!(p.x >= x1 - 0.5 && p.x + p.w <= x2 + 0.5, "{dir:?} 成员挤出外框");
                    assert!(p.y >= y1 - 0.5 && p.y + NODE_H <= y2 + 0.5, "{dir:?} 成员挤出外框");
                }
            }
        }
    }
}
