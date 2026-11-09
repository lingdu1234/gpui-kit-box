//! Markdown 插件：Mermaid `flowchart` 渲染。
//!
//! 只支持手册实际用到的子集：`flowchart TD/TB` 的节点与箭头。
//!
//! ```text
//! flowchart TD
//!     A[启动软件] --> B{授权检查}
//!     B -- 已激活 --> C[进入主界面]
//!     B -->|首次| D[激活向导]
//! ```
//!
//! 支持 `ID[文本]`、`ID{文本}`、`ID(文本)` 三种形状，`-->` / `-.->` / `==>` 三种箭头，
//! 以及 `-->|文字|` 和 `-- 文字 -->` 两种边标注。解析不了的图（`sequenceDiagram`、
//! `subgraph` 之类）直接返回 `None`，交给默认代码块渲染，不会吞内容。
//!
//! 排版思路：按最长路径分层（忽略回边），同层节点横排居中；正向边走「下-横-下」
//! 折线，回边走右侧专用通道，箭头与折线用 `canvas` 画，节点与边标注用绝对定位的
//! `div` 画——文字排版与居中直接复用 GPUI 的布局能力。

use std::collections::HashMap;

use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, PathBuilder, Pixels, Point, Styled, Window,
    canvas, div, point, px,
};

use super::estimate_width;

/// 关闭「已弃用 / 冗余写法」的告警没有意义，这里保持安静。
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
        let graph = parse_graph(&code.value)?;
        Some(
            MarkdownNode::new("mermaid", graph)
                .text(code.value.clone())
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(graph) = node.data::<Graph>() else {
            return div().into_any_element();
        };
        render_graph(graph, cx)
    }
}

// ---------------------------------------------------------------------------
// 图结构
// ---------------------------------------------------------------------------

const NODE_H: f32 = 40.;
const H_GAP: f32 = 72.;
/// 层间距的基数：每层之间至少留这么多高度，曲线与标签才不挤。
const GAP_BASE: f32 = 84.;
const PAD: f32 = 14.;
const FONT: f32 = 13.;
/// 侧通道宽度：跨层长边与回边从这里绕开中间的行。
const LANE_W: f32 = 36.;
/// 节点区与第一条侧通道之间再留一段空白，长线才会明显走在图的两侧。
const LANE_MARGIN: f32 = 30.;
const MIN_W: f32 = 140.;
const MAX_W: f32 = 260.;

/// 指向目标框底部的箭头（回退边从下方收进去）。
fn arrow_up(tx: f32, ty: f32) -> [(f32, f32); 3] {
    [(tx - 5., ty + 10.), (tx + 5., ty + 10.), (tx, ty)]
}

/// 指向目标框顶部的箭头（正向边与长边的收尾一致）。
fn arrow_down(tx: f32, ty: f32) -> [(f32, f32); 3] {
    [(tx - 5., ty - 10.), (tx + 5., ty - 10.), (tx, ty)]
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Rect,
    Round,
    Diamond,
}

#[derive(Clone)]
struct GraphNode {
    label: String,
    shape: Shape,
}

#[derive(Clone)]
struct GraphEdge {
    from: usize,
    to: usize,
    label: Option<String>,
    /// 指向已排在前面的层：画成右侧回边通道。
    back: bool,
}

#[derive(Clone)]
struct Graph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

