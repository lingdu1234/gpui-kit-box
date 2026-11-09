//! `sequenceDiagram` 时序图：解析、生命线布局与 GPUI 绘制。
//!
//! 支持 Mermaid 常用的消息、参与者、激活、注释与组合片段：
//!
//! ```text
//! sequenceDiagram
//!     autonumber
//!     actor U as 用户
//!     participant A as 前端
//!     participant B as 服务
//!     U->>+A: 登录
//!     A->>+B: 校验
//!     B-->>-A: 结果
//!     Note right of B: 缓存命中
//!     alt 成功
//!         A-->>U: 200
//!     else 失败
//!         A-->>U: 500
//!     end
//!     deactivate A
//! ```
//!
//! 布局是「参与者等距分列 + 事件逐行下排」：每个消息/注释占一行，组合片段
//! 用栈记录起止行，最后按行区间画外框；激活框同样按行区间沿生命线画。
//! 这套模型与流程图的图层布局完全不同，所以本模块自带布局，不复用 `layout.rs`。

use std::collections::HashMap;

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use super::common::{clean_label, text_width};

const PAD: f32 = 18.;
const HEAD_Y: f32 = 12.;
const HEAD_H: f32 = 36.;
const COL_GAP: f32 = 100.;
const MIN_COL_W: f32 = 112.;
const ROW_H: f32 = 52.;
const TOP_GAP: f32 = 30.;
const ACT_W: f32 = 12.;
const FONT: f32 = 12.;
/// 片段上下留白：开始/结束行前各插一段空隙，标签与消息不会互相压住。
const FRAG_HEADER: f32 = 20.;
const FRAG_FOOTER: f32 = 20.;
/// 相邻片段之间再留一点间距，边框与标签才分得开。
const FRAG_PAD: f32 = 6.;
/// 片段外框只包住实际参与的参与者，左右各留这么多空白。
const FRAG_SIDE_MARGIN: f32 = 30.;
/// 只有一个参与者的片段也保持最小宽度，守卫文字才放得下。
const FRAG_MIN_W: f32 = 160.;

// ---------------------------------------------------------------------------
// 图结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Arrow {
    Solid,
    SolidFilled,
    Dotted,
    DottedFilled,
    Cross,
    OpenAsync,
    Bidi,
}

impl Arrow {
    fn dashed(self) -> bool {
        matches!(self, Arrow::Dotted | Arrow::DottedFilled)
    }
}

#[derive(Clone)]
struct Participant {
    label: String,
    actor: bool,
}

#[derive(Clone)]
struct Message {
    from: usize,
    to: usize,
    text: String,
    arrow: Arrow,
    number: Option<usize>,
}

#[derive(Clone, Copy)]
enum NoteAt {
    Left(usize),
    Right(usize),
    Over(usize, usize),
}

#[derive(Clone)]
struct Note {
    at: NoteAt,
    text: String,
}

#[derive(Clone)]
struct Fragment {
    kind: String,
    label: String,
    depth: usize,
    start: usize,
    end: usize,
    splits: Vec<(usize, String)>,
}

/// 只有会占一行的事件才进列表：消息与注释。片段和激活框的起止行号
/// 直接记在 `fragments` / `activations` 里，渲染时按行区间画。
#[derive(Clone)]
enum Event {
    Message(Message, usize),
    Note(Note, usize),
}

