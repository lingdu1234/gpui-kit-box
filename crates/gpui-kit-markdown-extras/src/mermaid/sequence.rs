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
//!
//! 纵向节奏由几段「带」拼出来：片段开头（类型标签 + 首段守卫）、`else` / `and`
//! 分隔守卫、消息行、片段结尾。守卫必须独占一条带——早期版本把分隔守卫直接
//! 画在行带顶，11px 文字与下一行消息文字只差 5px，`[支付失败]` 就压在
//! 「返回失败码」上。所有文字块的坐标都在布局层算好（`SeqLayout::labels` /
//! `FragBox`），`texts_never_cover_each_other` 用同一份几何做两两相交检查。
//!
//! 配色与其他图共用 [`super::palette`]：生命线 / 消息线 / 箭头是强调色，
//! 参与者框是淡表面，文字分正文与次级两档。

use std::collections::HashMap;

use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point,
};

use super::common::{clean_label, text_width};
use super::palette;

const PAD: f32 = 18.;
const HEAD_Y: f32 = 12.;
const HEAD_H: f32 = 36.;
const COL_GAP: f32 = 100.;
const MIN_COL_W: f32 = 112.;
const ROW_H: f32 = 56.;
const TOP_GAP: f32 = 30.;
const ACT_W: f32 = 12.;
const FONT: f32 = 12.;
/// 片段开头留白：放得下类型标签（alt/par/…）与首段守卫，下面还留一条消息文字的
/// 空间。以前只有 20px，守卫底边离第一条消息的文字只有 5px，视觉上像压在字上。
const FRAG_HEADER: f32 = 32.;
const FRAG_FOOTER: f32 = 24.;
/// 分隔（`else` / `and`）前的留白：守卫独立占一条横带，不会与这一行的
/// 消息文字抢同一行。
const FRAG_SPLIT: f32 = 26.;
/// 相邻片段之间再留一点间距，边框与标签才分得开。
const FRAG_PAD: f32 = 8.;
/// 片段外框只包住实际参与的参与者，左右各留这么多空白。
const FRAG_SIDE_MARGIN: f32 = 30.;
/// 只有一个参与者的片段也保持最小宽度：类型标签 + 守卫要排得开。
const FRAG_MIN_W: f32 = 180.;
/// 消息文字底边到箭头线的距离：文字压在线上方，留一点空隙。
const LABEL_DY: f32 = 24.;

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

