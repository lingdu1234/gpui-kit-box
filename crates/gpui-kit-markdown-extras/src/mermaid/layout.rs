//! 分层布局引擎：`flowchart` 与 `stateDiagram` 共用的「节点-连线」排版。
//!
//! 算法与原先单文件版本完全一致：最长路径分层（忽略 DFS 回边）→ 重心法减少
//! 交叉 → 坐标修正推开重叠 → 跨层长边走左右侧通道。这里只把数据结构抽成公共
//! 类型，让两类图能共用同一套布线，避免各自维护一份容易走样的布局器。
//!
//! 节点高度固定 [`NODE_H`]：复合状态这类「大框」由渲染层按成员包围盒额外描边，
//! 不参与布局，因此状态图也能直接复用本引擎。

use std::collections::HashMap;

use crate::estimate_width;

pub(super) const NODE_H: f32 = 46.;
pub(super) const H_GAP: f32 = 90.;
/// 层间距的基数：每层之间至少留这么多高度，曲线与标签才不挤。
pub(super) const GAP_BASE: f32 = 104.;
pub(super) const PAD: f32 = 22.;
pub(super) const FONT: f32 = 13.;
/// 侧通道宽度：跨层长边与回边从这里绕开中间的行。
pub(super) const LANE_W: f32 = 42.;
/// 节点区与第一条侧通道之间再留一段空白，长线才会明显走在图的两侧。
pub(super) const LANE_MARGIN: f32 = 38.;
pub(super) const MIN_W: f32 = 152.;
pub(super) const MAX_W: f32 = 300.;
/// 指向目标框底部的箭头（回退边从下方收进去）。
fn arrow_up(tx: f32, ty: f32) -> [(f32, f32); 3] {
    [(tx - 5., ty + 10.), (tx + 5., ty + 10.), (tx, ty)]
}

/// 指向目标框顶部的箭头（正向边与长边的收尾一致）。
fn arrow_down(tx: f32, ty: f32) -> [(f32, f32); 3] {
    [(tx - 5., ty - 10.), (tx + 5., ty - 10.), (tx, ty)]
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Shape {
    Rect,
    Round,
    Diamond,
    /// 状态图 `[*]`：实心圆 + 外环。
    #[cfg_attr(not(feature = "mermaid-state"), allow(dead_code))]
    Terminal,
    /// 状态图 `<<fork>>` / `<<join>>`：粗横条。
    #[cfg_attr(not(feature = "mermaid-state"), allow(dead_code))]
    Bar,
}

/// 复合状态的外框：按成员节点的包围盒在渲染层描边，不参与分层布局。
#[derive(Clone)]
pub(super) struct Overlay {
    pub(super) label: String,
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
}

#[derive(Clone)]
pub(super) struct Graph {
    pub(super) nodes: Vec<GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
    /// 复合状态外框（flowchart 恒为空）。
    pub(super) overlays: Vec<Overlay>,
    /// 依附在某个节点右侧的说明文字：`(节点, 文本)`。
    pub(super) notes: Vec<(usize, String)>,
}

// ---------------------------------------------------------------------------
// 回边识别
// ---------------------------------------------------------------------------

/// DFS 标出回边：指向「当前还在递归栈里」的节点的边。
pub(super) fn mark_back_edges(edges: &mut [GraphEdge], count: usize) {
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
// 布局结果
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) struct Placed {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
}

pub(super) struct EdgeLine {
    pub(super) path: EdgePath,
    pub(super) head: [(f32, f32); 3],
    pub(super) label: Option<(f32, f32, String)>,
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
        let in_deg = graph
            .edges
            .iter()
            .filter(|e| e.to == node && !e.back)
            .count();
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
