//! 分层布局引擎：`flowchart` 与 `stateDiagram` 共用的「节点-连线」排版。
//!
//! 算法与原先单文件版本完全一致：最长路径分层（忽略 DFS 回边）→ 重心法减少
//! 交叉 → 坐标修正推开重叠 → 跨层长边走左右侧通道。这里只把数据结构抽成公共
//! 类型，让两类图能共用同一套布线，避免各自维护一份容易走样的布局器。
//!
//! 节点高度固定 [`NODE_H`]：状态图的复合状态以自身节点为锚点参与分层（内部
//! 成员经虚拟边钉到锚点下方），外框按成员包围盒描边，因此两类图能共用本引擎。

use std::collections::HashMap;

use crate::estimate_width;

pub(super) const NODE_H: f32 = 46.;
pub(super) const H_GAP: f32 = 90.;
/// 层间距的基数：每层之间至少留这么多高度，曲线与标签才不挤。
pub(super) const GAP_BASE: f32 = 104.;
pub(super) const PAD: f32 = 22.;
pub(super) const FONT: f32 = 13.;
/// 复合状态外框顶部预留：标题栏 [`OVERLAY_HEADER`] 再加上下呼吸。
pub(super) const OVERLAY_TOP: f32 = 34.;
/// 复合状态标题栏高度。渲染层画分隔线与标题文字时也用它，改这里两处一起动。
pub(super) const OVERLAY_HEADER: f32 = 26.;
/// 复合状态外框底部到最后一行成员的留白。
pub(super) const OVERLAY_BOTTOM: f32 = 26.;
/// 复合状态第一层内部成员距「锚点行顶」的偏移。
///
/// 锚点自己是不画框的（外框就是它），所以内部第一层不必先走完一整行节点高度：
/// 从锚点行顶往下让出标题栏加一点呼吸即可，否则标题栏下面会空出一大块。
const OVERLAY_FIRST_INSET: f32 = 16.;
/// 侧通道宽度：跨层长边与回边从这里绕开中间的行。
pub(super) const LANE_W: f32 = 42.;
/// 节点区与第一条侧通道之间再留一段空白，长线才会明显走在图的两侧。
pub(super) const LANE_MARGIN: f32 = 38.;
pub(super) const MIN_W: f32 = 152.;
pub(super) const MAX_W: f32 = 300.;
/// 连线样式：`-->` 实线、`-.->` 虚线、`==>` 粗线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EdgeStyle {
    Solid,
    Dotted,
    Thick,
}

/// 连线端点的符号：无（`---`）、三角箭头（`-->`）、圆圈（`--o`）、叉（`--x`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EdgeEnd {
    Bare,
    Arrow,
    Circle,
    Cross,
}

fn unit(v: (f32, f32)) -> (f32, f32) {
    let len = (v.0 * v.0 + v.1 * v.1).sqrt().max(1e-4);
    (v.0 / len, v.1 / len)
}

