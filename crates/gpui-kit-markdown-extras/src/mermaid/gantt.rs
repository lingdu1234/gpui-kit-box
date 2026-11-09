//! `gantt` 甘特图：解析、时间轴换算与 GPUI 绘制。
//!
//! 日期换算用纯 Rust 实现（Howard Hinnant 的 civil <-> days 算法），不引入
//! `chrono` 之类的新依赖，保持 crate 只有 `gpui-kit` 一个依赖。
//!
//! ```text
//! gantt
//!     title 版本 0.2 计划
//!     dateFormat YYYY-MM-DD
//!     axisFormat %m/%d
//!     tickInterval 1week
//!     excludes weekends
//!     section 设计
//!     需求分析 :done, des1, 2026-01-05, 3d
//!     原型评审 :active, des2, after des1, 2d
//!     section 开发
//!     编码 :crit, dev1, after des2, 10d
//!     发版 :milestone, m1, after dev1, 0d
//! ```
//!
//! `excludes weekends` 会把时间轴压缩成工作日：x 坐标按「非周末天数」计，
//! 周末不占宽度、也不生成刻度，任务条自然跳过休息日。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use super::common::{clean_label, dashed_line, text_width};

const PAD: f32 = 24.;
const ROW_H: f32 = 36.;
const SECTION_H: f32 = 30.;
const TITLE_H: f32 = 34.;
const AXIS_H: f32 = 32.;
const BAR_H: f32 = 18.;
const MIN_LABEL_W: f32 = 132.;
const MIN_AXIS_W: f32 = 560.;
const MAX_AXIS_W: f32 = 1080.;

// ---------------------------------------------------------------------------
// 日期：civil <-> 1970-01-01 起的天数
// ---------------------------------------------------------------------------

/// 格里高利历日期转「Unix 纪元天数」；`days_from_civil(1970,1,1) == 0`。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// 0 = 周日 …… 6 = 周六（1970-01-01 是周四）。
fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

fn is_weekend(day: i64) -> bool {
    matches!(weekday(day), 0 | 6)
}

/// 今天的纪元天数（UTC；仅用于画 todayMarker，差一天不影响阅读）。
fn today_day() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86400)
        .unwrap_or(0)
}

