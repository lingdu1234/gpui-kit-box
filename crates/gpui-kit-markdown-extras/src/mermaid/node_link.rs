//! 节点-连线图的 GPUI 渲染：`flowchart` 与 `stateDiagram` 共用几何、分两套视觉语言。
//!
//! 两类图用的是同一份布局（`layout.rs`），但呈现方式略有不同：
//!
//! - [`NodeLinkStyle::Flowchart`]：淡底圆角框 + 实心三角箭头，尽量融进正文；
//! - [`NodeLinkStyle::State`]：空心圆角框 + 开口 V 箭头，对齐 Mermaid 官方的
//!   状态图观感——起止圆是强调色圆点 / 圆环，选择是空心菱形，同步条是强调色
//!   粗条，复合状态画成带标题栏的圆角框，注释带虚线引线。
//!
//! 配色不在这里拼，统一来自 [`super::palette`]：两种风格只决定「填不填底色、
//! 有没有标题栏、箭头实心还是开口」，线条与文字色始终是同一套色阶。
//!
//! 连线、箭头、符号用 `canvas` 画，节点文字与边标签用绝对定位的 `div` 画——
//! 文字排版与居中直接复用 GPUI 的布局能力。

use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, App, Hsla, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point,
};

use super::common::{rounded_elbow, rounded_rect};
use super::layout::*;
use super::palette::{self, DiagramPalette};
use crate::estimate_width;

/// 两套视觉语言：几何相同、配色同源，只在「填充 / 标题栏 / 箭头样式」上分叉。
#[derive(Clone, Copy, PartialEq)]
pub(super) enum NodeLinkStyle {
    Flowchart,
    State,
}

/// 复合状态标题栏高度。取布局层的常量，保证「外框预留」和「标题栏实高」
/// 不会各写一份数：布局按 [`OVERLAY_TOP`] 留顶、内部第一层按 [`OVERLAY_HEADER`]
/// 定位，这里只负责把标题文字与分隔线放进同一条带子里。
const HEADER_H: f32 = OVERLAY_HEADER;
/// 注释框高度：比状态框矮一圈，读起来像「旁注」而不是又一个状态。
const NOTE_H: f32 = NODE_H - 8.;

/// 两种风格在共用配色上的差异：状态图是空心框（填页底色）且带标题栏，
/// 流程图是淡表面填充、没有标题栏。箭头形状在绘制时直接按 `state` 分支。
#[derive(Clone, Copy)]
struct Style {
    node_fill: Hsla,
    frame_fill: Option<Hsla>,
    header_fill: Option<Hsla>,
    title: Hsla,
}

impl Style {
    fn new(state: bool, pal: &DiagramPalette) -> Self {
        if state {
            Self {
                node_fill: pal.background,
                frame_fill: Some(pal.frame_fill),
                header_fill: Some(pal.header_fill),
                title: pal.text,
            }
        } else {
            Self {
                node_fill: pal.surface,
                frame_fill: None,
                header_fill: None,
                title: pal.text_muted,
            }
        }
    }
}

/// 注释框：位置、文本，以及连回所属状态的那条虚线贝塞尔。
#[derive(Clone)]
struct Note {
    x: f32,
    y: f32,
    text: String,
    link: NoteLink,
}

#[derive(Clone, Copy)]
struct NoteLink {
    from: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    to: (f32, f32),
}

/// 画在 canvas 上的几何符号。
enum Symbol {
    /// 起点 `[*]`：实心圆点。
    Start(f32, f32),
    /// 终点 `[*]`：外环 + 实心内点。
    End(f32, f32),
    /// `<<fork>>` / `<<join>>` 的同步条（没有名字时才画）。
    Bar(f32, f32),
    /// `<<choice>>`：空心菱形。
    Diamond(f32, f32, f32, f32),
    /// 流程图的异形节点：圆、双圆、六边形、圆柱、子程序、斜面四边形、旗形。
    /// `(shape, x0, y0, w, h)`。
    Odd(Shape, f32, f32, f32, f32),
}

