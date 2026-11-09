//! 节点-连线图的 GPUI 渲染：`flowchart` 与 `stateDiagram` 共用。
//!
//! 连线、箭头、菱形/圆形/横条符号用 `canvas` 画，节点文字与边标签用绝对定位
//! 的 `div` 画——文字排版与居中直接复用 GPUI 的布局能力。

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use super::common::rounded_elbow;
use super::layout::*;
use crate::estimate_width;

pub(super) fn render_node_link(graph: &Graph, cx: &App) -> AnyElement {
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

    // 状态图专用：起止圆、同步条由 canvas 画；复合状态外框按成员包围盒算。
    let mut symbols: Vec<(u8, f32, f32)> = Vec::new();
    for (ix, node) in graph.nodes.iter().enumerate() {
        let p = layout.nodes[ix];
        let kind = match node.shape {
            Shape::Terminal => 1,
            Shape::Bar => 2,
            _ => continue,
        };
        symbols.push((kind, p.x + p.w / 2., p.y + NODE_H / 2.));
    }
    let mut overlays: Vec<(f32, f32, f32, f32, String)> = Vec::new();
    for ov in &graph.overlays {
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        for &ix in &ov.members {
            let Some(p) = layout.nodes.get(ix) else {
                continue;
            };
            let quad = (p.x, p.y, p.x + p.w, p.y + NODE_H);
            bounds = Some(match bounds {
                Some((a, b, c, d)) => (a.min(quad.0), b.min(quad.1), c.max(quad.2), d.max(quad.3)),
                None => quad,
            });
        }
        if let Some((x1, y1, x2, y2)) = bounds {
            // 外框上下留出的空间要放得下标题文字，同时别贴住相邻层。
            overlays.push((x1 - 26., y1 - 34., x2 + 26., y2 + 26., ov.label.clone()));
        }
    }
    let notes: Vec<(f32, f32, String)> = graph
        .notes
        .iter()
        .map(|(ix, text)| {
            let p = layout.nodes[*ix];
            (p.x + p.w + 10., p.y, text.clone())
        })
        .collect();

    let paint_overlays = overlays.clone();

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
            // 复合状态外框画在连线之下、节点之上。
            for (x1, y1, x2, y2, _) in &paint_overlays {
                let mut b = PathBuilder::stroke(px(1.));
                b.move_to(at(&(*x1, *y1)));
                b.line_to(at(&(*x2, *y1)));
                b.line_to(at(&(*x2, *y2)));
                b.line_to(at(&(*x1, *y2)));
                b.close();
                if let Ok(path) = b.build() {
                    window.paint_path(path, border);
                }
            }
            // 起止圆（外环 + 实心内圆）与同步条。
            for (kind, cx, cy) in &symbols {
                if *kind == 1 {
                    let outer = circle_points(*cx, *cy, 11.);
                    let inner = circle_points(*cx, *cy, 6.);
                    let mut fill = PathBuilder::fill();
                    fill.move_to(at(&inner[0]));
                    for p in &inner[1..] {
                        fill.line_to(at(p));
                    }
                    fill.close();
                    if let Ok(path) = fill.build() {
                        window.paint_path(path, line);
                    }
                    let mut stroke = PathBuilder::stroke(px(1.2));
                    stroke.move_to(at(&outer[0]));
                    for p in &outer[1..] {
                        stroke.line_to(at(p));
                    }
                    stroke.close();
                    if let Ok(path) = stroke.build() {
                        window.paint_path(path, line);
                    }
                } else {
                    let corners = [
                        (*cx - 30., *cy - 4.),
                        (*cx + 30., *cy - 4.),
                        (*cx + 30., *cy + 4.),
                        (*cx - 30., *cy + 4.),
                    ];
                    let mut fill = PathBuilder::fill();
                    fill.move_to(at(&corners[0]));
                    for p in &corners[1..] {
                        fill.line_to(at(p));
                    }
                    fill.close();
                    if let Ok(path) = fill.build() {
                        window.paint_path(path, line);
                    }
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

    // 右侧注释与复合状态外框可能探出布局宽度，容器要跟着变宽变高。
    let mut canvas_width = layout.width;
    let mut canvas_height = layout.height;
    for (x, _, text) in &notes {
        canvas_width = canvas_width.max(*x + estimate_width(text, 11.) + 16.);
    }
    for (_, _, x2, y2, _) in &overlays {
        canvas_width = canvas_width.max(*x2 + PAD);
        canvas_height = canvas_height.max(*y2 + PAD);
    }

    let mut container = div()
        .relative()
        .w(px(canvas_width))
        .h(px(canvas_height))
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
        if matches!(node.shape, Shape::Terminal | Shape::Bar) {
            // 几何符号由 canvas 画；同步条把名字放在条下方。
            if node.shape == Shape::Bar && !node.label.is_empty() {
                container = container.child(
                    div()
                        .absolute()
                        .left(px(placed.x))
                        .top(px(placed.y + NODE_H))
                        .w(px(placed.w))
                        .text_size(px(10.))
                        .text_color(line)
                        .text_center()
                        .child(node.label.clone()),
                );
            }
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

    for (x1, y1, _, _, label) in &overlays {
        container = container.child(
            h_flex()
                .absolute()
                .left(px(*x1 + 6.))
                .top(px(*y1 + 2.))
                .px(px(3.))
                .bg(theme.background)
                .text_size(px(11.))
                .text_color(line)
                .child(label.clone()),
        );
    }
    for (x, y, text) in &notes {
        let w = estimate_width(text, 11.) + 12.;
        container = container.child(
            div()
                .absolute()
                .left(px(*x))
                .top(px(*y + 4.))
                .w(px(w))
                .h(px(NODE_H - 8.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(border)
                .bg(muted)
                .text_size(px(10.))
                .text_color(line)
                .child(div().text_center().child(text.clone())),
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

/// 16 边形近似圆：GPUI 的 `PathBuilder` 没有 arc 指令。
fn circle_points(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..16)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / 16.;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}
