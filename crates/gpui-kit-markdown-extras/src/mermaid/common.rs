//! 五类 Mermaid 图共用的绘制原语：圆角折线、虚线、箭头、文本清理与宽度估算。
//!
//! 这里不碰 `gpui-kit` 的主题类型：颜色由各渲染模块自己从 `cx.theme()` 取，
//! 免得公共层被某个具体颜色类型绑死。工具是否用到取决于 feature 组合，
//! 因此整个模块允许存在「本次编译没被用到」的辅助函数。
#![allow(dead_code)]

use gpui_kit::{PathBuilder, Pixels, Point};

use crate::estimate_width;

/// 估算一段文字在给定字号下的宽度，供各图统一定尺寸。
pub(super) fn text_width(text: &str, font_size: f32) -> f32 {
    estimate_width(text, font_size)
}

/// 清掉 Mermaid 标签里的引号与换行标记，让文本能直接当纯文本画。
pub(super) fn clean_label(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .replace("<br/>", " ")
        .replace("<br>", " ")
        .trim()
        .to_string()
}

/// 用短线段模拟虚线：`PathBuilder` 没有 dash 属性，时序图的生命线、
/// 类图的依赖/实现关系、甘特图的今日标记都靠它画。
pub(super) fn dashed_line<F>(b: &mut PathBuilder, a: (f32, f32), c: (f32, f32), at: &F)
where
    F: Fn(&(f32, f32)) -> Point<Pixels>,
{
    let len = ((c.0 - a.0).powi(2) + (c.1 - a.1).powi(2)).sqrt();
    if len < 1. {
        return;
    }
    let (ux, uy) = ((c.0 - a.0) / len, (c.1 - a.1) / len);
    let mut d = 0.;
    while d < len {
        let e = (d + 5.).min(len);
        b.move_to(at(&(a.0 + ux * d, a.1 + uy * d)));
        b.line_to(at(&(a.0 + ux * e, a.1 + uy * e)));
        d += 9.;
    }
}

/// 以 `(x, y)` 为尖端画一个箭头三角形；`dir`：0 右、1 下、2 左、3 上。
pub(super) fn arrow_at(x: f32, y: f32, dir: u8) -> [(f32, f32); 3] {
    let (dx, dy) = match dir {
        0 => (-10., 0.),
        1 => (0., -10.),
        2 => (10., 0.),
        _ => (0., 10.),
    };
    let (nx, ny) = (-dy / 2., dx / 2.);
    [
        (x + dx + nx, y + dy + ny),
        (x + dx - nx, y + dy - ny),
        (x, y),
    ]
}

/// 把折线画成圆角：拐角处用二次曲线过渡，观感和 Mermaid 的曲线接近。
pub(super) fn rounded_elbow<F>(
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