/// 按 `dateFormat` 里 YYYY/MM/DD 的出现顺序解析日期；时间部分忽略。
fn parse_date(text: &str, fmt: &str) -> Option<i64> {
    let nums: Vec<i64> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(3)
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 3 {
        return None;
    }
    let upper = fmt.to_ascii_uppercase();
    let mut fields: Vec<(usize, char)> = Vec::new();
    for (key, tag) in [("YYYY", 'y'), ("MM", 'm'), ("DD", 'd')] {
        if let Some(at) = upper.find(key) {
            fields.push((at, tag));
        }
    }
    // 格式没写全时按 YYYY-MM-DD 兜底。
    if fields.len() < 3 {
        fields = vec![(0, 'y'), (1, 'm'), (2, 'd')];
    }
    fields.sort_by_key(|f| f.0);
    let mut y = 1970;
    let mut m = 1;
    let mut d = 1;
    for (value, (_, tag)) in nums.iter().zip(fields.iter()) {
        match tag {
            'y' => y = *value,
            'm' => m = *value,
            _ => d = *value,
        }
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// 时长转天数：`d`/`day` 天、`w` 周、`m`/`month` 月（30 天）、`h` 小时。
fn parse_duration(text: &str) -> Option<f32> {
    let s = text.trim().to_ascii_lowercase();
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f32 = digits.parse().ok()?;
    let unit: String = s.chars().skip(digits.len()).collect();
    let scale = match unit.trim().chars().next() {
        Some('w') => 7.,
        Some('m') => 30.,
        Some('h') => 1. / 24.,
        _ => 1.,
    };
    Some(value * scale)
}

fn format_axis(fmt: &str, day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    fmt.replace("%Y", &format!("{y:04}"))
        .replace("%y", &format!("{:02}", y.rem_euclid(100)))
        .replace("%m", &format!("{m:02}"))
        .replace("%d", &format!("{d:02}"))
        .replace("%b", &format!("{m}月"))
}

// ---------------------------------------------------------------------------
// 图结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Normal,
    Done,
    Active,
    Crit,
}

#[derive(Clone)]
struct Task {
    name: String,
    id: Option<String>,
    status: Status,
    milestone: bool,
    section: usize,
    start: Option<i64>,
    end_date: Option<i64>,
    duration: f32,
    after: Vec<String>,
}

#[derive(Clone)]
pub(super) struct GanttChart {
    title: String,
    sections: Vec<String>,
    tasks: Vec<Task>,
    date_fmt: String,
    axis_fmt: String,
    tick_days: f32,
    exclude_weekends: bool,
    today_marker: bool,
}

fn is_duration_token(text: &str) -> bool {
    let first = text.trim().chars().next();
    first.is_some_and(|c| c.is_ascii_digit()) && parse_duration(text).is_some()
}

pub(super) fn parse(source: &str) -> Option<GanttChart> {
    let mut chart = GanttChart {
        title: String::new(),
        sections: Vec::new(),
        tasks: Vec::new(),
        date_fmt: "YYYY-MM-DD".to_string(),
        axis_fmt: "%m/%d".to_string(),
        tick_days: 7.,
        exclude_weekends: false,
        today_marker: true,
    };
    let mut section = 0usize;

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("gantt") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("title ") {
            chart.title = clean_label(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("dateFormat ") {
            chart.date_fmt = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("axisFormat ") {
            chart.axis_fmt = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("tickInterval ") {
            let v = rest.trim().to_ascii_lowercase();
            let n: f32 = v
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(1.);
            let scale = if v.contains("week") {
                7.
            } else if v.contains("month") {
                30.
            } else if v.contains("hour") {
                1. / 24.
            } else {
                1.
            };
            chart.tick_days = (n * scale).max(1.);
            continue;
        }
        if lower.starts_with("excludes ") {
            chart.exclude_weekends = lower.contains("weekend");
            continue;
        }
        if lower.starts_with("todaymarker ") {
            chart.today_marker = !lower.contains("off");
            continue;
        }
        if let Some(rest) = line.strip_prefix("section ") {
            chart.sections.push(clean_label(rest));
            section = chart.sections.len();
            continue;
        }
        if lower.starts_with("includes ") || lower.starts_with("weekday ") {
            continue;
        }

        // 任务行：`名称 : 状态, id, 起始, 时长/结束`
        let Some((name, body)) = line.split_once(':') else {
            continue;
        };
        let mut task = Task {
            name: clean_label(name),
            id: None,
            status: Status::Normal,
            milestone: false,
            section,
            start: None,
            end_date: None,
            duration: 1.,
            after: Vec::new(),
        };
        let mut dates = Vec::new();
        for token in body.split(',') {
            let t = token.trim();
            if t.is_empty() {
                continue;
            }
            let low = t.to_ascii_lowercase();
            if low == "done" {
                task.status = Status::Done;
            } else if low == "active" {
                task.status = Status::Active;
            } else if low == "crit" || low == "critical" {
                task.status = Status::Crit;
            } else if low == "milestone" {
                task.milestone = true;
            } else if low.starts_with("after ") {
                task.after.extend(
                    low[6..]
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                );
            } else if let Some(day) = parse_date(t, &chart.date_fmt) {
                dates.push(day);
            } else if is_duration_token(t) {
                task.duration = parse_duration(t).unwrap_or(1.);
            } else if task.id.is_none() {
                task.id = Some(t.to_string());
            }
        }
        if let Some(&first) = dates.first() {
            task.start = Some(first);
        }
        if let Some(&second) = dates.get(1) {
            task.end_date = Some(second);
        }
        if task.name.is_empty() {
            continue;
        }
        chart.tasks.push(task);
    }

    if chart.tasks.is_empty() {
        return None;
    }
    Some(chart)
}

// ---------------------------------------------------------------------------
// 布局
// ---------------------------------------------------------------------------

struct Bar {
    x: f32,
    y: f32,
    w: f32,
    status: Status,
    milestone: bool,
}

struct GanttLayout {
    width: f32,
    height: f32,
    label_w: f32,
    rows: Vec<(f32, String, bool)>, // y, 文字, 是否 section 标题
    bars: Vec<Bar>,
    ticks: Vec<(f32, String)>,
    today_x: Option<f32>,
    axis_top: f32,
}

/// 把日历天映射到时间轴上的「格数」；排除周末时只数工作日。
fn axis_day(day: i64, min_day: i64, exclude: bool) -> f32 {
    if !exclude {
        return (day - min_day) as f32;
    }
    let mut count = 0i64;
    let mut d = min_day;
    while d < day {
        if !is_weekend(d) {
            count += 1;
        }
        d += 1;
    }
    count as f32
}

fn resolve_ranges(chart: &GanttChart) -> Vec<(i64, i64)> {
    let id_map: HashMap<String, usize> = chart
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.id.as_ref().map(|id| (id.clone(), i)))
        .collect();
    let mut ranges: Vec<Option<(i64, i64)>> = vec![None; chart.tasks.len()];
    for _ in 0..chart.tasks.len().max(1) {
        for (i, task) in chart.tasks.iter().enumerate() {
            let start = match task.start {
                Some(s) => s,
                None => {
                    let mut latest: Option<i64> = None;
                    for id in &task.after {
                        if let Some(&j) = id_map.get(id)
                            && let Some((_, end)) = ranges[j]
                        {
                            latest = Some(latest.map_or(end, |v: i64| v.max(end)));
                        }
                    }
                    match latest {
                        Some(v) => v,
                        None => continue,
                    }
                }
            };
            let end = task
                .end_date
                .unwrap_or(start + task.duration.ceil().max(1.) as i64);
            ranges[i] = Some((start, end.max(start + 1)));
        }
    }
    let fallback = chart
        .tasks
        .iter()
        .filter_map(|t| t.start)
        .min()
        .unwrap_or_else(today_day);
    ranges
        .into_iter()
        .map(|r| r.unwrap_or((fallback, fallback + 1)))
        .collect()
}

fn layout(chart: &GanttChart) -> GanttLayout {
    let ranges = resolve_ranges(chart);
    let mut min_day = ranges.iter().map(|r| r.0).min().unwrap_or(0);
    let mut max_day = ranges.iter().map(|r| r.1).max().unwrap_or(min_day + 1);
    if chart.today_marker {
        let today = today_day();
        if today >= min_day.saturating_sub(30) && today <= max_day + 30 {
            min_day = min_day.min(today);
            max_day = max_day.max(today + 1);
        }
    }
    let total = axis_day(max_day, min_day, chart.exclude_weekends).max(1.);

    let label_w = chart
        .tasks
        .iter()
        .map(|t| text_width(&t.name, 11.) + 20.)
        .chain(chart.sections.iter().map(|s| text_width(s, 12.) + 20.))
        .fold(MIN_LABEL_W, f32::max);
    // 先定时间轴宽度再反推每天占多少像素，短跨度的条也能铺满整条轴。
    let axis_w = MIN_AXIS_W.max((total * 8.).min(MAX_AXIS_W));
    let day_w = axis_w / total;
    let axis_x = PAD + label_w;
    let width = axis_x + axis_w + PAD;

    // 行：每个 section 一条标题，其下是该 section 的任务。
    let mut rows = Vec::new();
    let mut bars = Vec::new();
    let mut y = PAD + TITLE_H + AXIS_H;
    for (si, section) in chart.sections.iter().enumerate() {
        if si > 0 || !section.is_empty() {
            rows.push((y, section.clone(), true));
            y += SECTION_H;
        }
        for (ti, task) in chart.tasks.iter().enumerate() {
            if task.section != si + 1 {
                continue;
            }
            rows.push((y, task.name.clone(), false));
            let (start, end) = ranges[ti];
            let x = axis_x + axis_day(start, min_day, chart.exclude_weekends) * day_w;
            let w = if task.milestone {
                12.
            } else {
                ((axis_day(end, min_day, chart.exclude_weekends)
                    - axis_day(start, min_day, chart.exclude_weekends))
                    * day_w)
                    .max(6.)
            };
            bars.push(Bar {
                x: x.min(axis_x + axis_w - w.max(0.)),
                y: y + (ROW_H - BAR_H) / 2.,
                w,
                status: task.status,
                milestone: task.milestone,
            });
            y += ROW_H;
        }
    }
    // 没有 section 时任务也要有行（section 保持 0）。
    if chart.sections.is_empty() {
        for (ti, task) in chart.tasks.iter().enumerate() {
            rows.push((y, task.name.clone(), false));
            let (start, end) = ranges[ti];
            let x = axis_x + axis_day(start, min_day, chart.exclude_weekends) * day_w;
            let w = ((axis_day(end, min_day, chart.exclude_weekends)
                - axis_day(start, min_day, chart.exclude_weekends))
                * day_w)
                .max(6.);
            bars.push(Bar {
                x,
                y: y + (ROW_H - BAR_H) / 2.,
                w,
                status: task.status,
                milestone: task.milestone,
            });
            y += ROW_H;
        }
    }

    // 刻度：按 tickInterval 推进；排除周末时周末不出刻度。
    let mut ticks = Vec::new();
    let step = chart.tick_days.max(1.).round() as i64;
    let mut d = min_day;
    while d <= max_day {
        if !chart.exclude_weekends || !is_weekend(d) {
            ticks.push((
                axis_x + axis_day(d, min_day, chart.exclude_weekends) * day_w,
                format_axis(&chart.axis_fmt, d),
            ));
        }
        d += step;
    }

    let today_x = chart
        .today_marker
        .then(|| {
            let today = today_day();
            (today >= min_day && today <= max_day)
                .then(|| axis_x + axis_day(today, min_day, chart.exclude_weekends) * day_w)
        })
        .flatten();

    GanttLayout {
        width,
        height: y + PAD,
        label_w,
        rows,
        bars,
        ticks,
        today_x,
        axis_top: PAD + TITLE_H,
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

pub(super) fn render(chart: &GanttChart, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (border, muted, line, fg) = (
        theme.border,
        theme.muted,
        theme.muted_foreground,
        theme.foreground,
    );
    let (primary, success, danger, warning) =
        (theme.primary, theme.success, theme.danger, theme.warning);
    let lay = layout(chart);

    fn status_color(
        s: Status,
        muted: gpui_kit::Hsla,
        primary: gpui_kit::Hsla,
        success: gpui_kit::Hsla,
        danger: gpui_kit::Hsla,
    ) -> gpui_kit::Hsla {
        match s {
            Status::Done => success,
            Status::Active => primary,
            Status::Crit => danger,
            Status::Normal => muted,
        }
    }

    let draw = DrawData {
        bars: lay
            .bars
            .iter()
            .map(|b| {
                (
                    b.x,
                    b.y,
                    b.w,
                    b.milestone,
                    status_color(b.status, muted, primary, success, danger),
                )
            })
            .collect(),
        ticks: lay.ticks.clone(),
        today_x: lay.today_x,
        axis_top: lay.axis_top,
        height: lay.height,
    };

    let paint = canvas(
        move |bounds, _, _| bounds.origin,
        move |_bounds, origin, window, _| {
            let (ox, oy) = (origin.x, origin.y);
            let at = |p: &(f32, f32)| point(ox + px(p.0), oy + px(p.1));

            // 刻度网格线
            let mut grid = PathBuilder::stroke(px(1.));
            for (x, _) in &draw.ticks {
                dashed_line(&mut grid, (*x, draw.axis_top), (*x, draw.height - PAD), &at);
            }
            if let Ok(path) = grid.build() {
                window.paint_path(path, border);
            }
            // 时间轴基线
            let mut axis = PathBuilder::stroke(px(1.));
            let left = draw.ticks.first().map(|t| t.0).unwrap_or(0.);
            axis.move_to(at(&(left, draw.axis_top)));
            axis.line_to(at(&(
                draw.ticks.last().map(|t| t.0).unwrap_or(left),
                draw.axis_top,
            )));
            if let Ok(path) = axis.build() {
                window.paint_path(path, border);
            }

            // 任务条 / 里程碑菱形
            for (x, y, w, milestone, color) in &draw.bars {
                let mut fill = PathBuilder::fill();
                if *milestone {
                    let cx = x + 6.;
                    let cy = y + BAR_H / 2.;
                    fill.move_to(at(&(cx, cy - 9.)));
                    fill.line_to(at(&(cx + 9., cy)));
                    fill.line_to(at(&(cx, cy + 9.)));
                    fill.line_to(at(&(cx - 9., cy)));
                } else {
                    fill.move_to(at(&(*x, *y)));
                    fill.line_to(at(&(x + w, *y)));
                    fill.line_to(at(&(x + w, y + BAR_H)));
                    fill.line_to(at(&(*x, y + BAR_H)));
                }
                fill.close();
                if let Ok(path) = fill.build() {
                    window.paint_path(path, *color);
                }
            }

            // 今日标记
            if let Some(x) = draw.today_x {
                let mut b = PathBuilder::stroke(px(1.5));
                b.move_to(at(&(x, draw.axis_top)));
                b.line_to(at(&(x, draw.height - PAD)));
                if let Ok(path) = b.build() {
                    window.paint_path(path, warning);
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

    if !chart.title.is_empty() {
        container = container.child(
            div()
                .absolute()
                .left(px(PAD))
                .top(px(PAD - 4.))
                .text_size(px(14.))
                .text_color(line)
                .child(chart.title.clone()),
        );
    }

    // 刻度标签间距不足时跳过一部分，只保留网格线，日期不会叠成一团。
    let mut label_right = f32::MIN;
    for (x, text) in &lay.ticks {
        let w = text_width(text, 10.) + 8.;
        let left = x - w / 2.;
        if left < label_right + 8. {
            continue;
        }
        label_right = left + w;
        container = container.child(
            div()
                .absolute()
                .left(px(left))
                .top(px(lay.axis_top - 18.))
                .w(px(w))
                .text_size(px(10.))
                .text_color(line)
                .text_center()
                .child(text.clone()),
        );
    }

    for (y, text, is_section) in &lay.rows {
        container = container.child(
            h_flex()
                .absolute()
                .left(px(PAD))
                .top(px(*y))
                .w(px(lay.label_w - 12.))
                .text_size(px(if *is_section { 12. } else { 11. }))
                .text_color(if *is_section { line } else { fg })
                .child(text.clone()),
        );
    }

    container.into_any_element()
}

struct DrawData {
    bars: Vec<(f32, f32, f32, bool, gpui_kit::Hsla)>,
    ticks: Vec<(f32, String)>,
    today_x: Option<f32>,
    axis_top: f32,
    height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"gantt
    title 版本 0.2 计划
    dateFormat YYYY-MM-DD
    axisFormat %m/%d
    tickInterval 1week
    excludes weekends
    section 设计
    需求分析 :done, des1, 2026-01-05, 3d
    原型评审 :active, des2, after des1, 2d
    section 开发
    编码 :crit, dev1, after des2, 10d
    发版 :milestone, m1, after dev1, 0d
"#;

    #[test]
    fn civil_date_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        for day in [-365, 0, 1, 1000, 20000] {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day);
        }
        assert_eq!(
            parse_date("2026-01-05", "YYYY-MM-DD"),
            Some(days_from_civil(2026, 1, 5))
        );
        assert_eq!(
            parse_date("05/01/2026", "DD/MM/YYYY"),
            Some(days_from_civil(2026, 1, 5))
        );
    }

    #[test]
    fn parses_sections_statuses_and_dependencies() {
        let c = parse(SAMPLE).expect("应可解析");
        assert_eq!(c.title, "版本 0.2 计划");
        assert_eq!(c.sections.len(), 2);
        assert_eq!(c.tasks.len(), 4);
        assert!(c.exclude_weekends);
        assert_eq!(c.tick_days, 7.);
        assert!(c.tasks[0].status == Status::Done);
        assert!(c.tasks[1].after == vec!["des1"]);
        assert!(c.tasks[2].status == Status::Crit);
        assert!(c.tasks[3].milestone);
        let ranges = resolve_ranges(&c);
        assert_eq!(ranges[1].0, ranges[0].1, "after 应从依赖任务结束日开始");
        assert_eq!(ranges[2].0, ranges[1].1);
    }

    #[test]
    fn layout_keeps_bars_inside() {
        let c = parse(SAMPLE).unwrap();
        let l = layout(&c);
        assert!(l.width > 0. && l.height > 0.);
        assert_eq!(l.rows.len(), 6, "2 个 section 标题 + 4 个任务");
        for b in &l.bars {
            assert!(b.x >= 0. && b.x + b.w <= l.width + 0.5);
            assert!(b.y >= 0. && b.y + BAR_H <= l.height + 0.5);
        }
        for (x, _) in &l.ticks {
            assert!(*x >= 0. && *x <= l.width + 0.5);
        }
    }

    #[test]
    fn empty_chart_is_rejected() {
        assert!(parse("gantt\n title 空计划\n").is_none());
    }
}
