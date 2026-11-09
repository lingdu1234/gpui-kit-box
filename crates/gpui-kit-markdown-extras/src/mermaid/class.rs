//! `classDiagram` 类图：解析、类框布局与 GPUI 绘制。
//!
//! 支持类成员、注解、关系与多重度：
//!
//! ```text
//! classDiagram
//!     direction TB
//!     class Animal {
//!         +int age
//!         -String name
//!         +void eat()
//!     }
//!     <<interface>> Animal
//!     Animal <|-- Duck
//!     Animal *-- Heart : 组合
//!     Duck "1" --> "*" Pond : 关联
//!     Duck ..> Food : 依赖
//!     Duck ..|> Swim
//!     note for Duck "水禽"
//! ```
//!
//! 类框高度随成员数变化，无法套用 `layout.rs` 的固定行高分层器，因此本模块
//! 自带一套轻量布局：关系按「左操作数在上、右操作数在下」分层（与 Mermaid
//! 一致，`<|--` 这类记号只决定符号画在哪一端，不改变层次），同层顺序排开，
//! 跨层关系在层间空白带里走折线并主动避开其它类框。
//!
//! 类框按 Mermaid 的三栏结构画：表头（注解 + 类名）/ 属性 / 方法，两道分隔线。
//! `note for X` 不参与分层：TB/BT 贴在宿主类上方，LR/RL 贴在宿主类的左侧/右侧，
//! 用短虚线连过去；候选位置会主动避开其它类框。
//!
//! 配色不复用 `theme.border` / `theme.muted_foreground`：这两个 token 本身就是
//! 「弱化」色阶（浅色 neutral-200/500、深色 neutral-800/400），在深色背景上
//! 几乎看不见。类图统一用 `theme.info`（cyan-500/400）画边框、分隔线、关系线、
//! 端点符号与注记，文字用 `theme.foreground`，与 Mermaid.js 默认观感一致。

use std::collections::HashMap;

use gpui_kit::component::theme::ThemeColor;
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, Hsla, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use super::common::{clean_label, dashed_line, rounded_elbow, text_width};

const PAD: f32 = 20.;
const TITLE_H: f32 = 28.;
const ANNO_H: f32 = 16.;
const LINE_H: f32 = 18.;
const EMPTY_CELL_H: f32 = 14.;
const MIN_W: f32 = 116.;
const H_GAP: f32 = 52.;
const V_GAP: f32 = 74.;
const NOTE_H: f32 = 30.;
const NOTE_GAP: f32 = 14.;
/// 首层上方留白：既放注记框，也放「层上方」这条备用布线带。
const TOP_EXTRA: f32 = 60.;
const BOTTOM_EXTRA: f32 = 44.;

/// 可见度参数：旧版 `border_1()` + 1px 分隔线 + 1.2px 关系线在深色主题下
/// 基本消失。这里统一加粗，并让所有线条共用同一个强调色。
const DIVIDER_H: f32 = 1.5;
const EDGE_W: f32 = 1.8;
const MARKER_W: f32 = 1.5;
const NOTE_FRAME_W: f32 = 1.5;
const NOTE_LINK_W: f32 = 1.6;

/// 类图专用配色。抽成纯函数便于在明/暗两套主题下做回归测试，
/// 也避免渲染层到处散落 `theme.border` 之类的弱对比 token。
#[derive(Clone, Copy)]
struct Palette {
    /// 类框边框、两道分隔线、关系线、端点符号、注记框与注记连线。
    stroke: Hsla,
    /// 类框与注记框底色。
    fill: Hsla,
    /// 类名、成员、注解、标签、多重度。
    text: Hsla,
    /// 标签遮挡底色、空心端点符号内部填充。
    chip: Hsla,
}

fn palette(theme: &ThemeColor) -> Palette {
    // 自定义主题可能把 info 调回和 border 一样淡；退回前景色保证始终有对比。
    let stroke = if theme.info == theme.border {
        theme.foreground
    } else {
        theme.info
    };
    Palette {
        stroke,
        fill: theme.muted,
        text: theme.foreground,
        chip: theme.background,
    }
}

/// 取矩形朝向另一矩形中心的那条边的中点，作为注记虚线的端点。
/// 这样注记在左/右/上/下都能自动选对连接边，不用按方向写四套坐标。
fn link_anchor(from: Rect, to: Rect) -> (f32, f32) {
    let (fcx, fcy) = (from.x + from.w / 2., from.y + from.h / 2.);
    let (tcx, tcy) = (to.x + to.w / 2., to.y + to.h / 2.);
    let (dx, dy) = (tcx - fcx, tcy - fcy);
    if dx.abs() >= dy.abs() {
        if dx >= 0. {
            (from.x + from.w, fcy)
        } else {
            (from.x, fcy)
        }
    } else if dy >= 0. {
        (fcx, from.y + from.h)
    } else {
        (fcx, from.y)
    }
}

/// 注记虚线的两个端点。横向图里关系线通常从类中部进入，
/// 把宿主侧端点压低一点，虚线就不会和实线糊在同一条水平线上。
fn note_link(cand: Rect, o: Rect, horizontal: bool) -> ((f32, f32), (f32, f32)) {
    let a = link_anchor(cand, o);
    let mut c = link_anchor(o, cand);
    if horizontal {
        c.1 = o.y + o.h * 0.65;
    }
    (a, c)
}

/// 点到线段的距离，用于判断两条线是否贴在一起。
fn point_segment_dist(p: (f32, f32), a: (f32, f32), c: (f32, f32)) -> f32 {
    let (dx, dy) = (c.0 - a.0, c.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 0.001 {
        return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0., 1.);
    ((p.0 - (a.0 + dx * t)).powi(2) + (p.1 - (a.1 + dy * t)).powi(2)).sqrt()
}

/// 采样检查注记虚线是否几乎贴着某条关系线走；贴住了就换注记位置。
fn link_hugs_edge(a: (f32, f32), c: (f32, f32), edges: &[EdgeDraw]) -> bool {
    const STEPS: i32 = 16;
    (1..STEPS).any(|k| {
        let t = k as f32 / STEPS as f32;
        let q = (a.0 + (c.0 - a.0) * t, a.1 + (c.1 - a.1) * t);
        edges
            .iter()
            .any(|e| e.points.windows(2).any(|s| point_segment_dist(q, s[0], s[1]) < 3.))
    })
}

// ---------------------------------------------------------------------------
// 图结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Direction {
    Tb,
    Bt,
    Lr,
    Rl,
}

#[derive(Clone)]
struct ClassBox {
    name: String,
    /// 属性栏：不含括号的成员。
    attrs: Vec<String>,
    /// 方法栏：含 `(` 的成员。
    methods: Vec<String>,
    /// `<<interface>>` 之类，显示在类名上方。
    annotations: Vec<String>,
    /// `note for` / `note "…" as` 生成的注释框：虚线边框，不画三栏分隔线。
    note: bool,
    /// 注释框依附的类；`None` 表示独立注释。
    owner: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum RelKind {
    Inherit,
    Compose,
    Aggregate,
    Assoc,
    Link,
    Dependency,
    Realize,
    DashedLink,
}

impl RelKind {
    fn dashed(self) -> bool {
        matches!(
            self,
            RelKind::Dependency | RelKind::Realize | RelKind::DashedLink
        )
    }