#[derive(Clone)]
pub(super) struct Sequence {
    participants: Vec<Participant>,
    events: Vec<Event>,
    fragments: Vec<Fragment>,
    activations: Vec<(usize, usize, usize)>,
    rows: usize,
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

fn ensure(
    participants: &mut Vec<Participant>,
    index: &mut HashMap<String, usize>,
    id: &str,
    actor: bool,
) -> usize {
    let id = id.trim().trim_matches('"');
    if let Some(&ix) = index.get(id) {
        return ix;
    }
    let ix = participants.len();
    participants.push(Participant {
        label: id.to_string(),
        actor,
    });
    index.insert(id.to_string(), ix);
    ix
}

/// 找出消息行里最先出现的箭头，返回 `(字节位置, 长度, 类型)`。
fn find_arrow(line: &str) -> Option<(usize, usize, Arrow)> {
    // 长箭头排在前面：同一起点时优先匹配 `-->>` 而不是 `-->`。
    const ARROWS: &[(&str, Arrow)] = &[
        ("<<-->>", Arrow::Bidi),
        ("<<->>", Arrow::Bidi),
        ("-->>", Arrow::DottedFilled),
        ("->>", Arrow::SolidFilled),
        ("--x", Arrow::Cross),
        ("-x", Arrow::Cross),
        ("--)", Arrow::OpenAsync),
        ("-)", Arrow::OpenAsync),
        ("-->", Arrow::Dotted),
        ("->", Arrow::Solid),
    ];
    let mut best: Option<(usize, usize, Arrow)> = None;
    for &(token, kind) in ARROWS {
        if let Some(at) = line.find(token)
            && best.is_none_or(|(pos, _, _)| at < pos)
        {
            best = Some((at, token.len(), kind));
        }
    }
    best
}

fn is_fragment_start(lower: &str) -> Option<&'static str> {
    let rest = lower.trim();
    for (word, kind) in [
        ("loop", "loop"),
        ("alt", "alt"),
        ("opt", "opt"),
        ("par", "par"),
        ("critical", "critical"),
        ("break", "break"),
        ("rect", "rect"),
        ("box", "box"),
    ] {
        if rest == word || rest.starts_with(&format!("{word} ")) {
            return Some(kind);
        }
    }
    None
}