fn parse_graph(source: &str) -> Option<Graph> {
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
    Some(Graph { nodes, edges })
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

/// DFS 标出回边：指向「当前还在递归栈里」的节点的边。
fn mark_back_edges(edges: &mut [GraphEdge], count: usize) {
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in edges.iter().enumerate() {
        adjacency[edge.from].push(ix);
    }
    let mut color = vec![0u8; count];
    let mut back = vec![false; edges.len()];
    for root in 0..count {
        if color[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        color[root] = 1;
        while let Some((node, next)) = stack.pop() {
            if next >= adjacency[node].len() {
                color[node] = 2;
                continue;
            }
            stack.push((node, next + 1));
            let edge_ix = adjacency[node][next];
            let target = edges[edge_ix].to;
            match color[target] {
                1 => back[edge_ix] = true,
                0 => {
                    color[target] = 1;
                    stack.push((target, 0));
                }
                _ => {}
            }
        }
    }
    for (ix, edge) in edges.iter_mut().enumerate() {
        edge.back = back[ix];
    }
}

// ---------------------------------------------------------------------------
// 布局
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Placed {
    x: f32,
    y: f32,
    w: f32,
}

struct EdgeLine {
    path: EdgePath,
    head: [(f32, f32); 3],
    label: Option<(f32, f32, String)>,
}

#[derive(Clone, Debug)]
enum EdgePath {
    /// 相邻层：三次贝塞尔，从源框底部平滑弯到目标框顶部。
    Curve {
        p0: (f32, f32),
        c1: (f32, f32),
        c2: (f32, f32),
        p3: (f32, f32),
    },
    /// 长边 / 回边：沿侧通道走的折线，拐角做圆角。
    Elbow { points: Vec<(f32, f32)>, radius: f32 },
}

impl EdgePath {
    #[cfg(test)]
    fn points(&self) -> Vec<(f32, f32)> {
        match self {
            EdgePath::Curve { p0, c1, c2, p3 } => vec![*p0, *c1, *c2, *p3],
            EdgePath::Elbow { points, .. } => points.clone(),
        }
    }
}

struct Layout {
    width: f32,
    height: f32,
    nodes: Vec<Placed>,
    edges: Vec<EdgeLine>,
    /// 判断框画成菱形，尺寸比文字框大一圈，单独记录给 canvas 画。
    diamonds: Vec<Diamond>,
    /// 每个节点所在的层，供测试判断一条边是否跨层。
    #[allow(dead_code)]
    ranks: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Diamond {
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
}

/// 重心法的一趟：把每层节点按它在相邻层的邻居平均位置重排。
fn barycenter_pass(
    order: &mut [Vec<usize>],
    index_of: &mut [f32],
    graph: &Graph,
    escaping: &[bool],
    downward: bool,
) {
    let rows: Vec<usize> = if downward {
        (1..order.len()).collect()
    } else {
        (0..order.len().saturating_sub(1)).rev().collect()
    };
    for row in rows {
        let mut keyed: Vec<(f32, usize)> = order[row]
            .iter()
            .map(|&node| {
                let mut keys = Vec::new();
                for (edge, &escaping) in graph.edges.iter().zip(escaping) {
                    if escaping {
                        continue;
                    }
                    if downward && edge.to == node {
                        keys.push(index_of[edge.from]);
                    } else if !downward && edge.from == node {
                        keys.push(index_of[edge.to]);
                    }
                }
                let key = if keys.is_empty() {
                    index_of[node]
                } else {
                    keys.iter().sum::<f32>() / keys.len() as f32
                };
                (key, node)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        order[row] = keyed.into_iter().map(|(_, node)| node).collect();
        for (slot, &node) in order[row].iter().enumerate() {
            index_of[node] = slot as f32;
        }
    }
}

/// 坐标修正的一趟：节点向邻居的中心靠拢，再按层内顺序推开重叠。
fn coordinate_pass(
    order: &[Vec<usize>],
    pos: &mut [f32],
    widths: &[f32],
    graph: &Graph,
    escaping: &[bool],
    downward: bool,
) {
    let rows: Vec<usize> = if downward {
        (1..order.len()).collect()
    } else {
        (0..order.len().saturating_sub(1)).rev().collect()
    };
    for row in rows {
        let desired: Vec<(f32, usize)> = order[row]
            .iter()
            .map(|&node| {
                let mut centers: Vec<f32> = Vec::new();
                for (edge, &escaping) in graph.edges.iter().zip(escaping) {
                    if escaping {
                        continue;
                    }
                    let linked = if downward {
                        edge.to == node
                    } else {
                        edge.from == node
                    };
                    if linked {
                        let other = if downward { edge.from } else { edge.to };
                        centers.push(pos[other] + widths[other] / 2.);
                    }
                }
                centers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let center = match centers.len() {
                    0 => pos[node] + widths[node] / 2.,
                    len if len % 2 == 1 => centers[len / 2],
                    len => (centers[len / 2 - 1] + centers[len / 2]) / 2.,
                };
                (center - widths[node] / 2., node)
            })
            .collect();
        // 这里**不能**再按期望位置排序：层内顺序由重心法决定，重新排序会让
        // 宽一点的节点（期望 x 小几像素）插到前面去，把分支顺序打乱。
        let mut cursor = f32::MIN;
        for (want, node) in desired {
            let x = want.max(cursor);
            pos[node] = x;
            cursor = x + widths[node] + H_GAP;
        }
    }
}

/// 三次贝塞尔曲线上 `t` 处的点。
fn cubic_point(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let mt = 1. - t;
    let (a, b, c, d) = (mt * mt * mt, 3. * mt * mt * t, 3. * mt * t * t, t * t * t);
    (
        a * p0.0 + b * c1.0 + c * c2.0 + d * p3.0,
        a * p0.1 + b * c1.1 + c * c2.1 + d * p3.1,
    )
}

/// 这条边直着下去会不会穿过中间某层的节点。
///
/// 相邻层永远不挡（中间没有行）；跨层的边按三次曲线的采样点去撞中间各行的方框。
/// 只有真的会撞才值得绕侧通道——否则从侧边水平出发会横穿整行，把一排节点串成一条线。
fn blocked_between(
    nodes: &[Placed],
    rank: &[usize],
    from: usize,
    to: usize,
    from_is_diamond: bool,
) -> bool {
    let lo = rank[from] + 1;
    let hi = rank[to];
    if lo >= hi {
        return false;
    }
    let source = nodes[from];
    let target = nodes[to];
    let sx = source.x + source.w / 2.;
    let sy = if from_is_diamond {
        source.y + NODE_H / 2. + 16.
    } else {
        source.y + NODE_H
    };
    let tx = target.x + target.w / 2.;
    let ty = target.y;
    let dy = (ty - sy).max(1.);
    let c1 = (sx, sy + dy * 0.45);
    let c2 = (tx, ty - dy * 0.45);

    for step in 0..=20 {
        let t = step as f32 / 20.;
        let mt = 1. - t;
        let x = mt * mt * mt * sx + 3. * mt * mt * t * c1.0 + 3. * mt * t * t * c2.0 + t * t * t * tx;
        let y = mt * mt * mt * sy + 3. * mt * mt * t * c1.1 + 3. * mt * t * t * c2.1 + t * t * t * ty;
        for (ix, node) in nodes.iter().enumerate() {
            if rank[ix] < lo || rank[ix] >= hi {
                continue;
            }
            // 只留 2px 余量：留太宽会把「擦着框边过去」的曲线误判成穿框，
            // 白绕一圈侧通道。
            if x > node.x - 2.
                && x < node.x + node.w + 2.
                && y > node.y - 2.
                && y < node.y + NODE_H + 2.
            {
                return true;
            }
        }
    }
    false
}

fn layout(graph: &Graph) -> Layout {
    let count = graph.nodes.len();

    // 分层：只走非回边，最长路径。回边已经排除，所以不会死循环。
    let mut rank = vec![0usize; count];
    for _ in 0..count {
        let mut changed = false;
        for edge in graph.edges.iter().filter(|edge| !edge.back) {
            if rank[edge.to] < rank[edge.from] + 1 {
                rank[edge.to] = rank[edge.from] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let depth = rank.iter().copied().max().unwrap_or(0) + 1;

    // 向上或同层的边（除 DFS 回边外，还有「循环里跳回上层」这种回退边）同样要走侧通道：
    // 直着连会往回画成一条被压扁的曲线，而且会穿过中间的行。
    let escaping: Vec<bool> = graph
        .edges
        .iter()
        .map(|edge| edge.back || rank[edge.to] <= rank[edge.from])
        .collect();

    let widths: Vec<f32> = graph
        .nodes
        .iter()
        .map(|node| (estimate_width(&node.label, FONT) + 30.).clamp(MIN_W, MAX_W))
        .collect();

    // ---- 层内顺序：重心法交叉最小化 ----
    // 光靠出现顺序排，会有边横穿整个图；重心法把节点往邻居那一侧挪，
    // 交叉数明显下降（「填注册码」就会落在「激活向导」正下方）。
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for (ix, &row) in rank.iter().enumerate() {
        order[row].push(ix);
    }
    let mut index_of = vec![0f32; count];
    for row in &order {
        for (slot, &node) in row.iter().enumerate() {
            index_of[node] = slot as f32;
        }
    }
    for sweep in 0..6 {
        barycenter_pass(&mut order, &mut index_of, graph, &escaping, sweep % 2 == 0);
    }

    // ---- 横向坐标：向邻居中位数靠拢，再按层内顺序消除重叠 ----
    let mut pos = vec![0f32; count];
    for row in &order {
        let mut x = 0.;
        for &node in row {
            pos[node] = x;
            x += widths[node] + H_GAP;
        }
    }
    // 交替七趟后停在「向下」这一趟：收尾要让孩子跟着父亲走，否则最后一趟
    // 是向上的话，汇聚节点会留在上一次的错误位置（「进入主界面」就偏到了左边）。
    for pass in 0..11 {
        coordinate_pass(&order, &mut pos, &widths, graph, &escaping, pass % 2 == 0);
        let min = pos.iter().copied().fold(f32::MAX, f32::min);
        for value in pos.iter_mut() {
            *value -= min;
        }
    }

    // 收尾两件事：
    // 1. 分支主干（入度 ≤ 1、出度 ≥ 2，例如「授权检查」）对齐到它下面分支的中位数，
    //    否则会歪在第一个分支上方；
    // 2. 起点（无入边）对齐到它的下一步。
    // 汇聚节点（入度 ≥ 2，比如「进入主界面」）不参与，保持上面算好的位置。
    // 按层从下往上处理，父亲用的是已经定好的孩子坐标。
    let mut by_rank_desc: Vec<usize> = (0..count).collect();
    by_rank_desc.sort_by_key(|&node| std::cmp::Reverse(rank[node]));

    for &node in &by_rank_desc {
        let in_deg = graph.edges.iter().filter(|e| e.to == node && !e.back).count();
        let outs: Vec<f32> = graph
            .edges
            .iter()
            .filter(|edge| edge.from == node && !edge.back)
            .map(|edge| pos[edge.to] + widths[edge.to] / 2.)
            .collect();
        let aligns = (in_deg <= 1 && outs.len() >= 2) || (in_deg == 0 && !outs.is_empty());
        if !aligns {
            continue;
        }
        let mut outs = outs;
        outs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        pos[node] = outs[outs.len() / 2] - widths[node] / 2.;
    }

    // 对齐后可能挤到一起，再按层内顺序推开一次。
    for list in &order {
        let mut cursor = f32::MIN;
        for &node in list {
            let x = pos[node].max(cursor);
            pos[node] = x;
            cursor = x + widths[node] + H_GAP;
        }
    }
    let min = pos.iter().copied().fold(f32::MAX, f32::min);
    for value in pos.iter_mut() {
        *value -= min;
    }

    // 整张图按实际占宽收边；行内不做二次居中，否则单节点的层会被硬拉到画面中央，
    // 反而离开它前驱的正下方（「填注册码」就是这种情况）。
    let provisional: Vec<Placed> = (0..count)
        .map(|node| Placed {
            x: pos[node],
            y: PAD + rank[node] as f32 * (NODE_H + GAP_BASE),
            w: widths[node],
        })
        .collect();

    // 每一排最左、最右的节点：长边从这一侧收进目标时，同排没有别的节点挡路。
    let mut free_left = vec![false; count];
    let mut free_right = vec![false; count];
    for list in &order {
        let Some(left) = list
            .iter()
            .min_by(|a, b| pos[**a].partial_cmp(&pos[**b]).unwrap())
        else {
            continue;
        };
        let right = *list
            .iter()
            .max_by(|a, b| pos[**a].partial_cmp(&pos[**b]).unwrap())
            .unwrap();
        free_left[*left] = true;
        free_right[right] = true;
    }

    // 只有真的会被节点挡住的跨层边才走侧通道。跨两层的边如果直着下去不会
    // 撞到中间那行的节点（比如「人员认证」正下方是空的），就走普通曲线，
    // 从侧边水平出发会横穿整行，把一排节点串成一条线。
    let mut lane_of: Vec<Option<(bool, usize)>> = vec![None; graph.edges.len()];
    // 通道分左右两侧，各自独立编号；选哪一侧看目标在源的左边还是右边，
    // 否则长边会横穿整张图去绕远路，交叉数量翻倍。
    let mut left_slots = 0usize;
    let mut right_slots = 0usize;
    // 先挑出所有要绕行的边，再按「跨过的层数」从小到大分配通道：短边占内侧、
    // 长边占外侧，长边的两条水平段就落在短边竖线区间之外，不会横穿内侧竖线。
    // 按源码出现顺序分配恰好相反——后出现的边被排到外侧，水平段要横穿前面
    // 所有通道的竖线，发布流水线里那 5 处交叉就是这么来的。
    let mut detours: Vec<usize> = (0..graph.edges.len())
        .filter(|&ix| {
            let edge = &graph.edges[ix];
            escaping[ix]
                || blocked_between(
                    &provisional,
                    &rank,
                    edge.from,
                    edge.to,
                    graph.nodes[edge.from].shape == Shape::Diamond,
                )
        })
        .collect();
    detours.sort_by_key(|&ix| {
        let edge = &graph.edges[ix];
        (rank[edge.from].abs_diff(rank[edge.to]), ix)
    });
    for ix in detours {
        let edge = &graph.edges[ix];
        let from_center = provisional[edge.from].x + provisional[edge.from].w / 2.;
        let to_center = provisional[edge.to].x + provisional[edge.to].w / 2.;
        // 选边打分：目标那一侧空着（能从侧边收进去）记 2 分，源那一侧空着
        // （能从侧边出去）记 1 分，取分高的一侧；同分再看谁离得近。
        let left_score =
            free_left[edge.to] as i32 * 2 + free_left[edge.from] as i32;
        let right_score =
            free_right[edge.to] as i32 * 2 + free_right[edge.from] as i32;
        let left = match left_score.cmp(&right_score) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => to_center < from_center,
        };
        let index = if left {
            left_slots += 1;
            left_slots - 1
        } else {
            right_slots += 1;
            right_slots - 1
        };
        lane_of[ix] = Some((left, index));
    }
    let left_lanes = left_slots;
    let right_lanes = right_slots;

    // 从目标侧边收进去的长边：同一个目标同一侧要多条时，按高度错开。
    // 菱形只有顶点，仍然走上下；方框走侧边。
    let mut side_entry = vec![false; graph.edges.len()];
    let mut side_group: HashMap<(usize, bool), Vec<usize>> = HashMap::new();
    for (ix, edge) in graph.edges.iter().enumerate() {
        let Some((left, _)) = lane_of[ix] else {
            continue;
        };
        let free = if left {
            free_left[edge.to]
        } else {
            free_right[edge.to]
        };
        // 菱形也允许从侧边收进去，但落点在侧顶点上方的斜边中段（不是角上），
        // 这样横线始终在它向下发散的那些兄弟线之上。
        if free {
            side_entry[ix] = true;
            side_group.entry((edge.to, left)).or_default().push(ix);
        }
    }
    // 同侧多条时按通道编号排序：内侧的边先收（收尾横线离目标更远），
    // 外侧的边后收（收尾横线更贴目标），横线才会从内侧竖线的终点外侧通过，
    // 不会把几条并行通道串成网格。
    for list in side_group.values_mut() {
        list.sort_by_key(|&e| lane_of[e].map(|(_, index)| index).unwrap_or(0));
    }

    let max_row = (0..count)
        .map(|node| pos[node] + widths[node])
        .fold(0., f32::max);

    // 节点区整体右移，给左侧通道让位。
    let node_x0 = PAD + LANE_W * left_lanes as f32 + LANE_MARGIN;
    let lane_left = |index: usize| PAD + LANE_W * (index as f32 + 0.5);
    let lane_right = |index: usize| {
        node_x0 + max_row + LANE_MARGIN + LANE_W * (index as f32 + 0.5)
    };
    let width = node_x0 + max_row + LANE_MARGIN + LANE_W * right_lanes as f32 + PAD;

    let mut nodes = vec![
        Placed {
            x: 0.,
            y: 0.,
            w: 0.,
        };
        count
    ];
    for (row, list) in order.iter().enumerate() {
        let y = PAD + row as f32 * (NODE_H + GAP_BASE);
        for &node in list {
            nodes[node] = Placed {
                x: node_x0 + pos[node],
                y,
                w: widths[node],
            };
        }
    }
    let height = PAD * 2. + depth as f32 * NODE_H + (depth.saturating_sub(1) as f32) * GAP_BASE;

    let center = |ix: usize| nodes[ix].x + nodes[ix].w / 2.;
    let by_target = |a: &usize, b: &usize| {
        center(graph.edges[*a].to)
            .partial_cmp(&center(graph.edges[*b].to))
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let by_source = |a: &usize, b: &usize| {
        center(graph.edges[*a].from)
            .partial_cmp(&center(graph.edges[*b].from))
            .unwrap_or(std::cmp::Ordering::Equal)
    };

    // 同一个源的多条出边在框底均匀分开，各自独立成线。
    // 侧通道的边也参与分配出口，否则它会和普通边共用同一个点，箭头叠在一起。
    let mut exits: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in graph.edges.iter().enumerate() {
        exits[edge.from].push(ix);
    }
    for list in &mut exits {
        list.sort_by(by_target);
    }

    // 同一个目标的入边按来源从左到右排，入点依次错开。
    let mut entries: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in graph.edges.iter().enumerate() {
        entries[edge.to].push(ix);
    }
    for list in &mut entries {
        list.sort_by(by_source);
    }

    let slot_of = |list: &Vec<usize>, ix: usize| list.iter().position(|&x| x == ix).unwrap_or(0);
    let spread = |slot: usize, count: usize| (slot as f32 + 1.) / (count.max(1) as f32 + 1.);

    // 进出点的横向位置：方框按内容宽度均分；菱形把锚点摊在两条斜边的中段
    // （不到顶点、也不挤在中心），既分散又能让侧通道的横线从它们上方通过。
    let anchor_x = |node: usize, fraction: f32| -> f32 {
        let placed = &nodes[node];
        if graph.nodes[node].shape == Shape::Diamond {
            let hw = placed.w / 2. + 22.;
            placed.x + placed.w / 2. + (fraction - 0.5) * 2. * hw * 0.55
        } else {
            placed.x + placed.w * fraction
        }
    };

    // 进出点的纵向位置：方框取上下边；菱形按斜边求交点，让多个箭头落在
    // 菱形轮廓上，而不是全都挤在顶点那一个点上。
    let shape_y = |node: usize, x: f32, top: bool| -> f32 {
        let placed = &nodes[node];
        if graph.nodes[node].shape == Shape::Diamond {
            let cx = placed.x + placed.w / 2.;
            let hw = placed.w / 2. + 22.;
            let hh = NODE_H / 2. + 16.;
            let dx = (x - cx).abs().min(hw);
            let dy = hh * (1. - dx / hw);
            if top {
                placed.y + NODE_H / 2. - dy
            } else {
                placed.y + NODE_H / 2. + dy
            }
        } else if top {
            placed.y
        } else {
            placed.y + NODE_H
        }
    };

    let mut edges: Vec<EdgeLine> = graph
        .edges
        .iter()
        .enumerate()
        .map(|(ix, edge)| {
            let from = &nodes[edge.from];
            let to = &nodes[edge.to];
            let entry_list = &entries[edge.to];
            // 每一条入线都占一个独立入点（菱形也一样），箭头不会再叠在一起。
            let tx = anchor_x(edge.to, spread(slot_of(entry_list, ix), entry_list.len()));

            match lane_of[ix] {
                // 长边 / 回边：从源框的底边（或顶边）出，沿通道纵向走，
                // 再从目标框的正上方（或正下方）收进去。
                Some((left, index)) => {
                    let lane_x = if left {
                        lane_left(index)
                    } else {
                        lane_right(index)
                    };
                    let below = to.y >= from.y;
                    // 源框在这一侧没有邻居时，直接从侧边出去，省掉一段会穿过
                    // 兄弟边的横向折返。菱形从斜边上出去（离顶点约 45% 处），
                    // 既不在角上，也不会往下折返穿过自己的分支。
                    let diamond_from = graph.nodes[edge.from].shape == Shape::Diamond;
                    let side_exit = diamond_from
                        || if left {
                            free_left[edge.from]
                        } else {
                            free_right[edge.from]
                        };
                    let mut points: Vec<(f32, f32)> = Vec::new();
                    // 出水平段的左端：长边标签贴在这一段上，别丢在长竖线中间。
                    let (leave, out_x) = if side_exit {
                        let (sx, sy) = if diamond_from {
                            // 菱形从斜边中段出线，高度贴近中线：这样横线一定在
                            // 它向下发散的那些兄弟线之上，不会交叉。
                            let hw = from.w / 2. + 22.;
                            let hh = NODE_H / 2. + 16.;
                            let cx = from.x + from.w / 2.;
                            let cy = from.y + NODE_H / 2.;
                            (
                                if left { cx - 0.8 * hw } else { cx + 0.8 * hw },
                                if below { cy + 0.2 * hh } else { cy - 0.2 * hh },
                            )
                        } else {
                            (
                                if left { from.x } else { from.x + from.w },
                                from.y + NODE_H / 2.,
                            )
                        };
                        points.push((sx, sy));
                        (sy, sx)
                    } else {
                        let exit_list = &exits[edge.from];
                        let start_x =
                            anchor_x(edge.from, spread(slot_of(exit_list, ix), exit_list.len()));
                        let anchor = if below {
                            shape_y(edge.from, start_x, false)
                        } else {
                            shape_y(edge.from, start_x, true)
                        };
                        let leave = if below { anchor + 22. } else { anchor - 22. };
                        points.push((start_x, anchor));
                        points.push((start_x, leave));
                        (leave, start_x)
                    };

                    if side_entry[ix] {
                        // 目标排在最外侧：从侧边收进去，路径最短、也不占上下缝隙。
                        let group = &side_group[&(edge.to, left)];
                        let slot = group.iter().position(|&x| x == ix).unwrap_or(0);
                        let n = group.len();
                        let (ex, cy) = if graph.nodes[edge.to].shape == Shape::Diamond {
                            // 回边是从下方绕回来的，落点放在下半斜边：竖线顶端
                            // 因此低于同一行从菱形下半斜边出去的正向长边，两条
                            // 线不再互相穿插；正向长边仍从上半斜边入。同侧多条
                            // 入线沿斜边依次往中心收，外侧通道收得更靠近中心。
                            let hw = to.w / 2. + 22.;
                            let hh = NODE_H / 2. + 16.;
                            let cx = to.x + to.w / 2.;
                            let base = (0.45 - 0.12 * slot as f32).max(0.15);
                            let offset = if below { -base * hh } else { base * hh };
                            (
                                if left {
                                    cx - hw * (1. - offset.abs() / hh)
                                } else {
                                    cx + hw * (1. - offset.abs() / hh)
                                },
                                to.y + NODE_H / 2. + offset,
                            )
                        } else {
                            // 目标在下时外侧收得更低、目标在上（回边）时外侧收得
                            // 更高——总之外侧的收尾横线要落在内侧竖线的终点之外。
                            let sign = if below { 1. } else { -1. };
                            let offset = sign * (slot as f32 - (n as f32 - 1.) / 2.) * 12.;
                            (
                                if left { to.x } else { to.x + to.w },
                                to.y + NODE_H / 2. + offset,
                            )
                        };
                        let head = if left {
                            [(ex - 9., cy - 5.), (ex - 9., cy + 5.), (ex, cy)]
                        } else {
                            [(ex + 9., cy - 5.), (ex + 9., cy + 5.), (ex, cy)]
                        };
                        points.push((lane_x, leave));
                        points.push((lane_x, cy));
                        points.push((ex, cy));
                        // 标签贴在「源 → 通道」这段水平线旁边：长竖线中段离两端
                        // 都很远，放在那里看不出标签属于哪个分支，还容易被别的
                        // 通道压住（发布流水线里两个「失败」就是这么飘在半空的）。
                        let label = edge
                            .label
                            .as_ref()
                            .map(|text| ((out_x + lane_x) / 2., leave - 10., text.clone()));
                        return EdgeLine {
                            path: EdgePath::Elbow {
                                points,
                                radius: 10.,
                            },
                            head,
                            label,
                        };
                    }

                    // 其余情况从目标的正上方（或正下方）收进去。
                    // 不再从侧边水平插进去：目标那一排可能还坐着别的节点，
                    // 水平段会穿过它，箭头也会叠在同一个点上。
                    let tip_y = if below {
                        shape_y(edge.to, tx, true)
                    } else {
                        shape_y(edge.to, tx, false)
                    };
                    // 收尾横线按通道错开：最外侧贴着目标收（离目标 18px），
                    // 越靠内退得越远。外侧横线因此落在内侧竖线的终点之外，
                    // 不会横穿竖线；同侧通道越多，退得越开。
                    let lanes = if left { left_lanes } else { right_lanes };
                    let offset = 18. + (lanes as f32 - 1. - index as f32) * 12.;
                    let join_y = if below {
                        tip_y - offset
                    } else {
                        tip_y + offset
                    };
                    let head = if below {
                        arrow_down(tx, tip_y)
                    } else {
                        arrow_up(tx, tip_y)
                    };
                    points.push((lane_x, leave));
                    points.push((lane_x, join_y));
                    points.push((tx, join_y));
                    points.push((tx, tip_y));
                    let label = edge
                        .label
                        .as_ref()
                        .map(|text| ((out_x + lane_x) / 2., leave - 10., text.clone()));
                    EdgeLine {
                        path: EdgePath::Elbow {
                            points,
                            radius: 10.,
                        },
                        head,
                        label,
                    }
                }
                // 相邻层：一条独立的 S 曲线，起点在自己那一份出口、终点在自己那一份入口。
                None => {
                    let exit_list = &exits[edge.from];
                    let sx = anchor_x(edge.from, spread(slot_of(exit_list, ix), exit_list.len()));
                    let sy = shape_y(edge.from, sx, false);
                    let ty = shape_y(edge.to, tx, true);
                    let dy = (ty - sy).max(1.);
                    let c1 = (sx, sy + dy * 0.45);
                    let c2 = (tx, ty - dy * 0.45);
                    // 标签压在线身上，并挑「连接少的那一端」放：
                    // 扇入多的目标（如「进入主界面」上方汇了五六条线）把标签放到曲线上游，
                    // 并按入边顺序在高度上错开，否则全挤在框顶那一行。
                    let in_deg = entries[edge.to].len();
                    let out_deg = exits[edge.from].len();
                    let label = edge.label.as_ref().map(|text| {
                        let t = if in_deg > out_deg {
                            // 在 [0.22, 0.64] 上按入边序号均匀铺开：相邻两条标签至少
                            // 隔十几像素高度，不会因为字宽而互相压住。
                            let n = in_deg.max(2) as f32;
                            let slot = slot_of(&entries[edge.to], ix) as f32;
                            0.22 + 0.42 * slot / (n - 1.)
                        } else {
                            0.72
                        };
                        let (x, y) = cubic_point((sx, sy), c1, c2, (tx, ty), t);
                        (x, y, text.clone())
                    });
                    EdgeLine {
                        path: EdgePath::Curve {
                            p0: (sx, sy),
                            c1,
                            c2,
                            p3: (tx, ty),
                        },
                        head: arrow_down(tx, ty),
                        label,
                    }
                }
            }
        })
        .collect();

    // 同一目标的多条标签再强制拉开 18px：曲线在目标附近纵向走得很慢，
    // 只按 t 均匀分布仍可能因为字宽压住，这里做最后一道保险。
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in graph.edges.iter().enumerate() {
        if edges[ix].label.is_some() {
            grouped[edge.to].push(ix);
        }
    }
    for list in &mut grouped {
        list.sort_by(|&a, &b| {
            let ya = edges[a].label.as_ref().map(|l| l.1).unwrap_or(0.);
            let yb = edges[b].label.as_ref().map(|l| l.1).unwrap_or(0.);
            ya.partial_cmp(&yb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut last = f32::MIN;
        for &ix in list.iter() {
            if let Some((x, y, text)) = edges[ix].label.clone() {
                let y = y.max(last + 18.);
                edges[ix].label = Some((x, y, text));
                last = y;
            }
        }
    }

    let mut diamonds = Vec::new();
    for (ix, node) in graph.nodes.iter().enumerate() {
        if node.shape == Shape::Diamond {
            let placed = nodes[ix];
            diamonds.push(Diamond {
                cx: placed.x + placed.w / 2.,
                cy: placed.y + NODE_H / 2.,
                hw: placed.w / 2. + 22.,
                hh: NODE_H / 2. + 16.,
            });
        }
    }

    Layout {
        width,
        height,
        nodes,
        edges,
        diamonds,
        ranks: rank,
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/// 把折线画成圆角：拐角处用二次曲线过渡，观感和 Mermaid 的曲线接近。
fn rounded_elbow<F>(
    builder: &mut PathBuilder,
    points: &[(f32, f32)],
    radius: f32,
    at: &F,
) where
    F: Fn(&(f32, f32)) -> Point<Pixels>,
{
    let Some(first) = points.first() else {
        return;
    };
    builder.move_to(at(first));
    for window in points.windows(3) {
        let (prev, corner, next) = (window[0], window[1], window[2]);
        let in_len = ((corner.0 - prev.0).powi(2) + (corner.1 - prev.1).powi(2)).sqrt();
        let out_len = ((next.0 - corner.0).powi(2) + (next.1 - corner.1).powi(2)).sqrt();
        let r = radius.min(in_len / 2.).min(out_len / 2.);
        if r <= 0.5 {
            builder.line_to(at(&corner));
            continue;
        }
        let start = (
            corner.0 + (prev.0 - corner.0) * r / in_len,
            corner.1 + (prev.1 - corner.1) * r / in_len,
        );
        let end = (
            corner.0 + (next.0 - corner.0) * r / out_len,
            corner.1 + (next.1 - corner.1) * r / out_len,
        );
        builder.line_to(at(&start));
        builder.curve_to(at(&end), at(&corner));
    }
    if let Some(last) = points.last() {
        builder.line_to(at(last));
    }
}

fn render_graph(graph: &Graph, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (border, muted, line) = (theme.border, theme.muted, theme.muted_foreground);
    let layout = layout(graph);

    let mut paths: Vec<EdgePath> = Vec::new();
    let mut heads: Vec<[(f32, f32); 3]> = Vec::new();
    let mut labels: Vec<(f32, f32, String)> = Vec::new();
    for edge in &layout.edges {
        paths.push(edge.path.clone());
        heads.push(edge.head);
        if let Some(label) = &edge.label {
            labels.push(label.clone());
        }
    }
    let shapes = layout.diamonds.clone();

    let paint = canvas(
        move |bounds, _, _| bounds.origin,
        move |_bounds, origin, window, _| {
            // 布局算出来的是画布内坐标，而 `paint_path` 收的是窗口坐标，
            // 必须加上画布自身的原点，否则所有线条都会跑到窗口左上角。
            let (ox, oy) = (origin.x, origin.y);
            let at = |p: &(f32, f32)| point(ox + px(p.0), oy + px(p.1));
            for edge_path in &paths {
                let mut builder = PathBuilder::stroke(px(1.2));
                match edge_path {
                    EdgePath::Curve { p0, c1, c2, p3 } => {
                        builder.move_to(at(p0));
                        builder.cubic_bezier_to(at(p3), at(c1), at(c2));
                    }
                    EdgePath::Elbow { points, radius } => {
                        rounded_elbow(&mut builder, points, *radius, &at);
                    }
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, line);
                }
            }
            for head in &heads {
                let mut builder = PathBuilder::fill();
                builder.move_to(at(&(head[0].0, head[0].1)));
                builder.line_to(at(&(head[1].0, head[1].1)));
                builder.line_to(at(&(head[2].0, head[2].1)));
                builder.close();
                if let Ok(path) = builder.build() {
                    window.paint_path(path, line);
                }
            }
            // 菱形画在边之后：连线从框下方穿过时会被填充盖住。
            for shape in &shapes {
                let points = [
                    (shape.cx, shape.cy - shape.hh),
                    (shape.cx + shape.hw, shape.cy),
                    (shape.cx, shape.cy + shape.hh),
                    (shape.cx - shape.hw, shape.cy),
                ];
                let mut fill = PathBuilder::fill();
                fill.move_to(at(&points[0]));
                for p in &points[1..] {
                    fill.line_to(at(p));
                }
                fill.close();
                if let Ok(path) = fill.build() {
                    window.paint_path(path, muted);
                }
                let mut stroke = PathBuilder::stroke(px(1.));
                stroke.move_to(at(&points[0]));
                for p in &points[1..] {
                    stroke.line_to(at(p));
                }
                stroke.close();
                if let Ok(path) = stroke.build() {
                    window.paint_path(path, border);
                }
            }
        },
    )
    .absolute()
    .inset_0();

    let mut container = div()
        .relative()
        .w(px(layout.width))
        .h(px(layout.height))
        .mx_auto()
        .child(paint);

    for (ix, node) in graph.nodes.iter().enumerate() {
        let placed = &layout.nodes[ix];
        if node.shape == Shape::Diamond {
            // 菱形由 canvas 画（见上面的 shapes），这里只放居中的文字。
            container = container.child(
                div()
                    .absolute()
                    .left(px(placed.x))
                    .top(px(placed.y))
                    .w(px(placed.w))
                    .h(px(NODE_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(FONT))
                    .child(div().text_center().px_1().child(node.label.clone())),
            );
            continue;
        }
        let radius = match node.shape {
            Shape::Round => px(18.),
            _ => px(6.),
        };
        container = container.child(
            div()
                .absolute()
                .left(px(placed.x))
                .top(px(placed.y))
                .w(px(placed.w))
                .h(px(NODE_H))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .rounded(radius)
                .border_1()
                .border_color(border)
                .bg(muted)
                .text_size(px(FONT))
                .child(div().text_center().px_2().child(node.label.clone())),
        );
    }

    for (x, y, text) in labels {
        let width = estimate_width(&text, 11.) + 10.;
        // 贴边的通道标签不能探出画布。
        let left = (x - width / 2.).clamp(0., (layout.width - width).max(0.));
        container = container.child(
            h_flex()
                .absolute()
                .left(px(left))
                .top(px(y - 8.))
                .px(px(3.))
                .rounded(px(4.))
                .bg(theme.background)
                .text_size(px(11.))
                .text_color(line)
                .child(text),
        );
    }

    container.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具里是使用手册中的 5 张图：插件必须能吃下的真实样本。
    const GUIDE: &str = include_str!("../tests/fixtures/diagrams.md");
    /// 展示页里的 3 张图（含三段回边的「发布流水线」），同样要做布局回归。
    const SHOWCASE: &str = include_str!("../tests/fixtures/showcase.md");

    fn mermaid_blocks(source: &str) -> Vec<String> {
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
            let left = layout.nodes.iter().map(|node| node.x).fold(f32::MAX, f32::min);
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
                    assert!(pair[1] - pair[0] > 1., "节点 {from} 的两条出线共用了同一个锚点");
                }
            }
            for (to, mut xs) in entries {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for pair in xs.windows(2) {
                    assert!(pair[1] - pair[0] > 1., "节点 {to} 的两条入线共用了同一个锚点");
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
                        a.0, b.0, a, b
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
        assert_no_crossings(SHOWCASE, "展示页");
    }

    #[test]
    fn edges_do_not_cross_nodes() {
        for (name, source) in [("使用手册夹具", GUIDE), ("展示页", SHOWCASE)] {
            for (bi, block) in mermaid_blocks(source).iter().enumerate() {
                let graph = parse_graph(block).expect("图应可解析");
                let layout = layout(&graph);
                let hits = node_hits(&graph, &layout);
                assert!(
                    hits.is_empty(),
                    "{name} 第 {bi} 张图有连线穿过节点:\n{}",
                    hits.join("\n")
                );
            }
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