/// 形如 `fork1` / `paid_check` / `check` 的小写标识符：只是给图用的内部名字，
/// 画到图上是噪音（Mermaid 也不显示），所以同步条与菱形都不带字。
fn is_machine_id(label: &str) -> bool {
    let mut chars = label.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 复合状态里成员的显示名：`Running::Booting` 只留 `Booting`——外框标题已经
/// 说明了它属于谁，重复前缀既占宽又难看。
fn member_label(label: &str) -> &str {
    label.rsplit("::").next().unwrap_or(label)
}

/// 图的自然尺寸（未缩放）：布局尺寸 + 右侧注释 / 外框可能探出的部分。
/// 供等比适配包装在布局期计算缩放系数。
pub(super) fn node_link_size(graph: &Graph) -> (f32, f32) {
    let lay = layout(graph);
    let notes = place_notes(graph, &lay);
    canvas_size(&lay, &notes)
}

/// 注释落位：默认贴节点右侧，那一格被别的节点占了就退到整张图右边。
fn place_notes(graph: &Graph, layout: &Layout) -> Vec<Note> {
    // 图的右边界：注释默认贴在节点右侧，那一格被别的节点占了就退到整张图右边，
    // 否则两个框会叠在一起（订单状态机里 Paid 右边的 Failed 就是这种情况）。
    let graph_right = layout
        .nodes
        .iter()
        .map(|p| p.x + p.w)
        .chain(layout.overlays.iter().map(|(_, _, x2, _, _)| *x2))
        .fold(0., f32::max);
    graph
        .notes
        .iter()
        .map(|(ix, text)| {
            let p = layout.nodes[*ix];
            let w = estimate_width(text, 11.) + 20.;
            let mut x = p.x + p.w + 16.;
            let mut y = p.y;
            let clash = layout.nodes.iter().enumerate().any(|(j, q)| {
                j != *ix && q.x < x + w && q.x + q.w > x && q.y < p.y + NODE_H && q.y + NODE_H > p.y
            });
            let link = if clash {
                // 右边那一格已经被别的状态占了：退到整张图右边、并下移半行，
                // 引线从锚点右下角兜下去，才不会看起来像连着隔壁那个状态。
                x = graph_right + 24.;
                y = p.y + NODE_H + 22.;
                let to = (x, y + NOTE_H / 2.);
                NoteLink {
                    from: (p.x + p.w, p.y + NODE_H),
                    c1: (p.x + p.w + 34., p.y + NODE_H + 26.),
                    c2: (to.0 - 44., to.1),
                    to,
                }
            } else {
                let to = (x, y + NOTE_H / 2.);
                NoteLink {
                    from: (p.x + p.w, p.y + NODE_H / 2.),
                    c1: (p.x + p.w + 6., to.1),
                    c2: (to.0 - 8., to.1),
                    to,
                }
            };
            Note {
                x,
                y,
                text: text.clone(),
                link,
            }
        })
        .collect()
}

/// 自然尺寸：右侧注释与复合状态外框可能探出布局宽高，容器要跟着变宽变高。
fn canvas_size(layout: &Layout, notes: &[Note]) -> (f32, f32) {
    let mut width = layout.width;
    let mut height = layout.height;
    for note in notes {
        width = width.max(note.x + estimate_width(&note.text, 11.) + 24.);
        height = height.max(note.y + NOTE_H);
    }
    for (_, _, x2, y2, _) in &layout.overlays {
        width = width.max(*x2 + PAD);
        height = height.max(*y2 + PAD);
    }
    (width, height)
}

pub(super) fn render_node_link(
    graph: &Graph,
    style: NodeLinkStyle,
    scale: f32,
    cx: &App,
) -> AnyElement {
    // 等比缩放：函数内所有 px 值（坐标、宽高、字号、线宽、圆角）同乘 scale。
    // 布局坐标本身不动，canvas 的 `at` 映射与文字 div 都经过这个 px，
    // 几何天然相似，不变形。scale=1 时与未缩放输出逐像素一致。
    let px = move |v: f32| gpui_kit::px(v * scale);
    let state = style == NodeLinkStyle::State;
    let pal = palette::of(cx);
    let look = Style::new(state, &pal);
    let layout = layout(graph);

    // `~~~` 隐形连线照常参与布局，但这里不画：按序号过滤掉 invisible 的边。
    let visible = |ix: usize| !graph.edges[ix].invisible;
    let paths: Vec<(EdgePath, EdgeStyle)> = layout
        .edges
        .iter()
        .enumerate()
        .filter(|(ix, _)| visible(*ix))
        .map(|(_, e)| (e.path.clone(), e.style))
        .collect();
    let heads: Vec<Option<[(f32, f32); 3]>> = layout
        .edges
        .iter()
        .enumerate()
        .filter(|(ix, _)| visible(*ix))
        .map(|(_, e)| e.head)
        .collect();
    let left_heads: Vec<Option<[(f32, f32); 3]>> = layout
        .edges
        .iter()
        .enumerate()
        .filter(|(ix, _)| visible(*ix))
        .map(|(_, e)| e.left_head)
        .collect();
    let marks: Vec<(f32, f32, EdgeEnd)> = layout
        .edges
        .iter()
        .enumerate()
        .filter(|(ix, _)| visible(*ix))
        .flat_map(|(_, e)| e.marks.iter().copied())
        .collect();
    let labels: Vec<(f32, f32, String)> = layout
        .edges
        .iter()
        .enumerate()
        .filter(|(ix, _)| visible(*ix))
        .filter_map(|(_, e)| e.label.clone())
        .collect();

    // 起点 / 终点都画成 `[*]`：靠出入度区分，起点只有出边、终点只有入边。
    let mut has_in = vec![false; graph.nodes.len()];
    let mut has_out = vec![false; graph.nodes.len()];
    for edge in &graph.edges {
        has_out[edge.from] = true;
        has_in[edge.to] = true;
    }

    let mut symbols: Vec<Symbol> = Vec::new();
    for (ix, node) in graph.nodes.iter().enumerate() {
        let p = layout.nodes[ix];
        let (cx, cy) = (p.x + p.w / 2., p.y + NODE_H / 2.);
        match node.shape {
            Shape::Terminal => {
                if has_out[ix] && !has_in[ix] {
                    symbols.push(Symbol::Start(cx, cy));
                } else {
                    symbols.push(Symbol::End(cx, cy));
                }
            }
            // 有名字的 fork/join 当普通状态画（见下面节点框的分支），只有
            // 名字是内部标识符时才退化成同步条。
            Shape::Bar if is_machine_id(&node.label) => symbols.push(Symbol::Bar(cx, cy)),
            Shape::Diamond => {}
            // 方框、圆角框、药丸由 div 画，其余异形交给 canvas。
            Shape::Rect | Shape::Round | Shape::Stadium => {}
            shape => symbols.push(Symbol::Odd(shape, p.x, p.y, p.w, NODE_H)),
        }
    }
    // 菱形尺寸由布局层算（比文字框大一圈），直接搬过来。
    for d in &layout.diamonds {
        symbols.push(Symbol::Diamond(d.cx, d.cy, d.hw, d.hh));
    }

    // 复合状态外框矩形在布局层算好（布线的贴边锚点也依赖它），这里直接用。
    let overlays = layout.overlays.clone();
    let notes = place_notes(graph, &layout);

    let paint_overlays = overlays.clone();
    let paint_notes = notes.clone();

    let paint = canvas(
        move |bounds, _, _| bounds.origin,
        move |_bounds, origin, window, _| {
            // 布局算出来的是画布内坐标，而 `paint_path` 收的是窗口坐标，
            // 必须加上画布自身的原点，否则所有线条都会跑到窗口左上角。
            let (ox, oy) = (origin.x, origin.y);
            let at = |p: &(f32, f32)| point(ox + px(p.0), oy + px(p.1));

            for (edge_path, style) in &paths {
                match style {
                    EdgeStyle::Dotted => {
                        // GPUI 的 PathBuilder 没有虚线，把路径采样后切成小段。
                        for (a, b) in dash_segments(&sample_path(edge_path), 5., 4.) {
                            let mut builder = PathBuilder::stroke(px(1.2));
                            builder.move_to(at(&a));
                            builder.line_to(at(&b));
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, pal.line);
                            }
                        }
                    }
                    _ => {
                        let mut builder = PathBuilder::stroke(if *style == EdgeStyle::Thick {
                            px(2.4)
                        } else {
                            px(1.2)
                        });
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
                            window.paint_path(path, pal.line);
                        }
                    }
                }
            }

            // 箭头：状态图用开口的 V（贴近 Mermaid），流程图保留实心三角。
            // `---` / `--o` / `--x` 这类没有三角箭头的端点不画。
            if state {
                let mut barbs = PathBuilder::stroke(px(1.2));
                for head in heads.iter().flatten().chain(left_heads.iter().flatten()) {
                    let tip = head[2];
                    for corner in [head[0], head[1]] {
                        // 三角顶点缩到 72%，把实心箭头的两条斜边留成 V 形箭头。
                        let end = (
                            tip.0 + (corner.0 - tip.0) * 0.72,
                            tip.1 + (corner.1 - tip.1) * 0.72,
                        );
                        barbs.move_to(at(&tip));
                        barbs.line_to(at(&end));
                    }
                }
                if let Ok(path) = barbs.build() {
                    window.paint_path(path, pal.line);
                }
            } else {
                for head in heads.iter().flatten().chain(left_heads.iter().flatten()) {
                    let mut builder = PathBuilder::fill();
                    builder.move_to(at(&(head[0].0, head[0].1)));
                    builder.line_to(at(&(head[1].0, head[1].1)));
                    builder.line_to(at(&(head[2].0, head[2].1)));
                    builder.close();
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, pal.line);
                    }
                }
            }

            // `--o` / `--x` 端点记号：圆圈先用页底色盖住线身再描边。
            for (mx, my, mark) in &marks {
                match mark {
                    EdgeEnd::Circle => {
                        let mut fill = PathBuilder::fill();
                        let ring = circle_points(*mx, *my, 3.5);
                        fill.move_to(at(&ring[0]));
                        for p in &ring[1..] {
                            fill.line_to(at(p));
                        }
                        fill.close();
                        if let Ok(path) = fill.build() {
                            window.paint_path(path, pal.background);
                        }
                        let mut stroke = PathBuilder::stroke(px(1.2));
                        stroke.move_to(at(&ring[0]));
                        for p in &ring[1..] {
                            stroke.line_to(at(p));
                        }
                        stroke.close();
                        if let Ok(path) = stroke.build() {
                            window.paint_path(path, pal.line);
                        }
                    }
                    EdgeEnd::Cross => {
                        for (dx, dy) in [(-3.2, -3.2), (-3.2, 3.2)] {
                            let mut builder = PathBuilder::stroke(px(1.2));
                            builder.move_to(at(&(*mx + dx, *my + dy)));
                            builder.line_to(at(&(*mx - dx, *my - dy)));
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, pal.line);
                            }
                        }
                    }
                    _ => {}
                }
            }

            // 注释引线：虚线贝塞尔，先于外框与节点画（被节点盖住的部分自然消失）。
            for note in &paint_notes {
                let link = note.link;
                let mut b = PathBuilder::stroke(px(1.));
                b.move_to(at(&link.from));
                b.cubic_bezier_to(at(&link.to), at(&link.c1), at(&link.c2));
                if let Ok(path) = b.build() {
                    window.paint_path(path, pal.note_outline);
                }
            }

            // 复合状态外框画在连线之下、节点之上：底色 → 标题栏 → 分隔线 → 描边。
            for (x1, y1, x2, y2, _) in &paint_overlays {
                let (w, h) = (x2 - x1, y2 - y1);
                if let Some(fill) = look.frame_fill {
                    let mut b = PathBuilder::fill();
                    rounded_rect(&mut b, &at, *x1, *y1, w, h, [8.; 4]);
                    if let Ok(path) = b.build() {
                        window.paint_path(path, fill);
                    }
                }
                if let Some(fill) = look.header_fill {
                    let mut b = PathBuilder::fill();
                    rounded_rect(&mut b, &at, *x1, *y1, w, HEADER_H, [8., 8., 0., 0.]);
                    if let Ok(path) = b.build() {
                        window.paint_path(path, fill);
                    }
                    let mut line = PathBuilder::stroke(px(1.));
                    line.move_to(at(&(*x1, y1 + HEADER_H)));
                    line.line_to(at(&(*x2, y1 + HEADER_H)));
                    if let Ok(path) = line.build() {
                        window.paint_path(path, pal.frame_outline);
                    }
                }
                let mut b = PathBuilder::stroke(px(1.2));
                rounded_rect(&mut b, &at, *x1, *y1, w, h, [8.; 4]);
                if let Ok(path) = b.build() {
                    window.paint_path(path, pal.frame_outline);
                }
            }

            // 几何符号：起止圆、同步条、选择菱形。
            for symbol in &symbols {
                match symbol {
                    Symbol::Start(cx, cy) => {
                        let mut fill = PathBuilder::fill();
                        let points = circle_points(*cx, *cy, 6.);
                        fill.move_to(at(&points[0]));
                        for p in &points[1..] {
                            fill.line_to(at(p));
                        }
                        fill.close();
                        if let Ok(path) = fill.build() {
                            window.paint_path(path, pal.outline);
                        }
                    }
                    Symbol::End(cx, cy) => {
                        let mut ring = PathBuilder::stroke(px(1.6));
                        let outer = circle_points(*cx, *cy, 8.5);
                        ring.move_to(at(&outer[0]));
                        for p in &outer[1..] {
                            ring.line_to(at(p));
                        }
                        ring.close();
                        if let Ok(path) = ring.build() {
                            window.paint_path(path, pal.outline);
                        }
                        let mut dot = PathBuilder::fill();
                        let inner = circle_points(*cx, *cy, 3.8);
                        dot.move_to(at(&inner[0]));
                        for p in &inner[1..] {
                            dot.line_to(at(p));
                        }
                        dot.close();
                        if let Ok(path) = dot.build() {
                            window.paint_path(path, pal.outline);
                        }
                    }
                    Symbol::Bar(cx, cy) => {
                        let mut b = PathBuilder::fill();
                        rounded_rect(&mut b, &at, cx - 30., cy - 4.5, 60., 9., [4.5; 4]);
                        if let Ok(path) = b.build() {
                            window.paint_path(path, pal.outline);
                        }
                    }
                    Symbol::Diamond(cx, cy, hw, hh) => {
                        let points = [
                            (*cx, cy - hh),
                            (cx + hw, *cy),
                            (*cx, cy + hh),
                            (cx - hw, *cy),
                        ];
                        // 空心：先用页底色盖住穿过的连线，再描边。
                        let mut fill = PathBuilder::fill();
                        fill.move_to(at(&points[0]));
                        for p in &points[1..] {
                            fill.line_to(at(p));
                        }
                        fill.close();
                        if let Ok(path) = fill.build() {
                            window.paint_path(path, look.node_fill);
                        }
                        let mut stroke = PathBuilder::stroke(px(1.4));
                        stroke.move_to(at(&points[0]));
                        for p in &points[1..] {
                            stroke.line_to(at(p));
                        }
                        stroke.close();
                        if let Ok(path) = stroke.build() {
                            window.paint_path(path, pal.frame_outline);
                        }
                    }
                    Symbol::Odd(shape, x0, y0, w, h) => {
                        let (x0, y0, w, h) = (*x0, *y0, *w, *h);
                        let x1 = x0 + w;
                        let y1 = y0 + h;
                        let cx = x0 + w / 2.;
                        let cy = y0 + h / 2.;
                        let s = 14.;
                        let (mut fills, mut strokes): (
                            Vec<Vec<(f32, f32)>>,
                            Vec<Vec<(f32, f32)>>,
                        ) = (Vec::new(), Vec::new());
                        match shape {
                            Shape::Circle => {
                                strokes.push(ellipse_points(cx, cy, h / 2. - 1., h / 2. - 1., false));
                            }
                            Shape::DoubleCircle => {
                                strokes.push(ellipse_points(cx, cy, h / 2. - 1., h / 2. - 1., false));
                                strokes.push(ellipse_points(cx, cy, h / 2. - 7., h / 2. - 7., false));
                            }
                            Shape::Hexagon => {
                                let poly = vec![
                                    (x0, cy),
                                    (x0 + 12., y0),
                                    (x1 - 12., y0),
                                    (x1, cy),
                                    (x1 - 12., y1),
                                    (x0 + 12., y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            Shape::Database => {
                                let e = 7.;
                                let rx = w / 2.;
                                // 主体：矩形身 + 底部外凸弧，一起填充盖住穿过线。
                                let mut body = vec![(x0, y0 + e), (x1, y0 + e), (x1, y1 - e)];
                                body.extend(ellipse_points(cx, y1 - e, rx, e, true));
                                fills.push(body);
                                strokes.push(ellipse_points(cx, y0 + e, rx, e, false));
                                strokes.push(vec![(x0, y0 + e), (x0, y1 - e)]);
                                strokes.push(vec![(x1, y0 + e), (x1, y1 - e)]);
                            }
                            Shape::Subroutine => {
                                let rect = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
                                fills.push(rect.clone());
                                strokes.push(rect);
                                strokes.push(vec![(x0 + 7., y0), (x0 + 7., y1)]);
                                strokes.push(vec![(x1 - 7., y0), (x1 - 7., y1)]);
                            }
                            Shape::Para => {
                                let poly = vec![
                                    (x0 + s, y0),
                                    (x1, y0),
                                    (x1 - s, y1),
                                    (x0, y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            Shape::ParaAlt => {
                                let poly = vec![
                                    (x0, y0),
                                    (x1 - s, y0),
                                    (x1, y1),
                                    (x0 + s, y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            Shape::Trap => {
                                let poly = vec![
                                    (x0 + s, y0),
                                    (x1 - s, y0),
                                    (x1, y1),
                                    (x0, y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            Shape::TrapAlt => {
                                let poly = vec![
                                    (x0, y0),
                                    (x1, y0),
                                    (x1 - s, y1),
                                    (x0 + s, y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            Shape::Flag => {
                                let poly = vec![
                                    (x0, y0),
                                    (x1 - s, y0),
                                    (x1, cy),
                                    (x1 - s, y1),
                                    (x0, y1),
                                ];
                                fills.push(poly.clone());
                                strokes.push(poly);
                            }
                            _ => {}
                        }
                        // 先填充盖住从框下穿过的连线，再描边。
                        for poly in &fills {
                            let mut fill = PathBuilder::fill();
                            fill.move_to(at(&poly[0]));
                            for p in &poly[1..] {
                                fill.line_to(at(p));
                            }
                            fill.close();
                            if let Ok(path) = fill.build() {
                                window.paint_path(path, look.node_fill);
                            }
                        }
                        for poly in &strokes {
                            let mut stroke = PathBuilder::stroke(px(1.4));
                            stroke.move_to(at(&poly[0]));
                            for p in &poly[1..] {
                                stroke.line_to(at(p));
                            }
                            stroke.close();
                            if let Ok(path) = stroke.build() {
                                window.paint_path(path, pal.outline);
                            }
                        }
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0();

    let (canvas_width, canvas_height) = canvas_size(&layout, &notes);

    let mut container = div()
        .relative()
        .w(px(canvas_width))
        .h(px(canvas_height))
        .mx_auto()
        .child(paint);

    // 复合状态的锚点不画自身的方框：外框就是复合状态，标题画在标题栏里。
    // 子图（flowchart）没有锚点，成员方框照常画。
    let mut anchors = vec![false; graph.nodes.len()];
    for ov in &graph.overlays {
        if let Some(anchor) = ov.anchor {
            anchors[anchor] = true;
        }
    }
    for (ix, node) in graph.nodes.iter().enumerate() {
        if anchors[ix] {
            continue;
        }
        let placed = &layout.nodes[ix];
        let label = member_label(&node.label);
        if node.shape == Shape::Diamond {
            // 菱形由 canvas 画（见上面的 symbols），内部标识符不写字。
            if is_machine_id(&node.label) {
                continue;
            }
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
                    .child(div().text_center().px(px(4.)).child(label.to_string())),
            );
            continue;
        }
        if node.shape == Shape::Terminal {
            continue; // 起止圆由 canvas 画，不带文字
        }
        if node.shape == Shape::Bar && is_machine_id(&node.label) {
            continue; // 同步条由 canvas 画，名字只在有语义时才有意义
        }
        if matches!(
            node.shape,
            Shape::Circle
                | Shape::DoubleCircle
                | Shape::Hexagon
                | Shape::Database
                | Shape::Subroutine
                | Shape::Para
                | Shape::ParaAlt
                | Shape::Trap
                | Shape::TrapAlt
                | Shape::Flag
        ) {
            // 字形由 canvas 画（见上面的 Odd 符号），这里只放居中的文字。
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
                    .child(div().text_center().px(px(4.)).child(label.to_string())),
            );
            continue;
        }
        let radius = if state {
            px(6.)
        } else if node.shape == Shape::Round {
            px(18.)
        } else if node.shape == Shape::Stadium {
            px(NODE_H / 2.)
        } else {
            px(6.)
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
                .border(px(1.))
                .border_color(pal.outline)
                .bg(look.node_fill)
                .text_size(px(FONT))
                .text_color(pal.text)
                .child(div().text_center().px(px(8.)).child(label.to_string())),
        );
    }

    for (x1, y1, _, _, label) in &overlays {
        container = container.child(
            h_flex()
                .absolute()
                .left(px(*x1 + 10.))
                .top(px(if look.header_fill.is_some() {
                    *y1 + (HEADER_H - 16.) / 2.
                } else {
                    *y1 + 2.
                }))
                .text_size(px(11.))
                .text_color(look.title)
                .child(label.clone()),
        );
    }
    for note in &notes {
        let w = estimate_width(&note.text, 11.) + 20.;
        container = container.child(
            div()
                .absolute()
                .left(px(note.x))
                .top(px(note.y))
                .w(px(w))
                .h(px(NOTE_H))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border(px(1.))
                .border_color(pal.note_outline)
                .bg(pal.note_fill)
                .text_size(px(11.))
                .text_color(pal.text_muted)
                .child(div().text_center().child(note.text.clone())),
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
                .bg(pal.background)
                .text_size(px(11.))
                .text_color(pal.text_muted)
                .child(text),
        );
    }

    container.into_any_element()
}

/// 16 边形近似圆：GPUI 的 `PathBuilder` 没有 arc 指令。
fn circle_points(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..16)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / 16.;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// 16 等分椭圆；`half` 为 true 时只取下半弧（圆柱的底部）。
fn ellipse_points(cx: f32, cy: f32, rx: f32, ry: f32, half: bool) -> Vec<(f32, f32)> {
    let (n, end) = if half {
        (8, std::f32::consts::PI)
    } else {
        (16, std::f32::consts::TAU)
    };
    (0..=n)
        .map(|i| {
            let a = end * i as f32 / n as f32;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// 采样连线为折线：曲线取 24 个点，折线保持折点。
fn sample_path(path: &EdgePath) -> Vec<(f32, f32)> {
    match path {
        EdgePath::Curve { p0, c1, c2, p3 } => (0..=24)
            .map(|i| cubic_point(*p0, *c1, *c2, *p3, i as f32 / 24.))
            .collect(),
        EdgePath::Elbow { points, .. } => points.clone(),
    }
}

/// 沿折线切出「画 dash、空 gap」的小段：GPUI 的 PathBuilder 不支持虚线，
/// 只能把路径走一遍、按剩余长度交替画与不画。
fn dash_segments(points: &[(f32, f32)], dash: f32, gap: f32) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let Some(&first) = points.first() else {
        return out;
    };
    let mut prev = first;
    let mut drawing = true;
    let mut left = dash;
    for &p in &points[1..] {
        loop {
            let dx = p.0 - prev.0;
            let dy = p.1 - prev.1;
            let remaining = (dx * dx + dy * dy).sqrt();
            if remaining <= 1e-4 {
                break;
            }
            let step = left.min(remaining);
            let next = (
                prev.0 + dx / remaining * step,
                prev.1 + dy / remaining * step,
            );
            if drawing {
                out.push((prev, next));
            }
            prev = next;
            left -= step;
            if left <= 1e-4 {
                drawing = !drawing;
                left = if drawing { dash } else { gap };
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_ids_are_not_drawn_as_text() {
        for id in ["fork1", "join", "check", "paid_check"] {
            assert!(is_machine_id(id), "{id} 应视作内部标识符");
        }
        for name in ["Fork", "Join", "Check", "等待指令", "Build"] {
            assert!(!is_machine_id(name), "{name} 应视作可显示的名字");
        }
    }

    #[test]
    fn composite_members_drop_their_prefix() {
        assert_eq!(member_label("Running::Booting"), "Booting");
        assert_eq!(member_label("Paying::Waiting"), "Waiting");
        assert_eq!(member_label("Idle"), "Idle");
    }
}