pub(super) fn parse(source: &str) -> Option<Sequence> {
    let lines: Vec<&str> = source.lines().collect();
    let mut participants: Vec<Participant> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut events: Vec<Event> = Vec::new();
    let mut fragments: Vec<Fragment> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut active: HashMap<usize, usize> = HashMap::new();
    let mut activations: Vec<(usize, usize, usize)> = Vec::new();
    let mut row = 0usize;
    let mut number = 0usize;
    let mut autonumber = false;
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("sequencediagram")
            || lower == "autonumber"
            || lower.starts_with("title ")
            || lower.starts_with("link ")
            || lower.starts_with("links ")
            || lower.starts_with("properties ")
        {
            autonumber |= lower == "autonumber";
            continue;
        }

        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            let actor = lower.starts_with("actor ");
            let rest = line[line.find(' ').unwrap() + 1..].trim();
            let (id, display) = match rest.split_once(" as ") {
                Some((id, display)) => (id.trim(), clean_label(display)),
                None => (rest, clean_label(rest)),
            };
            let ix = ensure(&mut participants, &mut index, id, actor);
            participants[ix].label = display;
            continue;
        }

        if lower == "activate" || lower.starts_with("activate ") {
            let who = ensure(&mut participants, &mut index, &line[8..].trim(), false);
            active.entry(who).or_insert(row);
            continue;
        }
        if lower.starts_with("deactivate ") {
            let who = ensure(&mut participants, &mut index, &line[10..].trim(), false);
            if let Some(start) = active.remove(&who) {
                activations.push((who, start, row));
            }
            continue;
        }

        if lower.starts_with("note ") {
            let rest = &line[5..];
            let (head, text) = match rest.split_once(':') {
                Some((head, text)) => (head.trim(), clean_label(text)),
                None => {
                    // 多行注释：一直读到 `end note`。
                    let head = rest.trim();
                    let mut buf = Vec::new();
                    while i < lines.len() {
                        let next = lines[i].trim();
                        i += 1;
                        if next.to_ascii_lowercase().starts_with("end note") {
                            break;
                        }
                        if !next.is_empty() {
                            buf.push(clean_label(next));
                        }
                    }
                    (head, buf.join(" "))
                }
            };
            let words: Vec<&str> = head.split_whitespace().collect();
            let at = if words.first().is_some_and(|w| *w == "left") {
                let who = words.last().copied().unwrap_or_default();
                NoteAt::Left(ensure(&mut participants, &mut index, who, false))
            } else if words.first().is_some_and(|w| *w == "right") {
                let who = words.last().copied().unwrap_or_default();
                NoteAt::Right(ensure(&mut participants, &mut index, who, false))
            } else {
                // `Note over A,B: ...`，也可能只写一个参与者。
                let ids = words.last().copied().unwrap_or_default();
                let mut pair = ids.split(',');
                let a = ensure(
                    &mut participants,
                    &mut index,
                    pair.next().unwrap_or(""),
                    false,
                );
                let b = match pair.next() {
                    Some(b) => ensure(&mut participants, &mut index, b, false),
                    None => a,
                };
                NoteAt::Over(a, b)
            };
            events.push(Event::Note(Note { at, text }, row));
            row += 1;
            continue;
        }

        if let Some(kind) = is_fragment_start(&lower) {
            let frag = Fragment {
                kind: kind.to_string(),
                // `loop 每 5 秒` 这类标签要取原文（保留大小写），不能用 lower。
                label: clean_label(line[kind.len().min(line.len())..].trim()),
                depth: stack.len(),
                start: row,
                end: row,
                splits: Vec::new(),
            };
            let ix = fragments.len();
            fragments.push(frag);
            stack.push(ix);
            continue;
        }

        if lower == "else"
            || lower.starts_with("else ")
            || lower == "and"
            || lower.starts_with("and ")
            || lower == "option"
            || lower.starts_with("option ")
        {
            if let Some(&ix) = stack.last() {
                let word = lower.split_whitespace().next().unwrap_or("");
                let label = clean_label(line[word.len().min(line.len())..].trim());
                fragments[ix].splits.push((row, label));
            }
            continue;
        }

        if lower == "end" {
            if let Some(ix) = stack.pop() {
                fragments[ix].end = row;
            }
            continue;
        }

        // ---- 消息 ----
        let Some((at, len, arrow)) = find_arrow(line) else {
            continue; // 不认识的行忽略，不吞整张图
        };
        let from_id = line[..at].trim();
        let mut tail = line[at + len..].trim();
        let mut activate = false;
        let mut deactivate = false;
        if let Some(rest) = tail.strip_prefix('+') {
            activate = true;
            tail = rest.trim_start();
        } else if let Some(rest) = tail.strip_prefix('-') {
            deactivate = true;
            tail = rest.trim_start();
        }
        let (to_id, text) = match tail.split_once(':') {
            Some((to, text)) => (to.trim(), clean_label(text)),
            None => (tail.trim(), String::new()),
        };
        if from_id.is_empty() || to_id.is_empty() {
            continue;
        }
        let from = ensure(&mut participants, &mut index, from_id, false);
        let to = ensure(&mut participants, &mut index, to_id, false);
        if autonumber {
            number += 1;
        }
        if activate {
            active.entry(to).or_insert(row);
        }
        if deactivate {
            // `A-->>-B` 里的 `-` 关闭的是发送方：Mermaid 的惯用写法
            // `Alice->>+B` / `B-->>-Alice` 就是这么配对的。
            if let Some(start) = active.remove(&from) {
                activations.push((from, start, row));
            }
        }
        events.push(Event::Message(
            Message {
                from,
                to,
                text,
                arrow,
                number: autonumber.then_some(number),
            },
            row,
        ));
        row += 1;
    }

    // 未闭合的片段（少写 end）按当前行收尾，避免渲染出没有下边框的框。
    for ix in stack.drain(..) {
        fragments[ix].end = row;
    }
    // 未显式 deactivate 的激活框收到最后一行，语义上「一直激活到图结束」。
    for (who, start) in active.drain() {
        activations.push((who, start, row.max(start + 1)));
    }

    if participants.is_empty() || row == 0 {
        return None;
    }
    Some(Sequence {
        participants,
        events,
        fragments,
        activations,
        rows: row,
    })
}

// ---------------------------------------------------------------------------
// 布局
// ---------------------------------------------------------------------------

struct SeqLayout {
    width: f32,
    height: f32,
    xs: Vec<f32>,
    row_y: Vec<f32>,
    notes: Vec<(Note, usize, f32, f32, f32)>, // note, row, x, y, w
    messages: Vec<(Message, usize)>,
    fragments: Vec<FragBox>,
    activations: Vec<(f32, f32, f32, f32)>,
    /// 底部复刻的参与者框/小人所在的 y。
    bottom_head_y: f32,
}

/// 组合片段外框：标签拆成 `kind`（左上角标签）与 `label`（首段守卫）。
#[derive(Clone)]
struct FragBox {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    kind: String,
    label: String,
    splits: Vec<(f32, String)>,
}