/// 找出消息行里最先出现的箭头，返回 `(字节位置, 长度, 类型, 是否反向)`。
/// 反向箭头（`<-` / `<<-` …）箭头朝左：发送方在右、接收方在左，解析时交换两端。
fn find_arrow(line: &str) -> Option<(usize, usize, Arrow, bool)> {
    // 长箭头排在前面：同一起点时优先匹配 `-->>` 而不是 `-->`。
    // 双向 `<<->>` / `<<-->>` 起点更靠前，会先于反向 `<-` 命中，不会被误判。
    const ARROWS: &[(&str, Arrow, bool)] = &[
        ("<<-->>", Arrow::Bidi, false),
        ("<<->>", Arrow::Bidi, false),
        ("-->>", Arrow::DottedFilled, false),
        ("->>", Arrow::SolidFilled, false),
        ("--x", Arrow::Cross, false),
        ("-x", Arrow::Cross, false),
        ("--)", Arrow::OpenAsync, false),
        ("-)", Arrow::OpenAsync, false),
        ("-->", Arrow::Dotted, false),
        ("->", Arrow::Solid, false),
        ("<<--", Arrow::DottedFilled, true),
        ("<<-", Arrow::SolidFilled, true),
        ("<--", Arrow::Dotted, true),
        ("<-", Arrow::Solid, true),
    ];
    let mut best: Option<(usize, usize, Arrow, bool)> = None;
    for &(token, kind, reverse) in ARROWS {
        if let Some(at) = line.find(token)
            && best.is_none_or(|(pos, _, _, _)| at < pos)
        {
            best = Some((at, token.len(), kind, reverse));
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
    let mut step = 1usize;
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
            || lower.starts_with("title ")
            || lower.starts_with("link ")
            || lower.starts_with("links ")
            || lower.starts_with("properties ")
        {
            continue;
        }

        // `autonumber` / `autonumber 10` / `autonumber 10 5`：起始值与步长。
        if lower == "autonumber" || lower.starts_with("autonumber ") {
            let args: Vec<usize> = line["autonumber".len()..]
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            number = args.first().copied().unwrap_or(1);
            step = args.get(1).copied().unwrap_or(1).max(1);
            autonumber = true;
            continue;
        }

        // `create participant X as Y` / `create actor X`：中途登场的参与者。
        // 本布局把所有参与者统一排在顶/底，这里只负责登记，不画「中途出现」动画。
        if lower.starts_with("create ") {
            let rest = line[7..].trim();
            let rest_lower = rest.to_ascii_lowercase();
            let (actor, body) = if rest_lower.starts_with("participant ") {
                (false, rest[12..].trim())
            } else if rest_lower.starts_with("actor ") {
                (true, rest[6..].trim())
            } else {
                (false, rest)
            };
            let (id, display) = match body.split_once(" as ") {
                Some((id, display)) => (id.trim(), clean_label(display)),
                None => (body, clean_label(body)),
            };
            let ix = ensure(&mut participants, &mut index, id, actor);
            participants[ix].label = display;
            continue;
        }
        // `destroy X`：退场的参与者。登记一下保证它仍在图上，不画删除标记。
        if lower.starts_with("destroy ") {
            let who = line[8..].trim();
            let id = who.split_once(" as ").map(|(id, _)| id.trim()).unwrap_or(who);
            ensure(&mut participants, &mut index, id, false);
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
        let Some((at, len, arrow, reverse)) = find_arrow(line) else {
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
        // 反向箭头：右端才是发送方。交换后 from 在右、to 在左，
        // 渲染层按 x 坐标自然把箭头画成朝左。
        let (from_id, to_id) = if reverse {
            (to_id, from_id)
        } else {
            (from_id, to_id)
        };
        let from = ensure(&mut participants, &mut index, from_id, false);
        let to = ensure(&mut participants, &mut index, to_id, false);
        let msg_number = autonumber.then(|| {
            let n = number;
            number += step;
            n
        });
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
                number: msg_number,
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
    /// 消息文字块 `(左边距, 上边距, 文本)`。
    labels: Vec<(f32, f32, String)>,
    fragments: Vec<FragBox>,
    activations: Vec<(f32, f32, f32, f32)>,
    /// 底部复刻的参与者框/小人所在的 y。
    bottom_head_y: f32,
}

impl SeqLayout {
    /// 所有「文字块」的包围盒 `(种类, x, y, w, h)`（画布坐标）。
    ///
    /// 渲染层按这些坐标画文字，回归测试拿同一份几何做两两相交检查——
    /// 时序图里最常见的毛病就是两块字叠在一起，肉眼很难在改动时全部盯住。
    #[cfg(test)]
    fn text_rects(&self) -> Vec<(&'static str, f32, f32, f32, f32)> {
        let mut rects = Vec::new();
        for (x, y, text) in &self.labels {
            rects.push(("消息", *x, *y, text_width(text, 11.) + 10., 15.));
        }
        for (_, _, x, y, w) in &self.notes {
            rects.push(("注释", *x, *y, *w, 26.));
        }
        for f in &self.fragments {
            rects.push((
                "片段标签",
                f.x1,
                f.y1 - 9.,
                text_width(&f.kind, 10.) + 12.,
                16.,
            ));
            if !f.label.is_empty() {
                rects.push((
                    "首段守卫",
                    f.label_x,
                    f.y1 + 2.,
                    text_width(&f.label, 11.) + 4.,
                    15.,
                ));
            }
            for (band, text, x) in &f.splits {
                if !text.is_empty() {
                    rects.push((
                        "分隔守卫",
                        *x,
                        *band + 2.,
                        text_width(text, 11.) + 4.,
                        15.,
                    ));
                }
            }
        }
        for (msg, row) in &self.messages {
            if msg.number.is_some() {
                rects.push((
                    "编号",
                    self.xs[msg.from] - 8.,
                    self.row_y[*row] - 8.,
                    16.,
                    16.,
                ));
            }
        }
        rects
    }
}

/// 组合片段外框：左上角的类型标签（`kind`）、各段守卫与它们的落位。
///
/// 守卫的位置在布局层就算好——它既要居中，又必须给类型标签让位，还要和
/// 下面那行的消息文字错开。放在这里算一次，渲染与回归测试读的是同一份坐标，
/// 不会再出现「布局认为没压住、渲染出来却叠在一起」。
#[derive(Clone)]
struct FragBox {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    kind: String,
    /// 首段守卫（已带方括号）；片段没写标签时为空。
    label: String,
    /// 首段守卫的左边距。
    label_x: f32,
    /// 分隔守卫：`(横带顶 y, 文本（带方括号）, 左边距)`。
    splits: Vec<(f32, String, f32)>,
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

    // 片段开始/结束/分隔（`else`、`and`）都在对应行前插一段空隙：
    // 标签、守卫与消息各占一行，复杂图（片段嵌套、连续片段）不会挤在一起。
    //
    // 分隔尤其不能省：守卫原来直接画在该行的行带顶，跟这一行消息的文字
    // 只差 5px，`[支付失败]` 会压在「返回失败码」上——看起来就是文字被遮盖。
    let mut starts = vec![0usize; seq.rows + 1];
    let mut ends = vec![0usize; seq.rows + 1];
    let mut split_rows = vec![0usize; seq.rows + 1];
    for f in &seq.fragments {
        starts[f.start.min(seq.rows)] += 1;
        ends[f.end.min(seq.rows)] += 1;
        for (row, _) in &f.splits {
            split_rows[(*row).min(seq.rows)] += 1;
        }
    }
    let mut row_top = Vec::with_capacity(seq.rows + 1);
    let mut cursor = top;
    for r in 0..=seq.rows {
        cursor += starts[r] as f32 * FRAG_HEADER
            + ends[r] as f32 * FRAG_FOOTER
            + split_rows[r] as f32 * FRAG_SPLIT;
        row_top.push(cursor);
        cursor += ROW_H;
    }
    // 片段前一行内容的底边（row_top 已经减掉本行的片段留白）。
    let prev_bottom = |r: usize| {
        row_top[r]
            - starts[r] as f32 * FRAG_HEADER
            - ends[r] as f32 * FRAG_FOOTER
            - split_rows[r] as f32 * FRAG_SPLIT
    };
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

    // 消息文字块：横向居中在两位参与者之间，贴边时夹回画布内。
    // 坐标在布局层算好，渲染照抄、回归测试也读这一份，避免两边各算一套。
    let labels: Vec<(f32, f32, String)> = messages
        .iter()
        .filter_map(|(msg, row)| {
            let text = message_label(msg);
            if text.is_empty() {
                return None;
            }
            let w = text_width(&text, 11.) + 10.;
            let mid = (xs[msg.from] + xs[msg.to]) / 2.;
            let x = (mid - w / 2.).clamp(0., (width - w).max(0.));
            Some((x, row_y[*row] - LABEL_DY, text))
        })
        .collect();

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

    // 同一行上的分隔（多层片段同时 else）依次占带，跨片段共享这个游标。
    let mut split_seen = vec![0usize; seq.rows + 1];
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
            // 守卫水平居中，左边界要让开左上角的类型标签（alt/par/break）：
            // 窄片段里「break」和「[无法继续]」原来贴在一起，看着像文字叠字。
            let kind_w = text_width(&f.kind, 10.) + 20.;
            let guard_x = |text: &str| {
                let w = text_width(text, 11.);
                let center = (x1 + x2 - w) / 2.;
                center.max(x1 + kind_w).min((x2 - w - 6.).max(x1 + 4.))
            };
            // 分隔守卫各自占一条横带：同一行有多个 `else` / `and` 时从下往上叠，
            // 守卫文字落在带顶，下面的消息文字才不会被压住。
            let splits: Vec<(f32, String, f32)> = f
                .splits
                .iter()
                .map(|(row, label)| {
                    let row = (*row).min(seq.rows);
                    let slot = split_seen[row];
                    split_seen[row] += 1;
                    let band = row_top[row] - (split_rows[row] - slot) as f32 * FRAG_SPLIT;
                    let text = bracket(label);
                    let x = guard_x(&text);
                    (band, text, x)
                })
                .collect();
            let label = bracket(&f.label);
            FragBox {
                x1,
                y1,
                x2,
                y2,
                kind: f.kind.clone(),
                label_x: guard_x(&label),
                label,
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
        labels,
        fragments,
        activations,
        bottom_head_y,
    }
}

/// 消息文字只用正文；编号改由生命线上的圆圈表示。
fn message_label(msg: &Message) -> String {
    msg.text.clone()
}

/// 片段守卫统一套方括号（`[令牌有效]`）；没写标签时保持空串，不画框。
fn bracket(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("[{label}]")
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/// 图的自然尺寸（未缩放），供等比适配包装计算缩放系数。
pub(super) fn size(seq: &Sequence) -> (f32, f32) {
    let lay = layout(seq);
    (lay.width, lay.height)
}

pub(super) fn render(seq: &Sequence, scale: f32, cx: &App) -> AnyElement {
    // 等比缩放：函数内所有 px 值（坐标、宽高、字号、线宽、圆角）同乘 scale，
    // 布局坐标不动，几何天然相似，不变形。scale=1 时与未缩放输出一致。
    let px = move |v: f32| gpui_kit::px(v * scale);
    let pal = palette::of(cx);
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
                    window.paint_path(path, pal.line);
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
                for (sy, _, _) in &frag.splits {
                    super::common::dashed_line(&mut b, (x1, *sy), (x2, *sy), &at);
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, pal.line);
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
                    window.paint_path(path, pal.surface);
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
                    window.paint_path(path, pal.line);
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
                            window.paint_path(path, pal.line);
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
                            window.paint_path(path, pal.line);
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
                            window.paint_path(path, pal.line);
                        }
                    }
                    _ => {
                        let back = if dir == 0 { -9. } else { 9. };
                        let mut open = PathBuilder::stroke(px(1.2));
                        seg!(open, (tip.0 + back, tip.1 - 4.), tip);
                        seg!(open, (tip.0 + back, tip.1 + 4.), tip);
                        if let Ok(path) = open.build() {
                            window.paint_path(path, pal.line);
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
                        window.paint_path(path, pal.line);
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
                        .text_color(pal.text)
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
                    .border(px(1.))
                    .border_color(pal.outline)
                    .bg(pal.surface)
                    .text_size(px(FONT))
                    .text_color(pal.text)
                    .child(div().text_center().px(px(8.)).child(p.label.clone())),
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
                .bg(pal.surface)
                .text_size(px(10.))
                .text_color(pal.text_muted)
                .child(frag.kind.clone()),
        );
        // 守卫的横向位置由布局层算好（居中 + 给类型标签让位），这里只管画。
        if !frag.label.is_empty() {
            container = container.child(
                div()
                    .absolute()
                    .left(px(frag.label_x))
                    .top(px(frag.y1 + 2.))
                    .px(px(2.))
                    .bg(pal.background)
                    .text_size(px(11.))
                    .text_color(pal.text_muted)
                    .child(frag.label.clone()),
            );
        }
        for (sy, text, x) in &frag.splits {
            if text.is_empty() {
                continue;
            }
            container = container.child(
                div()
                    .absolute()
                    .left(px(*x))
                    .top(px(*sy + 2.))
                    .px(px(2.))
                    .bg(pal.background)
                    .text_size(px(11.))
                    .text_color(pal.text_muted)
                    .child(text.clone()),
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
                .border(px(1.))
                .border_color(pal.outline)
                .bg(pal.background)
                .text_size(px(9.))
                .text_color(pal.text)
                .child(format!("{number}")),
        );
    }

    // 消息标签：坐标由布局层算好（见 `SeqLayout::labels`）。
    for (x, y, text) in &lay.labels {
        container = container.child(
            h_flex()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .px(px(3.))
                .rounded(px(3.))
                .bg(pal.background)
                .text_size(px(11.))
                .text_color(pal.text_muted)
                .child(text.clone()),
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
                .bg(pal.surface)
                .text_size(px(11.))
                .text_color(pal.text_muted)
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

    /// `autonumber 10 5`：从 10 起、每条 +5。旧实现只认裸 `autonumber`。
    #[test]
    fn autonumber_honors_start_and_step() {
        let seq = parse(
            "sequenceDiagram\n autonumber 10 5\n A->>B: 一\n B->>A: 二\n A->>B: 三\n",
        )
        .unwrap();
        let nums: Vec<_> = seq
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message(m, _) => m.number,
                _ => None,
            })
            .collect();
        assert_eq!(nums, vec![10, 15, 20]);
        // 只写起始值：步长默认 1。
        let seq = parse("sequenceDiagram\n autonumber 3\n A->>B: x\n B->>A: y\n").unwrap();
        let nums: Vec<_> = seq
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message(m, _) => m.number,
                _ => None,
            })
            .collect();
        assert_eq!(nums, vec![3, 4]);
    }

    /// `create participant` / `create actor` / `destroy` 都要登记参与者，
    /// 不能把整行当未知消息丢掉。
    #[test]
    fn create_and_destroy_register_participants() {
        let seq = parse(
            "sequenceDiagram\n A->>B: 嗨\n create participant C as 丙\n create actor D\n B->>C: 转\n destroy C\n",
        )
        .unwrap();
        let labels: Vec<_> = seq.participants.iter().map(|p| p.label.as_str()).collect();
        assert!(labels.contains(&"丙"), "create participant 应登记别名");
        assert!(labels.contains(&"D"), "create actor 应登记");
        assert!(seq.participants.iter().any(|p| p.label == "D" && p.actor));
        // C 的显示名是别名「丙」；destroy 不删除它，参与者总数仍是 A/B/C/D 四个。
        assert_eq!(seq.participants.len(), 4, "destroy 不应移除参与者");
        assert_eq!(seq.rows, 2, "create/destroy 不占消息行");
    }

    /// 反向箭头 `<-` / `<<-`：右端是发送方，箭头朝左。
    #[test]
    fn reverse_arrows_swap_sender_and_receiver() {
        let seq = parse("sequenceDiagram\n participant A\n participant B\n B<-A: 反向\n").unwrap();
        let msg = seq
            .events
            .iter()
            .find_map(|e| match e {
                Event::Message(m, _) => Some(m),
                _ => None,
            })
            .unwrap();
        // A 在右（发送方），B 在左（接收方）。
        assert_eq!(seq.participants[msg.from].label, "A");
        assert_eq!(seq.participants[msg.to].label, "B");
        assert!(msg.arrow == Arrow::Solid);
        // 双向箭头不能被反向 `<-` 误吞。
        let seq = parse("sequenceDiagram\n A<<->>B: 双向\n").unwrap();
        let msg = seq
            .events
            .iter()
            .find_map(|e| match e {
                Event::Message(m, _) => Some(m),
                _ => None,
            })
            .unwrap();
        assert!(msg.arrow == Arrow::Bidi);
        assert_eq!(seq.participants[msg.from].label, "A");
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

    /// 展示页的六张时序图都必须让激活框显式闭合。
    /// 旧示例里的 `B-->>-A` 会让 Mermaid 报 "Trying to inactivate an inactive
    /// participant (B)"：`-` 关闭的是发送方，而 B 从未被 `+` 激活。
    #[test]
    fn all_showcase_activations_are_balanced() {
        let blocks = super::super::showcase_blocks("sequence");
        assert_eq!(blocks.len(), 6, "展示页时序图数量变了，需要重新确认夹具");
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

    /// 守卫必须独占一条横带，而且要让开左上角的类型标签。
    ///
    /// 旧实现把 `else` / `and` 守卫直接画在行带顶：11px 文字约 15px 高，
    /// 下面那条消息的文字起点只差 5px，`[支付失败]` 就压在「返回失败码」上，
    /// 看起来像文字被涂掉了。窄片段里 `break` 与 `[无法继续]` 也会贴在一起。
    #[test]
    fn guards_get_their_own_band() {
        // 11px 文字的实际行高约 14px，取 15 留一点余量。
        const TEXT_H: f32 = 15.;
        let mut checked = 0;
        for (bi, src) in super::super::showcase_blocks("sequence").iter().enumerate() {
            let seq = parse(src).unwrap();
            let l = layout(&seq);
            for (fi, f) in seq.fragments.iter().enumerate() {
                let fb = &l.fragments[fi];
                let kind_right = fb.x1 + text_width(&f.kind, 10.) + 16.;
                let first_label = |row: usize| {
                    l.messages
                        .iter()
                        .find(|(_, r)| *r == row)
                        .map(|(_, r)| l.row_y[*r] - LABEL_DY)
                };
                if !f.label.is_empty() {
                    if let Some(top) = first_label(f.start) {
                        assert!(
                            fb.y1 + 2. + TEXT_H <= top,
                            "第 {bi} 张图「{}」的守卫压住了第一行消息",
                            f.kind
                        );
                        checked += 1;
                    }
                    assert!(
                        fb.label_x >= kind_right - 0.5 || fb.label_x + 4. <= fb.x1,
                        "第 {bi} 张图「{}」的守卫挤到了类型标签上",
                        f.kind
                    );
                }
                for ((band, text, x), (row, _)) in fb.splits.iter().zip(f.splits.iter()) {
                    if text.is_empty() {
                        continue;
                    }
                    let top = first_label(*row).unwrap_or(*band + FRAG_SPLIT);
                    assert!(
                        *band + 2. + TEXT_H <= top,
                        "第 {bi} 张图「{}」的分隔守卫 {text} 与第 {row} 行消息同高",
                        f.kind
                    );
                    assert!(
                        *x >= kind_right - 0.5,
                        "第 {bi} 张图的分隔守卫 {text} 挤到了类型标签上"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 4, "展示页应检查到多条守卫，实际 {checked}");
    }

    /// 展示页六张时序图里，任意两块文字都不能叠在一起。
    ///
    /// 时序图的文字全是绝对定位：消息标签、注释、片段类型标签、守卫、编号圆圈。
    /// 只要布局少算一段间距，两块字就会互相盖住，改起来又很容易顾此失彼——
    /// 这里把渲染用的同一份几何拿来做两两相交检查。
    #[test]
    fn texts_never_cover_each_other() {
        let mut checked = 0;
        for (bi, src) in super::super::showcase_blocks("sequence").iter().enumerate() {
            let seq = parse(src).unwrap_or_else(|| panic!("第 {bi} 张时序图无法解析"));
            let l = layout(&seq);
            let rects = l.text_rects();
            checked += rects.len();
            for (i, a) in rects.iter().enumerate() {
                for b in rects.iter().skip(i + 1) {
                    let (an, ax, ay, aw, ah) = *a;
                    let (bn, bx, by, bw, bh) = *b;
                    let ox = (ax + aw).min(bx + bw) - ax.max(bx);
                    let oy = (ay + ah).min(by + bh) - ay.max(by);
                    assert!(
                        ox <= 0.5 || oy <= 0.5,
                        "第 {bi} 张图：{an}（{ax:.0},{ay:.0} {aw:.0}×{ah:.0}）\
                         与 {bn}（{bx:.0},{by:.0} {bw:.0}×{bh:.0}）重叠了 {ox:.1}×{oy:.1}"
                    );
                }
            }
        }
        assert!(checked >= 20, "六张展示图合计只检查到 {checked} 块文字，范围不对");
    }

    #[test]
    fn fragment_frames_cover_only_involved_participants() {        let blocks = super::super::showcase_blocks("sequence");
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

    /// 四种反向箭头都交换发送 / 接收方，且各自保留正确的箭头类型。
    #[test]
    fn all_reverse_arrow_types_swap_and_keep_kind() {
        for (token, kind) in [
            ("<-", Arrow::Solid),
            ("<<-", Arrow::SolidFilled),
            ("<--", Arrow::Dotted),
            ("<<--", Arrow::DottedFilled),
        ] {
            let src = format!("sequenceDiagram\n participant A\n participant B\n B{token}A: 反向\n");
            let seq = parse(&src).unwrap();
            let msg = seq
                .events
                .iter()
                .find_map(|e| match e {
                    Event::Message(m, _) => Some(m),
                    _ => None,
                })
                .unwrap();
            assert_eq!(seq.participants[msg.from].label, "A", "{token} 发送方应在右");
            assert_eq!(seq.participants[msg.to].label, "B", "{token} 接收方应在左");
            assert!(msg.arrow == kind, "{token} 箭头类型不对");
        }
    }

    /// autonumber 的边界：步长 0 钳到 1、非数字参数退回默认、裸命令从 1 起。
    #[test]
    fn autonumber_step_edge_cases() {
        let nums = |src: &str| -> Vec<usize> {
            parse(src)
                .unwrap()
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Message(m, _) => m.number,
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            nums("sequenceDiagram\n autonumber 5 0\n A->>B: x\n B->>A: y\n A->>B: z\n"),
            vec![5, 6, 7],
            "步长 0 应钳到 1"
        );
        assert_eq!(
            nums("sequenceDiagram\n autonumber\n A->>B: x\n B->>A: y\n"),
            vec![1, 2]
        );
        assert_eq!(
            nums("sequenceDiagram\n autonumber abc\n A->>B: x\n B->>A: y\n"),
            vec![1, 2],
            "非数字参数应退回默认 1/1"
        );
        assert_eq!(
            nums("sequenceDiagram\n autonumber 2 3\n A->>B: x\n B->>A: y\n A->>B: z\n"),
            vec![2, 5, 8]
        );
        assert_eq!(
            nums("sequenceDiagram\n A->>B: x\n B->>A: y\n"),
            Vec::<usize>::new(),
            "没有 autonumber 就不编号"
        );
    }

    /// create/destroy 的登场顺序与别名：destroy 不删除参与者，actor 标记正确。
    #[test]
    fn create_destroy_ordering_and_aliases() {
        let seq = parse(
            "sequenceDiagram\n A->>B: 一\n create actor C as 丙\n B->>C: 二\n create participant D\n destroy C as 丙\n C->>D: 三\n",
        )
        .unwrap();
        let labels: Vec<_> = seq.participants.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "B", "丙", "D"], "按登场顺序登记");
        assert!(seq.participants[2].actor, "create actor 应是 actor");
        assert!(!seq.participants[3].actor, "create participant 不是 actor");
        assert_eq!(seq.participants.len(), 4, "destroy 不应移除参与者");
        assert_eq!(seq.rows, 3, "create/destroy 不占消息行");
    }
}