fn sub(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

/// 沿来向 `dir` 收进 `end` 的三角箭头：尖角落在 `end`，底边退后 10px。
/// 竖直来向下正好复刻原先的 arrow_down / arrow_up，斜向与水平来向也能正确定向。
fn head_triangle(end: (f32, f32), dir: (f32, f32)) -> [(f32, f32); 3] {
    let (dx, dy) = unit(dir);
    let (bx, by) = (end.0 - dx * 10., end.1 - dy * 10.);
    [(bx - dy * 5., by + dx * 5.), (bx + dy * 5., by - dx * 5.), end]
}

/// 路径的起止点与两端行进方向：`(start, dir_start, end, dir_end)`。
/// 方向用于给 `--o` / `--x` / `<-->` 的端点符号定向。
fn path_endpoints(path: &EdgePath) -> ((f32, f32), (f32, f32), (f32, f32), (f32, f32)) {
    match path {
        EdgePath::Curve { p0, c1, c2, p3 } => (*p0, unit(sub(*c1, *p0)), *p3, unit(sub(*p3, *c2))),
        EdgePath::Elbow { points, .. } => {
            let n = points.len();
            if n < 2 {
                let p = points.first().copied().unwrap_or((0., 0.));
                return (p, (0., 1.), p, (0., 1.));
            }
            (points[0], unit(sub(points[1], points[0])), points[n - 1], unit(sub(points[n - 1], points[n - 2])))
        }
    }
}

/// 按端点符号补齐箭头三角与圆圈/叉记号，收尾所有 EdgeLine 的构造。
fn finish_line(
    path: EdgePath,
    style: EdgeStyle,
    left: EdgeEnd,
    right: EdgeEnd,
    label: Option<(f32, f32, String)>,
) -> EdgeLine {
    let (start, d0, end, d1) = path_endpoints(&path);
    let head = (right == EdgeEnd::Arrow).then(|| head_triangle(end, d1));
    // `<-->`：左端也有三角，尖角指向源节点。
    let left_head = (left == EdgeEnd::Arrow).then(|| head_triangle(start, unit(sub((0., 0.), d0))));
    let marks = [
        (left == EdgeEnd::Circle || left == EdgeEnd::Cross).then_some((start.0, start.1, left)),
        (right == EdgeEnd::Circle || right == EdgeEnd::Cross).then_some((end.0, end.1, right)),
    ]
    .into_iter()
    .flatten()
    .collect();
    EdgeLine {
        path,
        head,
        left_head,
        marks,
        label,
        style,
    }
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Shape {
    Rect,
    Round,
    Diamond,
    /// `([药丸])`：全圆角胶囊。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Stadium,
    /// `((圆))`：正圆。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Circle,
    /// `(((双圆)))`：同心双圈。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    DoubleCircle,
    /// `[(圆柱)]`：数据库。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Database,
    /// `[[子程序]]`：左右双竖线。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Subroutine,
    /// `{{六边形}}`。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Hexagon,
    /// `[/平行四边形/]`：上边右移。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Para,
    /// `[\\反平行四边形\\]`：上边左移。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    ParaAlt,
    /// `[/梯形\\]`：上窄下宽。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Trap,
    /// `[\\反梯形/]`：上宽下窄。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    TrapAlt,
    /// `>旗形]`：右侧尖角。
    #[cfg_attr(not(feature = "mermaid-flowchart"), allow(dead_code))]
    Flag,
    /// 状态图 `[*]`：实心圆 + 外环。
    #[cfg_attr(not(feature = "mermaid-state"), allow(dead_code))]
    Terminal,
    /// 状态图 `<<fork>>` / `<<join>>`：粗横条。
    #[cfg_attr(not(feature = "mermaid-state"), allow(dead_code))]
    Bar,
}

/// 外框：复合状态（`anchor` = 框自身的锚点节点）与流程图子图（`anchor` = None，
/// 纯视觉分组）共用。成员包围盒外扩一圈描边。
#[derive(Clone)]
pub(super) struct Overlay {
    pub(super) label: String,
    /// 复合状态锚点；子图没有锚点。
    pub(super) anchor: Option<usize>,
    pub(super) members: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct GraphNode {
    pub(super) label: String,
    pub(super) shape: Shape,
}

#[derive(Clone)]
pub(super) struct GraphEdge {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) label: Option<String>,
    /// 指向已排在前面的层：画成右侧回边通道。
    pub(super) back: bool,
    /// 实线 / 虚线 / 粗线（`-.->` / `==>`）。
    pub(super) style: EdgeStyle,
    /// 两端的端点符号（`--o` / `--x` / `<-->`）。
    pub(super) left: EdgeEnd,
    pub(super) right: EdgeEnd,
    /// 端点连在子图外框上时记录框的序号；此时 from/to 是占位的 0。
    pub(super) from_sg: Option<usize>,
    pub(super) to_sg: Option<usize>,
}

impl GraphEdge {
    /// 子图框边：端点连在子图外框上，不参与分层，节点落位后单独锚到框边。
    pub(super) fn is_frame(&self) -> bool {
        self.from_sg.is_some() || self.to_sg.is_some()
    }
}

#[derive(Clone)]
pub(super) struct Graph {
    pub(super) nodes: Vec<GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
    /// 复合状态外框（状态图）与子图外框（flowchart）。
    pub(super) overlays: Vec<Overlay>,
    /// 复合状态的虚拟约束边 `(锚点, 内部根节点)`：只参与分层与列对齐，不绘制。
    pub(super) pin_edges: Vec<(usize, usize)>,
    /// 依附在某个节点右侧的说明文字：`(节点, 文本)`。
    pub(super) notes: Vec<(usize, String)>,
}

// ---------------------------------------------------------------------------
// 回边识别
// ---------------------------------------------------------------------------

/// DFS 标出回边：指向「当前还在递归栈里」的节点的边。
/// 子图框边（端点是外框不是节点）不参与。
pub(super) fn mark_back_edges(edges: &mut [GraphEdge], count: usize) {
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in edges.iter().enumerate() {
        if edge.is_frame() {
            continue;
        }
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
// 布局结果
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) struct Placed {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
}

#[derive(Clone)]
pub(super) struct EdgeLine {
    pub(super) path: EdgePath,
    /// 右端是三角箭头时才有值（`---` / `--o` 等没有）。
    pub(super) head: Option<[(f32, f32); 3]>,
    /// 左端是三角箭头时才有值（`<-->`）。
    pub(super) left_head: Option<[(f32, f32); 3]>,
    /// 两端的圆圈 / 叉记号。
    pub(super) marks: Vec<(f32, f32, EdgeEnd)>,
    pub(super) label: Option<(f32, f32, String)>,
    pub(super) style: EdgeStyle,
}

#[derive(Clone, Debug)]
pub(super) enum EdgePath {
    /// 相邻层：三次贝塞尔，从源框底部平滑弯到目标框顶部。
    Curve {
        p0: (f32, f32),
        c1: (f32, f32),
        c2: (f32, f32),
        p3: (f32, f32),
    },
    /// 长边 / 回边：沿侧通道走的折线，拐角做圆角。
    Elbow {
        points: Vec<(f32, f32)>,
        radius: f32,
    },
}

impl EdgePath {
    #[cfg(all(test, feature = "mermaid-flowchart"))]
    pub(super) fn points(&self) -> Vec<(f32, f32)> {
        match self {
            EdgePath::Curve { p0, c1, c2, p3 } => vec![*p0, *c1, *c2, *p3],
            EdgePath::Elbow { points, .. } => points.clone(),
        }
    }
}

pub(super) struct Layout {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) nodes: Vec<Placed>,
    pub(super) edges: Vec<EdgeLine>,
    /// 判断框画成菱形，尺寸比文字框大一圈，单独记录给 canvas 画。
    pub(super) diamonds: Vec<Diamond>,
    /// 每个节点所在的层，供测试判断一条边是否跨层。
    #[allow(dead_code)]
    pub(super) ranks: Vec<usize>,
    /// 复合状态外框矩形 `(x1, y1, x2, y2, 标题)`：渲染层直接描边，
    /// 布线的贴边锚点（进出锚点的线贴框顶/框底）也依赖它。
    pub(super) overlays: Vec<(f32, f32, f32, f32, String)>,
}

#[derive(Clone, Copy)]
pub(super) struct Diamond {
    pub(super) cx: f32,
    pub(super) cy: f32,
    pub(super) hw: f32,
    pub(super) hh: f32,
}

// ---------------------------------------------------------------------------
// 布局算法
// ---------------------------------------------------------------------------

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
                // 虚拟边同样参与对齐：复合状态的内部链跟着锚点走。
                for &(from, to) in graph.pin_edges.iter() {
                    if downward && to == node {
                        keys.push(index_of[from]);
                    } else if !downward && from == node {
                        keys.push(index_of[to]);
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
                for &(from, to) in graph.pin_edges.iter() {
                    let linked = if downward { to == node } else { from == node };
                    if linked {
                        let other = if downward { from } else { to };
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
pub(super) fn cubic_point(
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
        let x =
            mt * mt * mt * sx + 3. * mt * mt * t * c1.0 + 3. * mt * t * t * c2.0 + t * t * t * tx;
        let y =
            mt * mt * mt * sy + 3. * mt * mt * t * c1.1 + 3. * mt * t * t * c2.1 + t * t * t * ty;
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
pub(super) fn layout(graph: &Graph) -> Layout {
    let count = graph.nodes.len();

    // 锚点 → 复合状态的桥：进出锚点的连线要贴外框顶/底边。子图没有锚点。
    let mut anchor_ov: Vec<Option<usize>> = vec![None; count];
    for (oi, ov) in graph.overlays.iter().enumerate() {
        if let Some(anchor) = ov.anchor {
            anchor_ov[anchor] = Some(oi);
        }
    }

    // 分层：只走非回边，最长路径。回边已经排除，所以不会死循环。
    // 虚拟边（锚点 → 内部根节点）同样参与：内部链被钉到锚点下一层。
    let relax = |rank: &mut [usize], from: usize, to: usize, len: usize, changed: &mut bool| {
        if rank[to] < rank[from] + len {
            rank[to] = rank[from] + len;
            *changed = true;
        }
    };
    let mut rank = vec![0usize; count];
    for _ in 0..count {
        let mut changed = false;
        for edge in graph.edges.iter().filter(|edge| !edge.back && !edge.is_frame()) {
            relax(&mut rank, edge.from, edge.to, 1, &mut changed);
        }
        for &(from, to) in graph.pin_edges.iter() {
            relax(&mut rank, from, to, 1, &mut changed);
        }
        if !changed {
            break;
        }
    }

    // 复合状态在纵向占 span+1 层：对外出边必须落到整个外框之下的层，
    // 否则线会从框中间穿出去（旧实现的 Running --> check 正是这样）。
    let mut anchor_outs: Vec<(usize, usize, usize)> = Vec::new();
    for ov in &graph.overlays {
        let Some(anchor) = ov.anchor else { continue; };
        let span = ov
            .members
            .iter()
            .filter(|&&m| m != anchor)
            .map(|&m| rank[m].saturating_sub(rank[anchor]))
            .max()
            .unwrap_or(0);
        for edge in graph
            .edges
            .iter()
            .filter(|edge| !edge.back && edge.from == anchor)
        {
            if !ov.members.contains(&edge.to) {
                anchor_outs.push((anchor, edge.to, span + 1));
            }
        }
    }
    // 子图框边把整组成员和外部节点钉在相邻层：节点 → 子图时所有成员都在
    // 来源节点之下，子图 → 节点时目标节点在所有成员之下，子图 → 子图时
    // 目标整组在来源整组之下。这样外框不会罩到外部节头上，两框也不会叠行。
    for edge in graph.edges.iter().filter(|edge| edge.is_frame()) {
        match (edge.from_sg, edge.to_sg) {
            (None, Some(sg)) => {
                for &m in &graph.overlays[sg].members {
                    anchor_outs.push((edge.from, m, 1));
                }
            }
            (Some(sg), None) => {
                for &m in &graph.overlays[sg].members {
                    anchor_outs.push((m, edge.to, 1));
                }
            }
            (Some(from_sg), Some(to_sg)) => {
                let src = &graph.overlays[from_sg].members;
                let dst = &graph.overlays[to_sg].members;
                for &s in src {
                    for &m in dst {
                        anchor_outs.push((s, m, 1));
                    }
                }
            }
            (None, None) => {}
        }
    }
    for _ in 0..count {
        let mut changed = false;
        for edge in graph.edges.iter().filter(|edge| !edge.back && !edge.is_frame()) {
            relax(&mut rank, edge.from, edge.to, 1, &mut changed);
        }
        for &(from, to) in graph.pin_edges.iter() {
            relax(&mut rank, from, to, 1, &mut changed);
        }
        for &(from, to, len) in anchor_outs.iter() {
            relax(&mut rank, from, to, len, &mut changed);
        }
        if !changed {
            break;
        }
    }
    let depth = rank.iter().copied().max().unwrap_or(0) + 1;

    // 向上或同层的边（除 DFS 回边外，还有「循环里跳回上层」这种回退边）同样要走侧通道：
    // 直着连会往回画成一条被压扁的曲线，而且会穿过中间的行。
    // 子图框边不进任何布线机械——它不连节点，节点落位后单独贴框画。
    let escaping: Vec<bool> = graph
        .edges
        .iter()
        .map(|edge| edge.back || edge.is_frame() || rank[edge.to] <= rank[edge.from])
        .collect();

    let widths: Vec<f32> = graph
        .nodes
        .iter()
        .map(|node| {
            // 圆 / 药丸按文字收窄，别让一个两字的圆占满 152px。
            let (min, max) = match node.shape {
                Shape::Circle | Shape::DoubleCircle => (96., 190.),
                Shape::Stadium => (110., MAX_W),
                _ => (MIN_W, MAX_W),
            };
            (estimate_width(&node.label, FONT) + 30.).clamp(min, max)
        })
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
        let in_deg = graph
            .edges
            .iter()
            .filter(|e| e.to == node && !e.back && !e.is_frame())
            .count();
        let outs: Vec<f32> = graph
            .edges
            .iter()
            .filter(|edge| edge.from == node && !edge.back && !edge.is_frame())
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

    // ---- 纵向行位 ----
    // 默认每行「节点高 + 层间距」。唯一的例外是复合状态的第一层内部成员：
    // 锚点自己占了一行却不画框，若仍按整行间距往下排，标题栏与第一个内部
    // 节点之间会空出 150px 上下（状态图里「起点离框顶太远」就是它）。
    // 这一行改成从锚点行顶只让出标题栏 + 呼吸，外框也随之内收。
    let mut below_anchor = vec![false; depth];
    for ov in &graph.overlays {
        let Some(anchor) = ov.anchor else { continue; };
        let row = rank[anchor] + 1;
        if row < depth && ov.members.iter().any(|&m| m != anchor && rank[m] == row) {
            below_anchor[row] = true;
        }
    }
    let mut row_y = vec![PAD; depth];
    for row in 1..depth {
        row_y[row] = if below_anchor[row] {
            row_y[row - 1] + OVERLAY_FIRST_INSET
        } else {
            row_y[row - 1] + NODE_H + GAP_BASE
        };
    }

    // 整张图按实际占宽收边；行内不做二次居中，否则单节点的层会被硬拉到画面中央，
    // 反而离开它前驱的正下方（「填注册码」就是这种情况）。
    let provisional: Vec<Placed> = (0..count)
        .map(|node| Placed {
            x: pos[node],
            y: row_y[rank[node]],
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
            // 子图框边不绕通道：它没有节点端，落位后单独贴框画。
            if edge.is_frame() {
                return false;
            }
            // 复合状态的出边从外框底边出发（见 shape_y 的锚点分支），
            // 不会擦到框内成员，按锚点自身的位置做穿框检测只会误判。
            if anchor_ov[edge.from].is_some() {
                return escaping[ix];
            }
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
        let left_score = free_left[edge.to] as i32 * 2 + free_left[edge.from] as i32;
        let right_score = free_right[edge.to] as i32 * 2 + free_right[edge.from] as i32;
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
    let lane_right = |index: usize| node_x0 + max_row + LANE_MARGIN + LANE_W * (index as f32 + 0.5);
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
        let y = row_y[row];
        for &node in list {
            nodes[node] = Placed {
                x: node_x0 + pos[node],
                y,
                w: widths[node],
            };
        }
    }
    let height = row_y[depth - 1] + NODE_H + PAD;

    // 外框：成员包围盒外扩一圈，顶部留标题的空间；锚点位于最顶层时不要再
    // 探出画布。空成员（空 subgraph）没有框，占位 None，保持与 graph.overlays
    // 的下标对齐，框边才能按序号查到自己的框。
    let mut ov_rects: Vec<Option<(f32, f32, f32, f32, String)>> =
        graph.overlays.iter().map(|_| None).collect();
    for (oi, ov) in graph.overlays.iter().enumerate() {
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        for &ix in &ov.members {
            let p = nodes[ix];
            let quad = (p.x, p.y, p.x + p.w, p.y + NODE_H);
            bounds = Some(match bounds {
                Some((a, b, c, d)) => (a.min(quad.0), b.min(quad.1), c.max(quad.2), d.max(quad.3)),
                None => quad,
            });
        }
        if let Some((x1, y1, x2, y2)) = bounds {
            ov_rects[oi] = Some((
                x1 - 26.,
                (y1 - OVERLAY_TOP).max(2.),
                x2 + 26.,
                y2 + OVERLAY_BOTTOM,
                ov.label.clone(),
            ));
        }
    }
    let overlays: Vec<(f32, f32, f32, f32, String)> =
        ov_rects.iter().flatten().cloned().collect();

    let center = |ix: usize| nodes[ix].x + nodes[ix].w / 2.;
    // 排序键取「目标 / 源端」的中心：框边按其外框的中心参与排序，
    // 占位端 0 的中心没有意义。
    let end_center_x = |ix: usize, from_side: bool| -> f32 {
        let edge = &graph.edges[ix];
        let (sg, node) = if from_side {
            (edge.from_sg, edge.from)
        } else {
            (edge.to_sg, edge.to)
        };
        if let Some(sg) = sg
            && let Some((x1, _, x2, _, _)) = ov_rects[sg]
        {
            return (x1 + x2) / 2.;
        }
        center(node)
    };
    let by_target = |a: &usize, b: &usize| {
        end_center_x(*a, false)
            .partial_cmp(&end_center_x(*b, false))
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let by_source = |a: &usize, b: &usize| {
        end_center_x(*a, true)
            .partial_cmp(&end_center_x(*b, true))
            .unwrap_or(std::cmp::Ordering::Equal)
    };

    // 同一个源的多条出边在框底均匀分开，各自独立成线。
    // 侧通道的边也参与分配出口，否则它会和普通边共用同一个点，箭头叠在一起。
    // 框边只有真实节点的那一端参与分摊（占位端 0 不占任何节点的进出点）。
    let mut exits: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut entries: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in graph.edges.iter().enumerate() {
        if edge.is_frame() {
            if edge.from_sg.is_none() {
                exits[edge.from].push(ix);
            }
            if edge.to_sg.is_none() {
                entries[edge.to].push(ix);
            }
            continue;
        }
        exits[edge.from].push(ix);
        entries[edge.to].push(ix);
    }
    for list in &mut exits {
        list.sort_by(by_target);
    }

    // 同一个目标的入边按来源从左到右排，入点依次错开。
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
    // 复合状态的锚点不画自身的框：进出它的线直接贴外框顶/底边。
    let shape_y = |node: usize, x: f32, top: bool| -> f32 {
        if let Some(oi) = anchor_ov[node] {
            return if top { overlays[oi].1 } else { overlays[oi].3 };
        }
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

    // ---- 子图框边：节点落位后贴外框画 ----
    // 框端朝向「另一端在哪侧就贴哪条边」，同一框同一条边上的多条框边
    // 按另一端位置排序后沿框边均分；节点端按朝向贴上/下边（横向时贴侧边
    // 中点），与普通边共用进出点分摊。两端锚点定好后连一条主轴 0.45 的
    // S 曲线，跟相邻层的普通曲线一个画法。
    let center_of =
        |node: usize| (nodes[node].x + nodes[node].w / 2., nodes[node].y + NODE_H / 2.);
    let end_center = |ix: usize, is_from: bool| -> Option<(f32, f32)> {
        let edge = &graph.edges[ix];
        let (sg, node) = if is_from {
            (edge.from_sg, edge.from)
        } else {
            (edge.to_sg, edge.to)
        };
        match sg {
            Some(sg) => ov_rects[sg]
                .as_ref()
                .map(|(x1, y1, x2, y2, _)| ((x1 + x2) / 2., (y1 + y2) / 2.)),
            None => Some(center_of(node)),
        }
    };
    // 面向另一端的那条边：0=顶 1=底 2=左 3=右。框外按归一化张角选顶底或
    // 左右；另一端在框内（成员指向自己所在子图）时贴最近的一条边。
    let facing = |rect: (f32, f32, f32, f32), other: (f32, f32)| -> u8 {
        let (x1, y1, x2, y2) = rect;
        let cx = (x1 + x2) / 2.;
        let cy = (y1 + y2) / 2.;
        if other.0 > x1 && other.0 < x2 && other.1 > y1 && other.1 < y2 {
            let dists = [other.1 - y1, y2 - other.1, other.0 - x1, x2 - other.0];
            return dists
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i as u8)
                .unwrap_or(0);
        }
        let (dx, dy) = (other.0 - cx, other.1 - cy);
        if dy.abs() * (x2 - x1) >= dx.abs() * (y2 - y1) {
            if dy < 0. { 0 } else { 1 }
        } else if dx < 0. {
            2
        } else {
            3
        }
    };
    let mut node_pts: HashMap<(usize, bool), (f32, f32)> = HashMap::new();
    let mut groups: HashMap<(usize, u8), Vec<(usize, bool)>> = HashMap::new();
    let mut frame_list: Vec<usize> = Vec::new();
    for (ix, edge) in graph.edges.iter().enumerate() {
        if !edge.is_frame() {
            continue;
        }
        frame_list.push(ix);
        for is_from in [true, false] {
            let (sg, node) = if is_from {
                (edge.from_sg, edge.from)
            } else {
                (edge.to_sg, edge.to)
            };
            if let Some(sg) = sg {
                if let (Some(rect), Some(other)) = (
                    ov_rects[sg].as_ref().map(|(x1, y1, x2, y2, _)| (*x1, *y1, *x2, *y2)),
                    end_center(ix, !is_from),
                ) {
                    let side = facing(rect, other);
                    groups.entry((sg, side)).or_default().push((ix, is_from));
                }
            } else {
                let side = end_center(ix, !is_from).map(|other| {
                    let placed = &nodes[node];
                    let (nx, ny) = center_of(node);
                    let (dx, dy) = (other.0 - nx, other.1 - ny);
                    if dy.abs() * placed.w >= dx.abs() * NODE_H {
                        if dy < 0. { 0 } else { 1 }
                    } else if dx < 0. {
                        2
                    } else {
                        3
                    }
                });
                // 上/下边走进出点分摊，与普通边不叠在同一个点上。
                let pt = side.map(|side| match side {
                    2 => (nodes[node].x, nodes[node].y + NODE_H / 2.),
                    3 => (nodes[node].x + nodes[node].w, nodes[node].y + NODE_H / 2.),
                    _ => {
                        let list = if is_from { &exits[node] } else { &entries[node] };
                        let x = anchor_x(node, spread(slot_of(list, ix), list.len()));
                        (x, shape_y(node, x, side == 0))
                    }
                });
                if let Some(pt) = pt {
                    node_pts.insert((ix, is_from), pt);
                }
            }
        }
    }
    let mut frame_pts: HashMap<(usize, bool), (f32, f32)> = HashMap::new();
    for ((sg, side), mut list) in groups {
        let Some((x1, y1, x2, y2, _)) = ov_rects[sg] else {
            continue;
        };
        // 组内按另一端沿框边方向的位置排序：靠左来的收在框边靠左的位置，
        // 线与线不会在框边前互相交叉。
        list.sort_by(|a, b| {
            let key = |end: &(usize, bool)| {
                end_center(end.0, !end.1)
                    .map(|c| if side >= 2 { c.1 } else { c.0 })
                    .unwrap_or(0.)
            };
            key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let n = list.len() as f32;
        for (slot, &(ix, is_from)) in list.iter().enumerate() {
            let f = (slot as f32 + 1.) / (n + 1.);
            let pt = match side {
                0 => (x1 + 24. + f * (x2 - x1 - 48.), y1),
                1 => (x1 + 24. + f * (x2 - x1 - 48.), y2),
                2 => (x1, y1 + 16. + f * (y2 - y1 - 32.)),
                _ => (x2, y1 + 16. + f * (y2 - y1 - 32.)),
            };
            frame_pts.insert((ix, is_from), pt);
        }
    }
    let mut frame_lines: HashMap<usize, EdgeLine> = HashMap::new();
    for &ix in &frame_list {
        let edge = &graph.edges[ix];
        let mut anchors: [(f32, f32); 2] = [(0., 0.); 2];
        let mut ok = true;
        for (k, is_from) in [(0usize, true), (1usize, false)] {
            let sg = if is_from {
                edge.from_sg
            } else {
                edge.to_sg
            };
            let pt = if sg.is_some() {
                frame_pts.get(&(ix, is_from)).copied()
            } else {
                node_pts.get(&(ix, is_from)).copied()
            };
            match pt {
                Some(pt) => anchors[k] = pt,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            // 空子图没有框可贴：从节点端垂一小段占位，保持边的序号对齐。
            let (_, node) = edge
                .from_sg
                .is_none()
                .then_some((true, edge.from))
                .unwrap_or((false, edge.to));
            let p = center_of(node);
            let stub = finish_line(
                EdgePath::Curve {
                    p0: p,
                    c1: (p.0, p.1 + 3.),
                    c2: (p.0, p.1 + 6.),
                    p3: (p.0, p.1 + 9.),
                },
                edge.style,
                edge.left,
                edge.right,
                None,
            );
            frame_lines.insert(ix, stub);
            continue;
        }
        let (s, t) = (anchors[0], anchors[1]);
        let (dx, dy) = (t.0 - s.0, t.1 - s.1);
        let label = edge
            .label
            .as_ref()
            .map(|text| ((s.0 + t.0) / 2., (s.1 + t.1) / 2. - 6., text.clone()));
        // 竖向主导照常上下弯，横向主导左右弯。
        let (c1, c2) = if dy.abs() >= dx.abs() {
            ((s.0, s.1 + dy * 0.45), (t.0, t.1 - dy * 0.45))
        } else {
            ((s.0 + dx * 0.45, s.1), (t.0 - dx * 0.45, t.1))
        };
        frame_lines.insert(
            ix,
            finish_line(
                EdgePath::Curve { p0: s, c1, c2, p3: t },
                edge.style,
                edge.left,
                edge.right,
                label,
            ),
        );
    }

    let mut edges: Vec<EdgeLine> = graph
        .edges
        .iter()
        .enumerate()
        .map(|(ix, edge)| {
            // 子图框边：贴外框单独画（见 frame_lines），不走普通布线。
            if let Some(line) = frame_lines.get(&ix) {
                return line.clone();
            }
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
                        return finish_line(
                            EdgePath::Elbow {
                                points,
                                radius: 10.,
                            },
                            edge.style,
                            edge.left,
                            edge.right,
                            label,
                        );
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
                    points.push((lane_x, leave));
                    points.push((lane_x, join_y));
                    points.push((tx, join_y));
                    points.push((tx, tip_y));
                    let label = edge
                        .label
                        .as_ref()
                        .map(|text| ((out_x + lane_x) / 2., leave - 10., text.clone()));
                    finish_line(
                        EdgePath::Elbow {
                            points,
                            radius: 10.,
                        },
                        edge.style,
                        edge.left,
                        edge.right,
                        label,
                    )
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
                    finish_line(
                        EdgePath::Curve {
                            p0: (sx, sy),
                            c1,
                            c2,
                            p3: (tx, ty),
                        },
                        edge.style,
                        edge.left,
                        edge.right,
                        label,
                    )
                }
            }
        })
        .collect();

    // 同一目标的多条标签再强制拉开 18px：曲线在目标附近纵向走得很慢，
    // 只按 t 均匀分布仍可能因为字宽压住，这里做最后一道保险。
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (ix, edge) in graph.edges.iter().enumerate() {
        if edges[ix].label.is_some() && !edge.is_frame() {
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
        overlays,
    }
}