fn layout(seq: &Sequence) -> SeqLayout {
    let col_w = seq
        .participants
        .iter()
        .map(|p| text_width(&p.label, FONT) + 30.)
        .fold(MIN_COL_W, f32::max)
        + COL_GAP;
    let mut left_pad = PAD;
    // 左侧注释可能探出参与者列，先把整张图往右推。
    for event in &seq.events {
        if let Event::Note(note, _) = event
            && let NoteAt::Left(_) = note.at
        {
            left_pad = left_pad.max(text_width(&note.text, 11.) + 24. + PAD);
        }
    }
    let xs: Vec<f32> = (0..seq.participants.len())
        .map(|i| left_pad + col_w / 2. + i as f32 * col_w)
        .collect();
    let top = HEAD_Y + HEAD_H + TOP_GAP;

    // 片段开始/结束都在对应行前插一段空隙：标签、守卫与消息各占一行，
    // 复杂图（片段嵌套、连续片段）不会挤在一起。
    let mut starts = vec![0usize; seq.rows + 1];
    let mut ends = vec![0usize; seq.rows + 1];
    for f in &seq.fragments {
        starts[f.start.min(seq.rows)] += 1;
        ends[f.end.min(seq.rows)] += 1;
    }
    let mut row_top = Vec::with_capacity(seq.rows + 1);
    let mut cursor = top;
    for r in 0..=seq.rows {
        cursor += starts[r] as f32 * FRAG_HEADER + ends[r] as f32 * FRAG_FOOTER;
        row_top.push(cursor);
        cursor += ROW_H;
    }
    // 片段前一行内容的底边（row_top 已经减掉本行的片段留白）。
    let prev_bottom =
        |r: usize| row_top[r] - starts[r] as f32 * FRAG_HEADER - ends[r] as f32 * FRAG_FOOTER;
    let row_y: Vec<f32> = row_top
        .iter()
        .take(seq.rows)
        .map(|top| top + ROW_H / 2.)
        .collect();

    let mut width = left_pad + col_w * seq.participants.len() as f32 + PAD;
    let mut notes = Vec::new();
    let mut messages = Vec::new();
    for event in &seq.events {
        match event {
            Event::Message(msg, row) => {
                let label = message_label(msg);
                let mid = (xs[msg.from] + xs[msg.to]) / 2.;
                width = width.max(mid + text_width(&label, 11.) / 2. + PAD);
                messages.push((msg.clone(), *row));
            }
            Event::Note(note, row) => {
                let w = text_width(&note.text, 11.) + 20.;
                let x = match note.at {
                    NoteAt::Left(i) => xs[i] - col_w / 2. - 8. - w,
                    NoteAt::Right(i) => xs[i] + 24.,
                    NoteAt::Over(a, b) => (xs[a] + xs[b]) / 2. - w / 2.,
                };
                let x = x.max(PAD);
                width = width.max(x + w + PAD);
                notes.push((note.clone(), *row, x, row_y[*row] - 13., w));
            }
        }
    }

    // 每个片段只框住它内部事件实际涉及的参与者，而不是整张图的宽度。
    let fragment_spans: Vec<Option<(usize, usize)>> = seq
        .fragments
        .iter()
        .map(|f| {
            let mut span: Option<(usize, usize)> = None;
            for event in &seq.events {
                let (row, a, b) = match event {
                    Event::Message(msg, row) => (*row, msg.from, msg.to),
                    Event::Note(note, row) => {
                        let (a, b) = match note.at {
                            NoteAt::Left(i) | NoteAt::Right(i) => (i, i),
                            NoteAt::Over(a, b) => (a, b),
                        };
                        (*row, a, b)
                    }
                };
                if row < f.start || row >= f.end {
                    continue;
                }
                span = Some(match span {
                    None => (a.min(b), a.max(b)),
                    Some((lo, hi)) => (lo.min(a).min(b), hi.max(a).max(b)),
                });
            }
            span
        })
        .collect();

    let fragments = seq
        .fragments
        .iter()
        .zip(fragment_spans)
        .map(|(f, span)| {
            let inset = f.depth as f32 * 9.;
            let (mut left, mut right) = match span {
                Some((lo, hi)) => (xs[lo] - FRAG_SIDE_MARGIN, xs[hi] + FRAG_SIDE_MARGIN),
                None => (PAD - 10., width - PAD + 10.),
            };
            // 参与列太少时撑到最小宽度，并保持居中。
            if right - left < FRAG_MIN_W {
                let center = (left + right) / 2.;
                left = center - FRAG_MIN_W / 2.;
                right = center + FRAG_MIN_W / 2.;
            }
            let x1 = left.max(PAD - 10.) + inset;
            let x2 = right.min(width - PAD + 10.) - inset;
            let y1 = prev_bottom(f.start) + ends[f.start] as f32 * FRAG_FOOTER + FRAG_PAD;
            // 底边落在结束行前的留白里，相邻片段之间自然留出空隙。
            let y2 =
                (prev_bottom(f.end) + ends[f.end] as f32 * FRAG_FOOTER - FRAG_PAD).max(y1 + 26.);
            let splits = f
                .splits
                .iter()
                .map(|(row, label)| (row_top[*row], label.clone()))
                .collect();
            FragBox {
                x1,
                y1,
                x2,
                y2,
                kind: f.kind.clone(),
                label: f.label.clone(),
                splits,
            }
        })
        .collect();

    let activations = seq
        .activations
        .iter()
        .map(|(who, start, end)| {
            let y1 = row_y.get(*start).copied().unwrap_or(top) - 9.;
            let y2 = row_y.get(end.saturating_sub(1)).copied().unwrap_or(y1) + 9.;
            (xs[*who] - ACT_W / 2., y1, ACT_W, (y2 - y1).max(14.))
        })
        .collect();

    // 时序图底部重复一遍参与者，生命线收在底框上沿。
    let bottom_head_y = row_top[seq.rows] + 20.;

    SeqLayout {
        width,
        height: bottom_head_y + HEAD_H + PAD,
        xs,
        row_y,
        notes,
        messages,
        fragments,
        activations,
        bottom_head_y,
    }
}