    fn directed(self) -> bool {
        !matches!(self, RelKind::Link | RelKind::DashedLink)
    }
}

#[derive(Clone)]
struct Relation {
    a: usize,
    b: usize,
    kind: RelKind,
    /// 端点符号画在 `a` 端（`<|--`、`*--` 这类左侧记号）。
    /// 注意：这里只影响符号，不影响「a 在 b 之上」的分层方向。
    reverse: bool,
    a_mult: Option<String>,
    b_mult: Option<String>,
    label: Option<String>,
}

#[derive(Clone)]
pub(super) struct ClassDiagram {
    classes: Vec<ClassBox>,
    relations: Vec<Relation>,
    direction: Direction,
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 长记号必须排在短记号前面：`..>` 不能被 `..` 抢先匹配。
const REL_TOKENS: &[(&str, RelKind, bool)] = &[
    ("<|..", RelKind::Realize, true),
    ("..|>", RelKind::Realize, false),
    ("<|--", RelKind::Inherit, true),
    ("--|>", RelKind::Inherit, false),
    ("*--", RelKind::Compose, true),
    ("--*", RelKind::Compose, false),
    ("o--", RelKind::Aggregate, true),
    ("--o", RelKind::Aggregate, false),
    ("<--", RelKind::Assoc, true),
    ("-->", RelKind::Assoc, false),
    ("<..", RelKind::Dependency, true),
    ("..>", RelKind::Dependency, false),
    ("..", RelKind::DashedLink, false),
    ("--", RelKind::Link, false),
];

fn find_relation(line: &str) -> Option<(usize, usize, RelKind, bool)> {
    let mut best: Option<(usize, usize, RelKind, bool)> = None;
    for &(token, kind, reverse) in REL_TOKENS {
        if let Some(at) = line.find(token)
            && best.is_none_or(|(pos, len, _, _)| at < pos || (at == pos && token.len() > len))
        {
            best = Some((at, token.len(), kind, reverse));
        }
    }
    best
}

/// 泛型 `Square~Shape~` 用 `Square` 做 id，显示名保留原文。
fn normalize_id(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .split('~')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

/// `<<interface>>` → `«interface»`，与 Mermaid 的类框注解一致。
fn anno_text(raw: &str) -> String {
    match raw.strip_prefix("<<").and_then(|s| s.strip_suffix(">>")) {
        Some(inner) => format!("«{inner}»"),
        None => raw.to_string(),
    }
}

/// Mermaid 按「是否带括号」把成员分到属性栏和方法栏。
fn is_method(member: &str) -> bool {
    member.contains('(') || member.trim_end().ends_with(')')
}

fn ensure(classes: &mut Vec<ClassBox>, index: &mut HashMap<String, usize>, raw: &str) -> usize {
    let id = normalize_id(raw);
    if id.is_empty() {
        return usize::MAX;
    }
    if let Some(&ix) = index.get(&id) {
        return ix;
    }
    let ix = classes.len();
    classes.push(ClassBox {
        name: clean_label(raw.trim()),
        attrs: Vec::new(),
        methods: Vec::new(),
        annotations: Vec::new(),
        note: false,
        owner: None,
    });
    index.insert(id, ix);
    ix
}

fn add_member(classes: &mut [ClassBox], ix: usize, member: &str) {
    if ix == usize::MAX || classes.is_empty() {
        return;
    }
    let text = clean_label(member);
    if text.is_empty() {
        return;
    }
    if is_method(&text) {
        classes[ix].methods.push(text);
    } else {
        classes[ix].attrs.push(text);
    }
}

/// `A "1"` → `("A", Some("1"))`；引号外的内容拼成 id。
fn split_side(text: &str) -> (String, Option<String>) {
    let parts: Vec<&str> = text.split('"').collect();
    let id = parts
        .iter()
        .step_by(2)
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mult = parts
        .get(1)
        .map(|s| clean_label(s))
        .filter(|s| !s.is_empty());
    (id, mult)
}

/// 注释框不生成关系线：它直接贴在宿主类上方，由 layout 单独定位。
fn add_note(
    classes: &mut Vec<ClassBox>,
    index: &mut HashMap<String, usize>,
    text: &str,
    key: &str,
    owner: Option<usize>,
) -> usize {
    let ix = classes.len();
    classes.push(ClassBox {
        name: clean_label(text),
        attrs: Vec::new(),
        methods: Vec::new(),
        annotations: Vec::new(),
        note: true,
        owner: owner.filter(|&o| o != usize::MAX),
    });
    index.insert(normalize_id(key), ix);
    ix
}

pub(super) fn parse(source: &str) -> Option<ClassDiagram> {
    let lines: Vec<&str> = source.lines().collect();
    let mut classes: Vec<ClassBox> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut relations: Vec<Relation> = Vec::new();
    let mut direction = Direction::Tb;
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("classdiagram") {
            continue;
        }
        if let Some(rest) = lower.strip_prefix("direction ") {
            direction = match rest.trim() {
                "bt" => Direction::Bt,
                "lr" => Direction::Lr,
                "rl" => Direction::Rl,
                _ => Direction::Tb,
            };
            continue;
        }
        if lower.starts_with("style ") || lower.starts_with("click ") || lower == "end" {
            continue;
        }

        // 独立注解：`<<interface>> Shape`
        if lower.starts_with("<<") {
            if let Some(end) = line.find(">>") {
                let anno = clean_label(&line[..end + 2]);
                let id = line[end + 2..].trim();
                let ix = ensure(&mut classes, &mut index, id);
                if ix != usize::MAX && !classes[ix].annotations.contains(&anno) {
                    classes[ix].annotations.push(anno);
                }
            }
            continue;
        }

        // `note for X "文本"` 与 `note "文本" as N`
        if lower.starts_with("note for ") {
            let raw = &line[9..];
            let (id, text) = split_side(raw);
            let owner = ensure(&mut classes, &mut index, &id);
            let key = format!("__note_{}", classes.len());
            add_note(
                &mut classes,
                &mut index,
                &text.unwrap_or_default(),
                &key,
                Some(owner),
            );
            continue;
        }
        if lower.starts_with("note ") {
            let raw = &line[5..];
            let text = split_side(raw).1.unwrap_or_default();
            let key = match raw.rsplit_once(" as ") {
                Some((_, id)) => id.trim().to_string(),
                None => format!("__note_{}", classes.len()),
            };
            add_note(&mut classes, &mut index, &text, &key, None);
            continue;
        }

        // 类块：`class X { ... }`
        if lower.starts_with("class ") && line.ends_with('{') {
            let id = line[6..line.len() - 1].trim();
            let ix = ensure(&mut classes, &mut index, id);
            while i < lines.len() {
                let inner = lines[i].trim();
                i += 1;
                if inner.starts_with('}') || inner.is_empty() {
                    if inner.starts_with('}') {
                        break;
                    }
                    continue;
                }
                if inner.starts_with("%%") {
                    continue;
                }
                if inner.starts_with("<<") && inner.ends_with(">>") {
                    if ix != usize::MAX && !classes[ix].annotations.contains(&inner.to_string()) {
                        classes[ix].annotations.push(inner.to_string());
                    }
                } else {
                    add_member(&mut classes, ix, inner);
                }
            }
            continue;
        }

        // 关系行优先于 `class X : member`：`<|--` 里也含 `:`.
        if let Some((at, len, kind, reverse)) = find_relation(line) {
            let left = &line[..at];
            let right = &line[at + len..];
            let (b_part, label) = match right.split_once(':') {
                Some((head, text)) => (head, Some(clean_label(text))),
                None => (right, None),
            };
            let (a_id, a_mult) = split_side(left);
            let (b_id, b_mult) = split_side(b_part);
            let a = ensure(&mut classes, &mut index, &a_id);
            let b = ensure(&mut classes, &mut index, &b_id);
            if a != usize::MAX && b != usize::MAX && a != b {
                relations.push(Relation {
                    a,
                    b,
                    kind,
                    reverse,
                    a_mult,
                    b_mult,
                    label: label.filter(|s| !s.is_empty()),
                });
            }
            continue;
        }

        // `class X : +int age` 冒号式成员，以及裸 `class X` 声明。
        if let Some(rest) = line.strip_prefix("class ") {
            if let Some((id, member)) = rest.split_once(':') {
                let ix = ensure(&mut classes, &mut index, id);
                add_member(&mut classes, ix, member);
            } else {
                ensure(&mut classes, &mut index, rest);
            }
            continue;
        }
        if let Some((id, member)) = line.split_once(':') {
            let ix = ensure(&mut classes, &mut index, id);
            add_member(&mut classes, ix, member);
        }
    }

    if classes.is_empty() {
        return None;
    }
    Some(ClassDiagram {
        classes,
        relations,
        direction,
    })
}

// ---------------------------------------------------------------------------
// 布局
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum MarkerKind {
    Triangle,
    Diamond,
    Open,
}

struct Marker {
    tip: (f32, f32),
    dir: (f32, f32),
    kind: MarkerKind,
    filled: bool,
}

struct EdgeDraw {
    /// 端点索引只在「线不能穿过别的框」回归测试里用到。
    #[cfg_attr(not(test), allow(dead_code))]
    a: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    b: usize,
    points: Vec<(f32, f32)>,
    dashed: bool,
    markers: Vec<Marker>,
}

/// 每个类框的三栏高度，渲染层据此放分隔线与文字。
#[derive(Clone, Copy)]
struct BoxSize {
    w: f32,
    h: f32,
    header_h: f32,
    attrs_h: f32,
}

struct ClassLayout {
    width: f32,
    height: f32,
    rects: Vec<Rect>,
    edges: Vec<EdgeDraw>,
    /// `(x, y, 文本, 所属边序号)`，测试用来保证标签贴自己的线。
    labels: Vec<(f32, f32, String, usize)>,
    mults: Vec<(f32, f32, String)>,
    /// 按类索引存放 `(x, y, w)`：每个非注释类框两道分隔线。
    dividers: Vec<Vec<(f32, f32, f32)>>,
    /// 注释框 → 宿主类的短虚线。
    note_links: Vec<((f32, f32), (f32, f32))>,
}

fn sizes(diagram: &ClassDiagram) -> Vec<BoxSize> {
    diagram
        .classes
        .iter()
        .map(|c| {
            if c.note {
                return BoxSize {
                    w: text_width(&c.name, 11.) + 24.,
                    h: NOTE_H,
                    header_h: 0.,
                    attrs_h: 0.,
                };
            }
            let widest = c
                .attrs
                .iter()
                .chain(c.methods.iter())
                .map(|m| text_width(m, 11.))
                .chain(c.annotations.iter().map(|a| text_width(&anno_text(a), 10.)))
                .fold(0., f32::max);
            let w = (text_width(&c.name, 13.).max(widest) + 28.).max(MIN_W);
            let header_h = c.annotations.len() as f32 * ANNO_H + TITLE_H;
            let attrs_h = if c.attrs.is_empty() {
                EMPTY_CELL_H
            } else {
                c.attrs.len() as f32 * LINE_H
            };
            let methods_h = if c.methods.is_empty() {
                EMPTY_CELL_H
            } else {
                c.methods.len() as f32 * LINE_H
            };
            BoxSize {
                w,
                h: header_h + attrs_h + methods_h,
                header_h,
                attrs_h,
            }
        })
        .collect()
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/// 线段是否穿过矩形内部（不含边界）。斜线用采样近似，够布局避让用了。
fn seg_hits_rect(p: (f32, f32), q: (f32, f32), r: Rect) -> bool {
    let (x1, x2, y1, y2) = (r.x + 1., r.x + r.w - 1., r.y + 1., r.y + r.h - 1.);
    if x2 <= x1 || y2 <= y1 {
        return false;
    }
    if (p.1 - q.1).abs() < 0.5 {
        if p.1 <= y1 || p.1 >= y2 {
            return false;
        }
        return p.0.min(q.0) < x2 && p.0.max(q.0) > x1;
    }
    if (p.0 - q.0).abs() < 0.5 {
        if p.0 <= x1 || p.0 >= x2 {
            return false;
        }
        return p.1.min(q.1) < y2 && p.1.max(q.1) > y1;
    }
    for k in 1..8 {
        let t = k as f32 / 8.;
        let (x, y) = (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
        if x > x1 && x < x2 && y > y1 && y < y2 {
            return true;
        }
    }
    false
}

fn unit(from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    (dx / len, dy / len)
}

fn layout(diagram: &ClassDiagram) -> ClassLayout {
    let n = diagram.classes.len();
    let sizes = sizes(diagram);
    let vertical = matches!(diagram.direction, Direction::Tb | Direction::Bt);

    // 最长路径分层：所有关系都是「左操作数在前、右操作数在后」。
    // `<|--` / `*--` 的 reverse 只决定三角/菱形画在哪一端，不能把层次倒过来，
    // 否则父类会跑到子类下方（旧实现的问题）。
    let mut rank = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for r in &diagram.relations {
            if rank[r.b] < rank[r.a] + 1 {
                rank[r.b] = rank[r.a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    if matches!(diagram.direction, Direction::Bt | Direction::Rl) {
        for r in &mut rank {
            *r = max_rank - *r;
        }
    }
    let depth = max_rank + 1;

    // 注释框不参与分层。
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for ix in 0..n {
        if !diagram.classes[ix].note {
            layers[rank[ix]].push(ix);
        }
    }

    let main_gap = if vertical { V_GAP } else { H_GAP + 40. };
    let mut main_offsets = Vec::with_capacity(depth);
    let mut thickness = Vec::with_capacity(depth);
    let mut cursor = PAD + TOP_EXTRA;
    for layer in &layers {
        main_offsets.push(cursor);
        let t = layer
            .iter()
            .map(|&ix| if vertical { sizes[ix].h } else { sizes[ix].w })
            .fold(0., f32::max);
        thickness.push(t);
        cursor += t + main_gap;
    }
    let main_size = cursor - main_gap + PAD + BOTTOM_EXTRA;

    let cross_of = |ix: usize| {
        if vertical {
            sizes[ix].w + H_GAP
        } else {
            sizes[ix].h + V_GAP / 2.
        }
    };
    let cross_extent = layers
        .iter()
        .map(|layer| layer.iter().map(|&ix| cross_of(ix)).sum::<f32>())
        .fold(0., f32::max);

    let mut rects = vec![
        Rect {
            x: 0.,
            y: 0.,
            w: 0.,
            h: 0.
        };
        n
    ];
    for (li, layer) in layers.iter().enumerate() {
        let extent: f32 = layer.iter().map(|&ix| cross_of(ix)).sum();
        let mut cross = PAD + (cross_extent - extent).max(0.) / 2.;
        for &ix in layer {
            let s = sizes[ix];
            let (x, y) = if vertical {
                (cross, main_offsets[li])
            } else {
                (main_offsets[li], cross)
            };
            rects[ix] = Rect {
                x,
                y,
                w: s.w,
                h: s.h,
            };
            cross += cross_of(ix);
        }
    }

    let mut width = if vertical {
        cross_extent + PAD
    } else {
        main_size
    };
    let mut height = if vertical {
        main_size
    } else {
        cross_extent + PAD
    };
    for r in &rects {
        if r.w > 0. {
            width = width.max(r.x + r.w + PAD);
            height = height.max(r.y + r.h + PAD);
        }
    }

    // 层间空白带的中心线：关系折线只能在这些带里横向走，避免穿过类框。
    let mut bands: Vec<f32> = Vec::with_capacity(depth + 1);
    let first_gap = (PAD + TOP_EXTRA - main_gap / 2.).max(10.);
    bands.push(first_gap);
    for li in 1..depth {
        bands.push((main_offsets[li - 1] + thickness[li - 1] + main_offsets[li]) / 2.);
    }
    bands.push(main_offsets[depth - 1] + thickness[depth - 1] + main_gap / 2.);

    // 同一个类上的多个端点沿框宽/框高均分，避免几条线叠在一起。
    let mut counts = vec![0usize; n];
    for r in &diagram.relations {
        counts[r.a] += 1;
        counts[r.b] += 1;
    }
    let mut used_a = vec![0usize; n];
    let mut used_b = vec![0usize; n];
    let mut slots = vec![((0usize, 0usize), (0usize, 0usize)); diagram.relations.len()];
    for (ei, r) in diagram.relations.iter().enumerate() {
        slots[ei].0 = (used_a[r.a], counts[r.a]);
        used_a[r.a] += 1;
        slots[ei].1 = (used_b[r.b], counts[r.b]);
        used_b[r.b] += 1;
    }
    let frac = |slot: (usize, usize)| (slot.0 as f32 + 1.) / (slot.1 as f32 + 1.);

    let mut edges = Vec::new();
    let mut labels = Vec::new();
    let mut mults = Vec::new();
    let mut used_lanes: Vec<f32> = Vec::new();

    for (ei, r) in diagram.relations.iter().enumerate() {
        let ra = rects[r.a];
        let rb = rects[r.b];
        let (fa, fb) = (frac(slots[ei].0), frac(slots[ei].1));
        let same = rank[r.a] == rank[r.b];
        let (s, e) = if vertical {
            let (ay, by) = (ra.y + ra.h / 2., rb.y + rb.h / 2.);
            if same || ay < by {
                (
                    (ra.x + ra.w * fa, ra.y + ra.h),
                    (rb.x + rb.w * fb, if same { rb.y + rb.h } else { rb.y }),
                )
            } else {
                ((ra.x + ra.w * fa, ra.y), (rb.x + rb.w * fb, rb.y + rb.h))
            }
        } else {
            let (ax, bx) = (ra.x + ra.w / 2., rb.x + rb.w / 2.);
            if same || ax < bx {
                (
                    (ra.x + ra.w, ra.y + ra.h * fa),
                    (if same { rb.x + rb.w } else { rb.x }, rb.y + rb.h * fb),
                )
            } else {
                ((ra.x, ra.y + ra.h * fa), (rb.x + rb.w, rb.y + rb.h * fb))
            }
        };

        // 候选布线带：两端层之间的空白带，每条带再给几个微调档位。
        let (lo, hi) = (rank[r.a].min(rank[r.b]), rank[r.a].max(rank[r.b]));
        let band_indices: Vec<usize> = if same {
            vec![lo + 1]
        } else {
            (lo + 1..=hi).collect()
        };
        let ideal = if vertical {
            (s.1 + e.1) / 2.
        } else {
            (s.0 + e.0) / 2.
        };
        let mut best: Option<(f32, Vec<(f32, f32)>, f32)> = None;
        for &bi in &band_indices {
            let center = bands[bi.min(bands.len() - 1)];
            for offset in [-24., -12., 0., 12., 24.] {
                let lane = center + offset;
                let points = if vertical {
                    if (s.0 - e.0).abs() < 1. {
                        vec![s, e]
                    } else {
                        vec![s, (s.0, lane), (e.0, lane), e]
                    }
                } else if (s.1 - e.1).abs() < 1. {
                    vec![s, e]
                } else {
                    vec![s, (lane, s.1), (lane, e.1), e]
                };
                let mut hits = 0i32;
                for seg in points.windows(2) {
                    for (j, other) in rects.iter().enumerate() {
                        if j == r.a || j == r.b || other.w <= 0. {
                            continue;
                        }
                        if seg_hits_rect(seg[0], seg[1], *other) {
                            hits += 1;
                        }
                    }
                }
                let crowded = used_lanes
                    .iter()
                    .filter(|&&l| (l - lane).abs() < 7.)
                    .count() as f32;
                let score = hits as f32 * 100.
                    + crowded * 30.
                    + (lane - ideal).abs() * 0.05
                    + (lane - center).abs() * 0.02;
                if best.as_ref().is_none_or(|(s0, _, _)| score < *s0) {
                    best = Some((score, points, lane));
                }
            }
        }
        let (_, points, lane) = best.expect("至少有一条候选布线带");
        used_lanes.push(lane);

        let mut markers = Vec::new();
        if r.kind.directed() {
            let (tip, from) = if r.reverse {
                (points[0], points[1])
            } else {
                let last = points.len() - 1;
                (points[last], points[last - 1])
            };
            let (kind, filled) = match r.kind {
                RelKind::Inherit | RelKind::Realize => (MarkerKind::Triangle, false),
                RelKind::Compose => (MarkerKind::Diamond, true),
                RelKind::Aggregate => (MarkerKind::Diamond, false),
                _ => (MarkerKind::Open, true),
            };
            markers.push(Marker {
                tip,
                dir: unit(from, tip),
                kind,
                filled,
            });
        }

        // 标签放在较长的那段出/入竖线上，而不是两端锚点的几何中点——
        // 后者会落在别的线上，看起来像标签贴错了关系。
        if let Some(text) = &r.label {
            let (x, y) = if points.len() == 2 {
                (
                    (points[0].0 + points[1].0) / 2.,
                    (points[0].1 + points[1].1) / 2.,
                )
            } else {
                let last = points.len() - 1;
                let first_seg = (points[0], points[1]);
                let last_seg = (points[last - 1], points[last]);
                let len =
                    |p: ((f32, f32), (f32, f32))| (p.1.0 - p.0.0).abs() + (p.1.1 - p.0.1).abs();
                let seg = if len(first_seg) >= len(last_seg) {
                    first_seg
                } else {
                    last_seg
                };
                ((seg.0.0 + seg.1.0) / 2., (seg.0.1 + seg.1.1) / 2.)
            };
            labels.push((x, y, text.clone(), ei));
        }

        // 多重度贴在各自端点外侧，不能落在框内。
        let last = points.len() - 1;
        if let Some(text) = &r.a_mult {
            let out = unit(points[0], points[1]);
            mults.push((
                points[0].0 + out.0 * 12. + 8.,
                points[0].1 + out.1 * 12.,
                text.clone(),
            ));
        }
        if let Some(text) = &r.b_mult {
            let out = unit(points[last], points[last - 1]);
            mults.push((
                points[last].0 + out.0 * 12. + 8.,
                points[last].1 + out.1 * 12.,
                text.clone(),
            ));
        }
        edges.push(EdgeDraw {
            a: r.a,
            b: r.b,
            points,
            dashed: r.kind.dashed(),
            markers,
        });
    }

    // 注记框最后定位：此时关系折线已经算好，注记要同时避开类框、关系线与
    // 别的注记。顺序不能反过来——旧实现先放注记再布线，LR 图里注记正好压在
    // 「OrderService → PaymentClient」的折线通道上，线只能从注记中间穿过去。
    // TB/BT 优先贴在宿主上方；LR/RL 优先贴在宿主逆流方向一侧（LR 在左、RL 在右），
    // 与 Mermaid.js 的 `note for` 观感一致。
    let horizontal = !vertical;
    let mut note_links = Vec::new();
    for ix in 0..n {
        if !diagram.classes[ix].note {
            continue;
        }
        let s = sizes[ix];
        let Some(owner) = diagram.classes[ix].owner.filter(|&o| o < n) else {
            // 独立注记：从左上往下找第一块空白（不压框、不压线）。
            let mut y = PAD;
            for _ in 0..20 {
                let cand = Rect {
                    x: PAD,
                    y,
                    w: s.w,
                    h: s.h,
                };
                let hits_box = (0..n)
                    .any(|j| j != ix && rects[j].w > 0. && overlap(cand, rects[j]));
                let hits_edge = edges
                    .iter()
                    .any(|e| e.points.windows(2).any(|g| seg_hits_rect(g[0], g[1], cand)));
                if !hits_box && !hits_edge {
                    break;
                }
                y += s.h + NOTE_GAP;
            }
            rects[ix] = Rect {
                x: PAD,
                y,
                w: s.w,
                h: s.h,
            };
            continue;
        };

        let o = rects[owner];
        let cy = o.y + o.h / 2. - s.h / 2.;
        let left_side = !matches!(diagram.direction, Direction::Rl);
        let mut candidates: Vec<(f32, f32)> = Vec::new();
        if vertical {
            for dy in [NOTE_GAP, NOTE_GAP + 26., NOTE_GAP + 52.] {
                candidates.push((o.x + o.w / 2. - s.w / 2., o.y - s.h - dy));
            }
            candidates.push((o.x + o.w / 2. - s.w / 2., o.y + o.h + NOTE_GAP));
            candidates.push((o.x - s.w - NOTE_GAP, cy));
            candidates.push((o.x + o.w + NOTE_GAP, cy));
        } else if left_side {
            // 宿主左侧，按离宿主由近到远试几个横向档位；被关系线占住的档位跳过。
            // 每档先试略下移 12px：LR 图里上一层类框的底边往往正好压在宿主中线附近，
            // 注记贴在正中线会和它只差不到 1px，看起来像粘在别人框上。
            for dx in [NOTE_GAP, NOTE_GAP + 26., NOTE_GAP + 52., NOTE_GAP + 84.] {
                candidates.push((o.x - s.w - dx, cy + 12.));
                candidates.push((o.x - s.w - dx, cy));
            }
            candidates.push((o.x - s.w - NOTE_GAP, o.y + o.h - s.h));
            candidates.push((o.x + o.w + NOTE_GAP, cy));
        } else {
            for dx in [NOTE_GAP, NOTE_GAP + 26., NOTE_GAP + 52., NOTE_GAP + 84.] {
                candidates.push((o.x + o.w + dx, cy + 12.));
                candidates.push((o.x + o.w + dx, cy));
            }
            candidates.push((o.x + o.w + NOTE_GAP, o.y + o.h - s.h));
            candidates.push((o.x - s.w - NOTE_GAP, cy));
        }
        let bottom = if vertical { main_size } else { cross_extent + PAD };
        candidates.push((PAD, (bottom - PAD - s.h).max(PAD)));
        candidates.push((PAD, PAD));

        let valid = |x: f32, y: f32| {
            if x < PAD || y < PAD {
                return false;
            }
            let cand = Rect {
                x,
                y,
                w: s.w,
                h: s.h,
            };
            let (a, c) = note_link(cand, o, horizontal);
            let hits_box = (0..n).any(|j| {
                j != ix
                    && rects[j].w > 0.
                    && (overlap(cand, rects[j])
                        // 虚线终点本来就落在宿主类边界上，宿主自身不算「穿框」。
                        || (j != owner && seg_hits_rect(a, c, rects[j])))
            });
            let hits_edge = edges
                .iter()
                .any(|e| e.points.windows(2).any(|g| seg_hits_rect(g[0], g[1], cand)));
            !hits_box && !hits_edge && !link_hugs_edge(a, c, &edges)
        };
        let (x, y) = candidates
            .iter()
            .copied()
            .find(|&(x, y)| valid(x, y))
            .unwrap_or((PAD, PAD));
        let cand = Rect {
            x,
            y,
            w: s.w,
            h: s.h,
        };
        rects[ix] = cand;
        note_links.push(note_link(cand, o, horizontal));
    }

    // 注记可能超出主层范围，画布要再扩一圈。
    for r in &rects {
        if r.w > 0. {
            width = width.max(r.x + r.w + PAD);
            height = height.max(r.y + r.h + PAD);
        }
    }

    let mut dividers: Vec<Vec<(f32, f32, f32)>> = vec![Vec::new(); n];
    for (i, c) in diagram.classes.iter().enumerate() {
        if c.note {
            continue;
        }
        let r = rects[i];
        let s = sizes[i];
        dividers[i].push((r.x, r.y + s.header_h, r.w));
        dividers[i].push((r.x, r.y + s.header_h + s.attrs_h, r.w));
    }

    ClassLayout {
        width,
        height,
        rects,
        edges,
        labels,
        mults,
        dividers,
        note_links,
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

fn marker_points(m: &Marker) -> Vec<(f32, f32)> {
    let (ux, uy) = m.dir;
    let (nx, ny) = (-uy, ux);
    let back = |d: f32, side: f32| (m.tip.0 - ux * d + nx * side, m.tip.1 - uy * d + ny * side);
    match m.kind {
        MarkerKind::Triangle => vec![m.tip, back(12., 7.), back(12., -7.)],
        MarkerKind::Diamond => vec![m.tip, back(7., 6.), back(14., 0.), back(7., -6.)],
        MarkerKind::Open => vec![back(10., 5.), m.tip, back(10., -5.)],
    }
}

pub(super) fn render(diagram: &ClassDiagram, cx: &App) -> AnyElement {
    let p = palette(cx.theme());
    let lay = layout(diagram);
    let sizes = sizes(diagram);

    // canvas 需要 'static：把要画的数据先 clone 好。
    // 注意 canvas 是第一个 child，会被后面的类框 div 盖住，所以分隔线改用 div 画。
    let draw = DrawData {
        edges: lay
            .edges
            .iter()
            .map(|e| {
                (
                    e.points.clone(),
                    e.dashed,
                    e.markers
                        .iter()
                        .map(|m| (marker_points(m), m.filled, m.kind))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        note_rects: diagram
            .classes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.note)
            .map(|(i, _)| lay.rects[i])
            .collect(),
        note_links: lay.note_links.clone(),
    };

    let paint = canvas(
        move |bounds, _, _| bounds.origin,
        move |_bounds, origin, window, _| {
            let (ox, oy) = (origin.x, origin.y);
            let at = |p: &(f32, f32)| point(ox + px(p.0), oy + px(p.1));
            macro_rules! seg {
                ($b:expr, $a:expr, $c:expr) => {{
                    $b.move_to(at(&$a));
                    $b.line_to(at(&$c));
                }};
            }

            // 注记框底色：先铺一层，让注记文字和背后的关系线分开。
            for r in &draw.note_rects {
                let mut fill = PathBuilder::fill();
                fill.move_to(at(&(r.x, r.y)));
                fill.line_to(at(&(r.x + r.w, r.y)));
                fill.line_to(at(&(r.x + r.w, r.y + r.h)));
                fill.line_to(at(&(r.x, r.y + r.h)));
                fill.close();
                if let Ok(path) = fill.build() {
                    window.paint_path(path, p.fill);
                }
            }

            // 关系线与端点符号：统一用强调色 + 加粗，深色背景下也能看清。
            for (points, dashed, markers) in &draw.edges {
                let mut b = PathBuilder::stroke(px(EDGE_W));
                if *dashed {
                    for pair in points.windows(2) {
                        dashed_line(&mut b, pair[0], pair[1], &at);
                    }
                } else {
                    rounded_elbow(&mut b, points, 8., &at);
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, p.stroke);
                }
                for (poly, filled, kind) in markers {
                    if *kind == MarkerKind::Open {
                        let mut open = PathBuilder::stroke(px(EDGE_W));
                        seg!(open, poly[0], poly[1]);
                        seg!(open, poly[2], poly[1]);
                        if let Ok(path) = open.build() {
                            window.paint_path(path, p.stroke);
                        }
                        continue;
                    }
                    let mut fill = PathBuilder::fill();
                    fill.move_to(at(&poly[0]));
                    for q in &poly[1..] {
                        fill.line_to(at(q));
                    }
                    fill.close();
                    if let Ok(path) = fill.build() {
                        window.paint_path(path, if *filled { p.stroke } else { p.chip });
                    }
                    let mut stroke = PathBuilder::stroke(px(MARKER_W));
                    stroke.move_to(at(&poly[0]));
                    for q in &poly[1..] {
                        stroke.line_to(at(q));
                    }
                    stroke.close();
                    if let Ok(path) = stroke.build() {
                        window.paint_path(path, p.stroke);
                    }
                }
            }

            // 注释框 → 宿主类的短虚线：与关系线同色同量级，不能淡到看不见。
            for (a, c) in &draw.note_links {
                let mut b = PathBuilder::stroke(px(NOTE_LINK_W));
                dashed_line(&mut b, *a, *c, &at);
                if let Ok(path) = b.build() {
                    window.paint_path(path, p.stroke);
                }
            }

            // note 框用虚线，跟三栏类框区分开
            for r in &draw.note_rects {
                let mut b = PathBuilder::stroke(px(NOTE_FRAME_W));
                let corners = [
                    (r.x, r.y),
                    (r.x + r.w, r.y),
                    (r.x + r.w, r.y + r.h),
                    (r.x, r.y + r.h),
                ];
                for k in 0..4 {
                    dashed_line(&mut b, corners[k], corners[(k + 1) % 4], &at);
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, p.stroke);
                }
            }
        },
    )
    .absolute()
    .inset_0();

    let mut container = div()
        .relative()
        .w(px(lay.width))
        .h(px(lay.height))
        .mx_auto()
        .child(paint);

    for (i, c) in diagram.classes.iter().enumerate() {
        let r = lay.rects[i];
        let s = sizes[i];
        if !c.note {
            container = container.child(
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.w))
                    .h(px(r.h))
                    .rounded(px(4.))
                    .border_2()
                    .border_color(p.stroke)
                    .bg(p.fill),
            );
            // 两道分隔线：表头 / 属性 / 方法，跟 Mermaid 的三栏类框一致。
            // 用 layout 算好的坐标，避免渲染层再算一遍导致两边走样；
            // 颜色和类框边框一致，粗细也随强调色一起加粗。
            for (x, y, w) in &lay.dividers[i] {
                container = container.child(
                    div()
                        .absolute()
                        .left(px(*x + 2.))
                        .top(px(*y))
                        .w(px((w - 4.).max(1.)))
                        .h(px(DIVIDER_H))
                        .bg(p.stroke),
                );
            }
        }

        let anno_h = c.annotations.len() as f32 * ANNO_H;
        for (k, anno) in c.annotations.iter().enumerate() {
            container = container.child(
                div()
                    .absolute()
                    .left(px(r.x + 6.))
                    .top(px(r.y + 2. + k as f32 * ANNO_H))
                    .w(px((r.w - 12.).max(1.)))
                    .text_size(px(10.))
                    .text_color(p.text)
                    .text_center()
                    .child(anno_text(anno)),
            );
        }

        container = container.child(
            div()
                .absolute()
                .left(px(r.x + 6.))
                .top(px(r.y + anno_h + (if c.note { 6. } else { 3. })))
                .w(px((r.w - 12.).max(1.)))
                .text_size(px(if c.note { 11. } else { 13. }))
                .text_color(p.text)
                .text_center()
                .child(c.name.clone()),
        );

        if c.note {
            continue;
        }
        let attrs_y = r.y + s.header_h + 2.;
        for (k, member) in c.attrs.iter().enumerate() {
            container = container.child(
                div()
                    .absolute()
                    .left(px(r.x + 8.))
                    .top(px(attrs_y + k as f32 * LINE_H))
                    .w(px((r.w - 16.).max(1.)))
                    .text_size(px(11.))
                    .text_color(p.text)
                    .child(member.clone()),
            );
        }
        let methods_y = attrs_y + s.attrs_h + 2.;
        for (k, member) in c.methods.iter().enumerate() {
            container = container.child(
                div()
                    .absolute()
                    .left(px(r.x + 8.))
                    .top(px(methods_y + k as f32 * LINE_H))
                    .w(px((r.w - 16.).max(1.)))
                    .text_size(px(11.))
                    .text_color(p.text)
                    .child(member.clone()),
            );
        }
    }

    for (x, y, text, _) in &lay.labels {
        let w = text_width(text, 11.) + 10.;
        container = container.child(
            h_flex()
                .absolute()
                .left(px((x - w / 2.).clamp(0., (lay.width - w).max(0.))))
                .top(px(y - 8.))
                .px(px(3.))
                .bg(p.chip)
                .text_size(px(11.))
                .text_color(p.text)
                .child(text.clone()),
        );
    }
    for (x, y, text) in &lay.mults {
        let w = text_width(text, 10.) + 6.;
        container = container.child(
            div()
                .absolute()
                .left(px((x - w / 2.).clamp(0., (lay.width - w).max(0.))))
                .top(px(y - 7.))
                .px(px(2.))
                .bg(p.chip)
                .text_size(px(10.))
                .text_color(p.text)
                .child(text.clone()),
        );
    }

    container.into_any_element()
}

struct DrawData {
    edges: Vec<(
        Vec<(f32, f32)>,
        bool,
        Vec<(Vec<(f32, f32)>, bool, MarkerKind)>,
    )>,
    note_rects: Vec<Rect>,
    note_links: Vec<((f32, f32), (f32, f32))>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"classDiagram
    direction TB
    class Animal {
        +int age
        -String name
        +void eat()
    }
    <<interface>> Animal
    Animal <|-- Duck
    Animal *-- Heart : 组合
    Duck "1" --> "*" Pond : 关联
    Duck ..> Food : 依赖
    Duck ..|> Swim
    note for Duck "水禽"
"#;

    /// 展示页「关系符号集合」那张：含父类、注解、注记与多重度，
    /// 和应用侧类图页是同一份文件。按内容找，避免依赖示例顺序。
    fn showcase() -> String {
        super::super::showcase_blocks("class")
            .into_iter()
            .find(|b| b.contains("class Animal"))
            .expect("展示页应有 Animal 类图")
    }

    #[test]
    fn parses_classes_members_and_relations() {
        let d = parse(SAMPLE).expect("应可解析");
        assert_eq!(d.classes.len(), 7, "Animal/Duck/Heart/Pond/Food/Swim/note");
        let animal = d.classes.iter().find(|c| c.name == "Animal").unwrap();
        assert_eq!(animal.attrs, vec!["+int age", "-String name"]);
        assert_eq!(animal.methods, vec!["+void eat()"]);
        assert_eq!(anno_text(&animal.annotations[0]), "«interface»");
        assert_eq!(d.relations.len(), 5, "注释框不再生成关系线");
        assert!(
            d.relations
                .iter()
                .any(|r| r.kind == RelKind::Inherit && r.reverse)
        );
        assert!(d.relations.iter().any(|r| r.kind == RelKind::Realize));
        assert!(d.relations.iter().any(|r| r.a_mult.as_deref() == Some("1")));
        assert!(d.relations.iter().any(|r| r.b_mult.as_deref() == Some("*")));
        let note = d.classes.iter().find(|c| c.note).unwrap();
        let duck = d.classes.iter().position(|c| c.name == "Duck").unwrap();
        assert_eq!(note.owner, Some(duck));
    }

    #[test]
    fn layout_keeps_boxes_inside() {
        let d = parse(SAMPLE).unwrap();
        let l = layout(&d);
        assert!(l.width > 0. && l.height > 0.);
        for r in &l.rects {
            assert!(r.w > 0. && r.h > 0.);
            assert!(r.x >= 0. && r.x + r.w <= l.width + 0.5);
            assert!(r.y >= 0. && r.y + r.h <= l.height + 0.5);
        }
        for e in &l.edges {
            for p in &e.points {
                assert!(p.0 >= -0.5 && p.0 <= l.width + 0.5);
                assert!(p.1 >= -0.5 && p.1 <= l.height + 0.5);
            }
        }
    }

    #[test]
    fn superclass_is_above_subclass() {
        let d = parse(&showcase()).unwrap();
        let l = layout(&d);
        let animal = d.classes.iter().position(|c| c.name == "Animal").unwrap();
        let duck = d.classes.iter().position(|c| c.name == "Duck").unwrap();
        assert!(
            l.rects[animal].y + l.rects[animal].h < l.rects[duck].y,
            "父类必须排在子类上方（旧实现被 reverse 记号倒过来了）"
        );
    }

    #[test]
    fn class_boxes_have_three_compartments() {
        let d = parse(&showcase()).unwrap();
        let l = layout(&d);
        let plain = d.classes.iter().filter(|c| !c.note).count();
        assert_eq!(
            l.dividers.iter().map(Vec::len).sum::<usize>(),
            plain * 2,
            "每个类框两道分隔线"
        );
        assert_eq!(l.note_links.len(), 1, "注释框有一条到宿主类的虚线");
    }

    /// 不能出现「线从别的类框中间穿过去」——旧实现的 note 虚线就横穿了 Heart。
    #[test]
    fn edges_do_not_pass_through_other_boxes() {
        for src in [SAMPLE.to_string(), showcase()] {
            let d = parse(&src).unwrap();
            let l = layout(&d);
            for (ei, e) in l.edges.iter().enumerate() {
                for seg in e.points.windows(2) {
                    for (j, r) in l.rects.iter().enumerate() {
                        if j == e.a || j == e.b {
                            continue;
                        }
                        assert!(
                            !seg_hits_rect(seg[0], seg[1], *r),
                            "第 {ei} 条线的 {seg:?} 穿过了第 {j} 个框",
                        );
                    }
                }
            }
        }
    }

    /// 标签必须落在自己那条边上，不能像旧实现那样堆在画布中央。
    #[test]
    fn labels_sit_on_their_own_edge() {
        let d = parse(&showcase()).unwrap();
        let l = layout(&d);
        for (x, y, text, ei) in &l.labels {
            let pts = &l.edges[*ei].points;
            let near = pts.windows(2).any(|s| {
                let (a, c) = (s[0], s[1]);
                if (a.0 - c.0).abs() < 0.5 {
                    (x - a.0).abs() < 1.5 && *y >= a.1.min(c.1) - 1.5 && *y <= a.1.max(c.1) + 1.5
                } else if (a.1 - c.1).abs() < 0.5 {
                    (y - a.1).abs() < 1.5 && *x >= a.0.min(c.0) - 1.5 && *x <= a.0.max(c.0) + 1.5
                } else {
                    false
                }
            });
            assert!(near, "标签「{text}」没有贴在第 {ei} 条边上");
        }
    }

    /// 展示页的四张类图（含 LR 三层架构、泛型、聚合）都要能排布且不穿框。
    #[test]
    fn all_showcase_class_diagrams_route_cleanly() {
        let blocks = super::super::showcase_blocks("class");
        assert_eq!(blocks.len(), 4, "展示页类图数量变了，需要重新确认夹具");
        for (bi, block) in blocks.iter().enumerate() {
            let d = parse(block).unwrap_or_else(|| panic!("第 {bi} 张类图无法解析"));
            let l = layout(&d);
            for r in &l.rects {
                assert!(
                    r.x >= -0.5
                        && r.x + r.w <= l.width + 0.5
                        && r.y >= -0.5
                        && r.y + r.h <= l.height + 0.5,
                    "第 {bi} 张图有类框超出画布"
                );
            }
            for (ei, e) in l.edges.iter().enumerate() {
                for seg in e.points.windows(2) {
                    for (j, r) in l.rects.iter().enumerate() {
                        if j == e.a || j == e.b {
                            continue;
                        }
                        assert!(
                            !seg_hits_rect(seg[0], seg[1], *r),
                            "第 {bi} 张图第 {ei} 条线穿过了第 {j} 个框"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn direction_lr_still_places_everything() {
        let d = parse("classDiagram\n direction LR\n A --> B\n B --> C\n").unwrap();
        let l = layout(&d);
        let a = l.rects[0];
        let c = l.rects[2];
        assert!(c.x > a.x, "LR 方向下后继应排在右侧");
        assert!(l.rects.iter().all(|r| r.x + r.w <= l.width + 0.5));
    }

    /// 展示页「三层服务架构」：带 `direction LR` 与 `note for PaymentClient`。
    fn lr_showcase() -> String {
        super::super::showcase_blocks("class")
            .into_iter()
            .find(|b| b.contains("note for PaymentClient"))
            .expect("展示页应有 PaymentClient 类图")
    }

    /// 明/暗主题都不能再用 `border` 当类图线条颜色：它天生是弱化色阶。
    /// 同时锁住线宽，避免以后又改回 1px 导致深色背景下看不见。
    #[test]
    fn palette_is_visible_in_light_and_dark_themes() {
        use gpui_kit::component::theme::ThemeColor;
        for theme in [ThemeColor::light(), ThemeColor::dark()] {
            let p = palette(&theme);
            assert_ne!(p.stroke, theme.border, "线条不能复用弱对比的 border");
            assert_ne!(p.stroke, theme.muted_foreground, "线条不能复用弱化文字色");
            assert_ne!(p.text, theme.muted_foreground, "文字必须用前景色");
        }
        assert!(EDGE_W >= 1.5 && MARKER_W >= 1.5 && NOTE_LINK_W >= 1.5);
        assert!(DIVIDER_H > 1. && NOTE_FRAME_W >= 1.5);
    }

    /// LR 图里注记要像 Mermaid.js 那样落在宿主类左侧（而不是压在上一层类框里），
    /// 虚线从注记右边连到宿主类左边，且不穿过任何别的类框。
    #[test]
    fn lr_note_sits_left_of_owner_and_links_horizontally() {
        let d = parse(&lr_showcase()).unwrap();
        assert_eq!(d.direction, Direction::Lr);
        let l = layout(&d);
        let owner = d
            .classes
            .iter()
            .position(|c| c.name == "PaymentClient")
            .unwrap();
        let note = d
            .classes
            .iter()
            .position(|c| c.note && c.owner == Some(owner))
            .unwrap();
        let nr = l.rects[note];
        let or = l.rects[owner];
        assert!(
            nr.x + nr.w <= or.x + 0.5,
            "注记应在宿主左侧：note.x+w={} owner.x={}",
            nr.x + nr.w,
            or.x
        );
        let nc = nr.y + nr.h / 2.;
        assert!(
            nc >= or.y - 1. && nc <= or.y + or.h + 1.,
            "注记应与宿主类大致同高"
        );
        for (j, r) in l.rects.iter().enumerate() {
            if j != note {
                assert!(!overlap(nr, *r), "注记压住了第 {j} 个框");
            }
        }
        assert_eq!(l.note_links.len(), 1);
        let (a, c) = l.note_links[0];
        assert!((a.0 - (nr.x + nr.w)).abs() < 1.5, "虚线从注记右边出发");
        assert!((c.0 - or.x).abs() < 1.5, "虚线连到宿主类左边");
        for (j, r) in l.rects.iter().enumerate() {
            if j != note && j != owner {
                assert!(!seg_hits_rect(a, c, *r), "注记虚线穿过第 {j} 个框");
            }
        }
    }

    /// 临时诊断：把 LR 图的真实布局导出成纯文本，供校对脚本重绘 PNG。
    #[test]
    fn dump_lr_layout_for_review() {
        let d = parse(&lr_showcase()).unwrap();
        let l = layout(&d);
        let mut out = format!("SIZE {} {}\n", l.width, l.height);
        for (i, c) in d.classes.iter().enumerate() {
            let r = l.rects[i];
            out.push_str(&format!(
                "BOX {} {} {} {} {} {} | {} | {} | {}\n",
                c.name.replace(' ', "_"),
                if c.note { 1 } else { 0 },
                r.x,
                r.y,
                r.w,
                r.h,
                c.attrs.join("§"),
                c.methods.join("§"),
                c.annotations.join("§"),
            ));
        }
        for e in &l.edges {
            let pts = e
                .points
                .iter()
                .map(|p| format!("{},{}", p.0, p.1))
                .collect::<Vec<_>>()
                .join(";");
            out.push_str(&format!("EDGE {} {}\n", if e.dashed { 1 } else { 0 }, pts));
        }
        for (x, y, t, _) in &l.labels {
            out.push_str(&format!("LABEL {x} {y} {t}\n"));
        }
        for (x, y, t) in &l.mults {
            out.push_str(&format!("MULT {x} {y} {t}\n"));
        }
        for (a, c) in &l.note_links {
            out.push_str(&format!("NLINK {} {} {} {}\n", a.0, a.1, c.0, c.1));
        }
        std::fs::write("../../target/lr-layout.txt", out).unwrap();
    }

    /// 所有展示页类图 + 基础夹具：注记框不能和任何框重叠，虚线不能穿框。
    #[test]
    fn notes_never_overlap_or_cross_other_boxes() {
        let mut sources = vec![SAMPLE.to_string()];
        sources.extend(super::super::showcase_blocks("class"));
        for (bi, src) in sources.iter().enumerate() {
            let d = parse(src).unwrap();
            let l = layout(&d);
            let notes: Vec<usize> = d
                .classes
                .iter()
                .enumerate()
                .filter(|(_, c)| c.note)
                .map(|(i, _)| i)
                .collect();
            for &ni in &notes {
                for (j, r) in l.rects.iter().enumerate() {
                    if j != ni {
                        assert!(!overlap(l.rects[ni], *r), "第 {bi} 张图注记压住了第 {j} 个框");
                    }
                }
            }
            for (a, c) in &l.note_links {
                for (j, r) in l.rects.iter().enumerate() {
                    if notes.contains(&j) {
                        continue;
                    }
                    assert!(
                        !seg_hits_rect(*a, *c, *r),
                        "第 {bi} 张图注记虚线穿过第 {j} 个框"
                    );
                }
            }
        }
    }
}