/// 消息文字只用正文；编号改由生命线上的圆圈表示。
fn message_label(msg: &Message) -> String {
    msg.text.clone()
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

pub(super) fn render(seq: &Sequence, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (border, muted, line, bg) = (
        theme.border,
        theme.muted,
        theme.muted_foreground,
        theme.background,
    );
    let lay = layout(seq);

    // 画进 canvas 的数据先 cloned 出来，闭包要 'static。
    let data = DrawData {
        xs: lay.xs.clone(),
        row_y: lay.row_y.clone(),
        messages: lay.messages.clone(),
        fragments: lay.fragments.clone(),
        activations: lay.activations.clone(),
        actors: seq
            .participants
            .iter()
            .enumerate()
            .filter(|(_, p)| p.actor)
            .map(|(i, _)| lay.xs[i])
            .collect(),
        head_bottom: HEAD_Y + HEAD_H,
        bottom_head_y: lay.bottom_head_y,
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

            // 生命线：实线，从顶部角色/参与者框一直落到下方复刻的框上沿。
            for &x in &data.xs {
                let mut b = PathBuilder::stroke(px(1.));
                seg!(b, (x, data.head_bottom), (x, data.bottom_head_y));
                if let Ok(path) = b.build() {
                    window.paint_path(path, line);
                }
            }

            // 组合片段外框 + 分隔线：整体用虚线，与实线生命线区分。
            for frag in &data.fragments {
                let (x1, y1, x2, y2) = (frag.x1, frag.y1, frag.x2, frag.y2);
                let mut b = PathBuilder::stroke(px(1.));
                let corners = [(x1, y1), (x2, y1), (x2, y2), (x1, y2)];
                for k in 0..4 {
                    super::common::dashed_line(&mut b, corners[k], corners[(k + 1) % 4], &at);
                }
                for (sy, _) in &frag.splits {
                    super::common::dashed_line(&mut b, (x1, *sy), (x2, *sy), &at);
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, line);
                }
            }

            // 激活框：只填充，不描边，像截图里压在生命线上的灰色长条。
            for (x, y, w, h) in &data.activations {
                let (x, y, w, h) = (*x, *y, *w, *h);
                let mut fill = PathBuilder::fill();
                fill.move_to(at(&(x, y)));
                fill.line_to(at(&(x + w, y)));
                fill.line_to(at(&(x + w, y + h)));
                fill.line_to(at(&(x, y + h)));
                fill.close();
                if let Ok(path) = fill.build() {
                    window.paint_path(path, muted);
                }
            }

            // 消息线、箭头、自消息回环。
            for (msg, row) in &data.messages {
                let y = data.row_y[*row];
                let (x1, x2) = (data.xs[msg.from], data.xs[msg.to]);
                let (tail_x, right) = if x2 >= x1 {
                    (x2 - 6., true)
                } else {
                    (x2 + 6., false)
                };
                let mut b = PathBuilder::stroke(px(1.));
                if msg.from == msg.to {
                    // 自发消息：向右绕一个小回环再指回自己。
                    let loop_x = x1 + 34.;
                    seg!(b, (x1 + 4., y - 7.), (loop_x, y - 7.));
                    seg!(b, (loop_x, y - 7.), (loop_x, y + 8.));
                    seg!(b, (loop_x, y + 8.), (x1 + 6., y + 8.));
                } else if msg.arrow.dashed() {
                    super::common::dashed_line(&mut b, (x1, y), (tail_x, y), &at);
                } else {
                    seg!(b, (x1, y), (tail_x, y));
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, line);
                }

                let tip = if msg.from == msg.to {
                    (x1 + 6., y + 8.)
                } else {
                    (x2, y)
                };
                let dir = if msg.from == msg.to || !right {
                    2u8
                } else {
                    0u8
                };
                match msg.arrow {
                    Arrow::Cross => {
                        let mut cross = PathBuilder::stroke(px(1.2));
                        seg!(cross, (tip.0 - 5., tip.1 - 5.), (tip.0 + 5., tip.1 + 5.));
                        seg!(cross, (tip.0 - 5., tip.1 + 5.), (tip.0 + 5., tip.1 - 5.));
                        if let Ok(path) = cross.build() {
                            window.paint_path(path, line);
                        }
                    }
                    Arrow::Bidi => {
                        let mut heads = PathBuilder::fill();
                        for (x, d) in [(x1, 2u8), (x2, 0u8)] {
                            let t = super::common::arrow_at(x, y, d);
                            heads.move_to(at(&t[0]));
                            heads.line_to(at(&t[1]));
                            heads.line_to(at(&t[2]));
                            heads.close();
                        }
                        if let Ok(path) = heads.build() {
                            window.paint_path(path, line);
                        }
                    }
                    // 只有实线消息用实心三角；虚线回复按截图画细的开放箭头。
                    Arrow::SolidFilled => {
                        let t = super::common::arrow_at(tip.0, tip.1, dir);
                        let mut head = PathBuilder::fill();
                        head.move_to(at(&t[0]));
                        head.line_to(at(&t[1]));
                        head.line_to(at(&t[2]));
                        head.close();
                        if let Ok(path) = head.build() {
                            window.paint_path(path, line);
                        }
                    }
                    _ => {
                        let back = if dir == 0 { -9. } else { 9. };
                        let mut open = PathBuilder::stroke(px(1.2));
                        seg!(open, (tip.0 + back, tip.1 - 4.), tip);
                        seg!(open, (tip.0 + back, tip.1 + 4.), tip);
                        if let Ok(path) = open.build() {
                            window.paint_path(path, line);
                        }
                    }
                }
            }

            // actor 的小人：头（12 边形近似圆）+ 身体 + 手臂 + 腿；上下各画一次。
            for &x in &data.actors {
                for base in [HEAD_Y + 8., data.bottom_head_y + 8.] {
                    let cx = x;
                    let cy = base;
                    let mut b = PathBuilder::stroke(px(1.2));
                    for k in 0..12 {
                        let a0 = k as f32 * std::f32::consts::TAU / 12.;
                        let a1 = (k + 1) as f32 * std::f32::consts::TAU / 12.;
                        b.move_to(at(&(cx + 4. * a0.cos(), cy - 6. + 4. * a0.sin())));
                        b.line_to(at(&(cx + 4. * a1.cos(), cy - 6. + 4. * a1.sin())));
                    }
                    seg!(b, (cx, cy - 2.), (cx, cy + 8.));
                    seg!(b, (cx - 6., cy + 2.), (cx + 6., cy + 2.));
                    seg!(b, (cx, cy + 8.), (cx - 5., cy + 14.));
                    seg!(b, (cx, cy + 8.), (cx + 5., cy + 14.));
                    if let Ok(path) = b.build() {
                        window.paint_path(path, line);
                    }
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

    // 顶部与底部各一份参与者：actor 画小人与名字，其余画圆角框。
    for (i, p) in seq.participants.iter().enumerate() {
        let w = (text_width(&p.label, FONT) + 30.).max(MIN_COL_W);
        if p.actor {
            for top in [HEAD_Y, lay.bottom_head_y] {
                container = container.child(
                    div()
                        .absolute()
                        .left(px(lay.xs[i] - 40.))
                        .top(px(top + 24.))
                        .w(px(80.))
                        .text_size(px(FONT))
                        .text_center()
                        .text_color(theme.foreground)
                        .child(p.label.clone()),
                );
            }
            continue;
        }
        for top in [HEAD_Y, lay.bottom_head_y] {
            container = container.child(
                div()
                    .absolute()
                    .left(px(lay.xs[i] - w / 2.))
                    .top(px(top))
                    .w(px(w))
                    .h(px(HEAD_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(border)
                    .bg(muted)
                    .text_size(px(FONT))
                    .text_color(theme.foreground)
                    .child(div().text_center().px_2().child(p.label.clone())),
            );
        }
    }

    // 组合片段的左上角类型标签与首段 / else 守卫；守卫居中显示。
    for frag in &lay.fragments {
        container = container.child(
            h_flex()
                .absolute()
                .left(px(frag.x1))
                .top(px(frag.y1 - 9.))
                .px(px(6.))
                .py(px(1.))
                .rounded(px(4.))
                .bg(muted)
                .text_size(px(10.))
                .text_color(line)
                .child(frag.kind.clone()),
        );
        let guard_left =
            |text: &str| ((frag.x1 + frag.x2 - text_width(text, 11.)) / 2.).max(frag.x1 + 40.);
        if !frag.label.is_empty() {
            let text = format!("[{}]", frag.label);
            container = container.child(
                div()
                    .absolute()
                    .left(px(guard_left(&text)))
                    .top(px(frag.y1 + 2.))
                    .px(px(2.))
                    .bg(bg)
                    .text_size(px(11.))
                    .text_color(line)
                    .child(text),
            );
        }
        for (sy, text) in &frag.splits {
            if text.is_empty() {
                continue;
            }
            let text = format!("[{text}]");
            container = container.child(
                div()
                    .absolute()
                    .left(px(guard_left(&text)))
                    .top(px(*sy + 2.))
                    .px(px(2.))
                    .bg(bg)
                    .text_size(px(11.))
                    .text_color(line)
                    .child(text),
            );
        }
    }

    // 序号圆圈：压在发送方生命线与消息线的交点上。
    for (msg, row) in &lay.messages {
        let Some(number) = msg.number else {
            continue;
        };
        container = container.child(
            div()
                .absolute()
                .left(px(lay.xs[msg.from] - 8.))
                .top(px(lay.row_y[*row] - 8.))
                .w(px(16.))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .border_1()
                .border_color(line)
                .bg(bg)
                .text_size(px(9.))
                .text_color(theme.foreground)
                .child(format!("{number}")),
        );
    }

    // 消息标签
    for (msg, row) in &lay.messages {
        let label = message_label(msg);
        if label.is_empty() {
            continue;
        }
        let w = text_width(&label, 11.) + 10.;
        let mid = (lay.xs[msg.from] + lay.xs[msg.to]) / 2.;
        let x = (mid - w / 2.).clamp(0., (lay.width - w).max(0.));
        container = container.child(
            h_flex()
                .absolute()
                .left(px(x))
                .top(px(lay.row_y[*row] - 19.))
                .px(px(3.))
                .rounded(px(3.))
                .bg(bg)
                .text_size(px(11.))
                .text_color(line)
                .child(label),
        );
    }

    // 注释：圆角灰框，压在连线之上。
    for (note, _, x, y, w) in &lay.notes {
        container = container.child(
            div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .w(px(*w))
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .bg(muted)
                .text_size(px(11.))
                .text_color(line)
                .child(div().text_center().child(note.text.clone())),
        );
    }

    container.into_any_element()
}

struct DrawData {
    xs: Vec<f32>,
    row_y: Vec<f32>,
    messages: Vec<(Message, usize)>,
    fragments: Vec<FragBox>,
    activations: Vec<(f32, f32, f32, f32)>,
    actors: Vec<f32>,
    head_bottom: f32,
    bottom_head_y: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"sequenceDiagram
    autonumber
    actor U as 用户
    participant A as 前端
    participant B as 服务
    U->>+A: 登录
    A->>+B: 校验
    B-->>-A: 结果
    Note right of B: 缓存命中
    alt 成功
        A-->>U: 200
    else 失败
        A-->>U: 500
    end
    deactivate A
    loop 重试
        A-)B: ping
    end
"#;

    #[test]
    fn parses_participants_messages_and_fragments() {
        let seq = parse(SAMPLE).expect("应可解析");
        assert_eq!(seq.participants.len(), 3);
        assert_eq!(seq.participants[0].label, "用户");
        assert!(seq.participants[0].actor);
        // 6 条消息 + 1 条注释，各占一行。
        assert_eq!(seq.rows, 7);
        let messages: Vec<_> = seq
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message(m, _) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].number, Some(1));
        assert!(messages[1].arrow == Arrow::SolidFilled);
        assert!(messages[2].arrow == Arrow::DottedFilled);
        assert_eq!(seq.fragments.len(), 2);
        assert_eq!(seq.fragments[0].splits.len(), 1);
        assert!(!seq.activations.is_empty(), "+/- 应产生激活区间");
    }

    #[test]
    fn layout_keeps_everything_inside() {
        let seq = parse(SAMPLE).unwrap();
        let l = layout(&seq);
        assert!(l.width > 0. && l.height > 0.);
        assert_eq!(l.row_y.len(), seq.rows);
        for (_, row, x, y, w) in &l.notes {
            assert!(*x >= 0. && x + w <= l.width + 0.5);
            assert!(*y >= 0. && y + 26. <= l.height + 0.5);
            assert!(*row < seq.rows);
        }
        for frag in &l.fragments {
            assert!(frag.x2 <= l.width + 0.5 && frag.y2 <= l.height + 0.5);
        }
        for (msg, row) in &l.messages {
            assert!(msg.from < seq.participants.len() && msg.to < seq.participants.len());
            assert!(*row < seq.rows);
        }
    }

    /// 展示页的四张时序图都必须让激活框显式闭合。
    /// 旧示例里的 `B-->>-A` 会让 Mermaid 报 "Trying to inactivate an inactive
    /// participant (B)"：`-` 关闭的是发送方，而 B 从未被 `+` 激活。
    #[test]
    fn all_showcase_activations_are_balanced() {
        let blocks = super::super::showcase_blocks("sequence");
        assert_eq!(blocks.len(), 4, "展示页时序图数量变了，需要重新确认夹具");
        for (ix, block) in blocks.iter().enumerate() {
            let seq = parse(block).unwrap_or_else(|| panic!("第 {ix} 张时序图无法解析"));
            for &(who, start, end) in &seq.activations {
                assert!(
                    end < seq.rows,
                    "第 {ix} 张图里参与者 {who} 的激活框（{start}→{end}）没有显式关闭"
                );
            }
        }
    }

    #[test]
    fn shortcut_activation_pairs_with_source() {
        let seq = parse(SAMPLE).unwrap();
        assert_eq!(seq.activations.len(), 2, "A 与 B 各有一段激活区间");
        assert!(
            seq.activations
                .iter()
                .all(|&(_, s, e)| s < e && e < seq.rows)
        );
        // 旧展示页的错例：`-` 想关掉从未激活的 B，A 的激活框就悬空到图末。
        let bad = "sequenceDiagram\n    U->>+A: hi\n    B-->>-A: bye";
        let parsed = parse(bad).expect("解析器对不配对写法保持宽容，不会吞整张图");
        assert!(
            parsed
                .activations
                .iter()
                .any(|&(_, _, end)| end == parsed.rows),
            "不配对的 - 应留下延续到图末的悬空激活框，供平衡性测试抓出来"
        );
    }

    #[test]
    fn fragment_frames_cover_only_involved_participants() {
        let blocks = super::super::showcase_blocks("sequence");
        let seq = parse(&blocks[1]).expect("登录示例应可解析");
        let l = layout(&seq);
        let alt = &l.fragments[0];
        let loop_frag = &l.fragments[1];
        assert!(alt.x2 < l.xs[2], "alt 不应框住未参与的服务列");
        assert!(loop_frag.x1 > l.xs[0], "loop 不应框住未参与的用户列");
        assert!(alt.x1 >= l.xs[0] - FRAG_SIDE_MARGIN - 0.5);
        assert!(
            alt.y2 + 6. <= loop_frag.y1,
            "上下相邻的片段框之间要留出空隙（alt 底 {:.1}，loop 顶 {:.1}）",
            alt.y2,
            loop_frag.y1
        );
    }

    #[test]
    fn unknown_or_disabled_diagrams_are_safe() {
        assert!(parse("sequenceDiagram\n").is_none());
        assert!(parse("sequenceDiagram\n  A->>B: 只有一条\n").is_some());
    }
}
