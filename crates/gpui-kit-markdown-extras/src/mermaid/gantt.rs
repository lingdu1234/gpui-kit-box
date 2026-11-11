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
//! # 版式
//!
//! 参照 Mermaid 官方甘特图的观感，与常见「左列任务名 + 右侧裸条」画法不同：
//!
//! - 标题居中置顶，日期刻度沉到图表**底部**；
//! - 左侧标签列只写 `section` 名（没有 `section` 时才退化成任务名），
//!   名字本身画在条形内部的居中位置——条形就是标签的容器；
//! - 每个 `section` 是一条贯通的交替底色带，把「阶段」和「任务」在视觉上分开；
//! - 条形描边 + 状态底色（`done` / `active` / `crit` / 默认），里程碑是空心菱形；
//! - 竖网格线只做定位参考，压得很淡，不与条形抢注意力；
//! - `todayMarker` 用一条红色竖线贯穿图表。
//!
//! 配色来自公共设施 [`super::palette`]：`active` 条形就是全图共用的强调色，
//! 底纹 / 网格 / 坐标轴用它的淡色阶；只有 `done` / `crit` / `today` 保留语义色
//! （绿 / 红），因为这里表达的是「进度状态」而不是图的装饰。
//!
//! # 时间轴与周末
//!
//! 横轴始终是**日历轴**：一天一格，周末也占宽度。`excludes weekends` 的作用是
//! 把周末涂成淡色竖带（一眼看出工作节奏），并让任务时长按工作日累计——所以
//! 跨周末的条会直接压在周末带上，与 Mermaid 的读法一致。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Hsla, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point,
};

use super::common::{clean_label, text_width};
use super::palette;

// 版式常量：改动会直接影响四张示例图的观感，调整时对着 showcase/gantt.md 一起看。
const PAD: f32 = 20.;
const TITLE_H: f32 = 34.;
const ROW_H: f32 = 28.;
const BAR_H: f32 = 16.;
const BAND_Y: f32 = 6.; // section 底色带上下内边距
const GROUP_GAP: f32 = 6.; // 相邻 section 带之间的空隙
const AXIS_H: f32 = 26.; // 底部日期区高度
const AXIS_GAP: f32 = 12.; // 标签列与时间轴之间的空隙
const MIN_LABEL_W: f32 = 56.;
const MIN_AXIS_W: f32 = 520.;
const MAX_AXIS_W: f32 = 900.;
const DAY_W: f32 = 14.; // 每天的目标像素，用来反推时间轴总宽
const BAR_FONT: f32 = 10.;
const TICK_FONT: f32 = 10.;
const ROW_FONT: f32 = 11.;
const MS_R: f32 = 7.; // 里程碑菱形半对角线

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
    /// `tickInterval`；`None` 表示按每天占的像素自动挑步长，避免标签挤在一起。
    tick_days: Option<f32>,
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
        tick_days: None,
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
            chart.tick_days = Some((n * scale).max(1.));
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

/// 一条任务条。`w == 0` 且 `milestone` 时只画菱形。
struct Bar {
    name: String,
    x: f32,
    y: f32,
    w: f32,
    status: Status,
    milestone: bool,
}

/// section 的贯通底色带；`alt` 决定用深一点还是浅一点的叠色。
struct Band {
    y: f32,
    h: f32,
    alt: bool,
}

/// 左侧标签列的一行文字：`muted` 为 section 名，否则是未分组任务名。
///
/// 记的是「行高」而不是文字基线：渲染时用 `line_height` 撑到 `h`，
/// 单行文字就会被垂直居中，不用手算字号带来的偏移。
struct RowLabel {
    y: f32,
    h: f32,
    text: String,
    muted: bool,
}

/// 一个 section 及其任务下标；没有 `section` 语句的任务归到 `title: None` 的组。
struct Group {
    title: Option<String>,
    tasks: Vec<usize>,
}

struct GanttLayout {
    width: f32,
    height: f32,
    bands: Vec<Band>,
    labels: Vec<RowLabel>,
    bars: Vec<Bar>,
    ticks: Vec<(f32, String)>,
    /// 周末淡色带的 x 区间（只在 `excludes weekends` 时非空）。
    weekends: Vec<(f32, f32)>,
    today_x: Option<f32>,
    chart_top: f32,
    chart_bottom: f32,
    tick_label_y: f32,
    axis_x: f32,
}

fn groups(chart: &GanttChart) -> Vec<Group> {
    let mut out = Vec::new();
    // 第一个 section 之前的任务是「未分组」，单独成带，标签列只能写任务名。
    let ungrouped: Vec<usize> = chart
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.section == 0)
        .map(|(i, _)| i)
        .collect();
    if !ungrouped.is_empty() {
        out.push(Group {
            title: None,
            tasks: ungrouped,
        });
    }
    for (si, name) in chart.sections.iter().enumerate() {
        let tasks: Vec<usize> = chart
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.section == si + 1)
            .map(|(i, _)| i)
            .collect();
        // 空 section 不占行，否则图上会多出一片没有任务只有标题的空白。
        if !tasks.is_empty() {
            out.push(Group {
                title: Some(name.clone()),
                tasks,
            });
        }
    }
    out
}

/// 从 `start` 起按工作日推进 `days` 天，返回结束日（不含）。
///
/// 起点落在周末时先顺延到周一；推进过程中周末不计入天数，但**占日历宽度**，
/// 这样「3d」的任务从周五开始会画到周一，中间压着周末带。
fn add_workdays(start: i64, days: i64) -> i64 {
    let mut day = start;
    while is_weekend(day) {
        day += 1;
    }
    let mut remaining = days.max(1);
    while remaining > 0 {
        day += 1;
        if !is_weekend(day) {
            remaining -= 1;
        }
    }
    day
}

/// 把任务解析成 `[start, end)` 的日历天区间；`after` 依赖要迭代到稳定。
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
            let end = match task.end_date {
                Some(e) => e,
                None if chart.exclude_weekends => {
                    add_workdays(start, task.duration.ceil().max(1.) as i64)
                }
                None => start + task.duration.ceil().max(1.) as i64,
            };
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

/// 把 `[min_day, max_day]` 里的周末合并成连续区间，供画淡色竖带用。
fn weekend_spans(min_day: i64, max_day: i64) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    let mut d = min_day;
    while d <= max_day {
        if is_weekend(d) {
            let start = d;
            while d <= max_day && is_weekend(d) {
                d += 1;
            }
            spans.push((start, d));
        } else {
            d += 1;
        }
    }
    spans
}

/// 没写 `tickInterval` 时按每天占的像素挑步长：相邻标签至少隔开约 64px，
/// 一天太密就两天、三天、一周地跳，避免日期叠字。
fn auto_tick_step(day_w: f32) -> i64 {
    for step in [1, 2, 3, 7, 14, 30, 90] {
        if step as f32 * day_w >= 64. {
            return step;
        }
    }
    365
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
    // 至少留一天，避免单任务 0 宽时除零。
    let total_days = (max_day - min_day).max(1) as f32;

    let groups = groups(chart);
    // 标签列只放 section 名，所以宽度按 section 名（未分组时按任务名）算，比旧版窄很多。
    let label_w = groups
        .iter()
        .flat_map(|g| match &g.title {
            Some(name) => vec![text_width(name, ROW_FONT)],
            None => g
                .tasks
                .iter()
                .map(|&i| text_width(&chart.tasks[i].name, ROW_FONT))
                .collect(),
        })
        .fold(MIN_LABEL_W, f32::max)
        + 20.;
    let axis_x = PAD + label_w + AXIS_GAP;
    let axis_w = (total_days * DAY_W).clamp(MIN_AXIS_W, MAX_AXIS_W);
    let day_w = axis_w / total_days;
    let width = axis_x + axis_w + PAD;
    let axis_right = axis_x + axis_w;

    let mut bands = Vec::new();
    let mut labels = Vec::new();
    let mut bars = Vec::new();
    let mut y = PAD + TITLE_H;
    let chart_top = y;
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            y += GROUP_GAP;
        }
        let band_h = group.tasks.len() as f32 * ROW_H + BAND_Y * 2.;
        bands.push(Band {
            y,
            h: band_h,
            alt: gi % 2 == 1,
        });
        if let Some(name) = &group.title {
            // section 名垂直居中在整条带上，表示「这一片都是这个阶段」。
            labels.push(RowLabel {
                y,
                h: band_h,
                text: name.clone(),
                muted: true,
            });
        }
        for (k, &ti) in group.tasks.iter().enumerate() {
            let row = y + BAND_Y + k as f32 * ROW_H;
            let task = &chart.tasks[ti];
            if group.title.is_none() {
                labels.push(RowLabel {
                    y: row,
                    h: ROW_H,
                    text: task.name.clone(),
                    muted: false,
                });
            }
            let (start, end) = ranges[ti];
            let x = axis_x + (start - min_day) as f32 * day_w;
            let w = if task.milestone {
                0.
            } else {
                ((end - start) as f32 * day_w).max(3.)
            };
            bars.push(Bar {
                // 末端贴边时往回缩，别让条形越过时间轴右边界。
                x: x.min((axis_right - w).max(axis_x)),
                y: row + (ROW_H - BAR_H) / 2.,
                w,
                status: task.status,
                milestone: task.milestone,
                name: task.name.clone(),
            });
        }
        y += band_h;
    }
    let chart_bottom = y;

    // 刻度固定在每个 tickInterval 的日期上；周末不屏蔽（会落在淡色带里，仍然可读）。
    let mut ticks = Vec::new();
    let step = match chart.tick_days {
        Some(v) => v.round().max(1.) as i64,
        None => auto_tick_step(day_w),
    };
    let mut d = min_day;
    while d <= max_day {
        ticks.push((
            axis_x + (d - min_day) as f32 * day_w,
            format_axis(&chart.axis_fmt, d),
        ));
        d += step;
    }

    let weekends = if chart.exclude_weekends {
        weekend_spans(min_day, max_day)
            .into_iter()
            .map(|(a, b)| {
                (
                    axis_x + (a - min_day) as f32 * day_w,
                    axis_x + (b - min_day) as f32 * day_w,
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let today_x = chart
        .today_marker
        .then(|| {
            let today = today_day();
            (today >= min_day && today <= max_day)
                .then(|| axis_x + (today - min_day) as f32 * day_w)
        })
        .flatten();

    let tick_label_y = chart_bottom + 8.;
    GanttLayout {
        width,
        height: tick_label_y + AXIS_H - 6.,
        bands,
        labels,
        bars,
        ticks,
        weekends,
        today_x,
        chart_top,
        chart_bottom,
        tick_label_y,
        axis_x,
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/// 条形配色：`(填充, 描边, 条内文字)`。
///
/// 颜色全部来自公共设施 [`super::palette`]，这里只负责把角色映射到四种状态：
/// 淡填 + 淡描边 = 未开始，绿色淡填 = 已完成，强调色实心 = 进行中，
/// 红色实心 = 关键路径。换主题、切明暗都不用改这里。
#[derive(Clone, Copy)]
struct StatusPalette {
    normal_fill: Hsla,
    normal_stroke: Hsla,
    normal_text: Hsla,
    done_fill: Hsla,
    done_stroke: Hsla,
    done_text: Hsla,
    active_fill: Hsla,
    active_stroke: Hsla,
    active_text: Hsla,
    crit_fill: Hsla,
    crit_stroke: Hsla,
    crit_text: Hsla,
}

impl StatusPalette {
    fn of(&self, status: Status) -> (Hsla, Hsla, Hsla) {
        match status {
            Status::Normal => (self.normal_fill, self.normal_stroke, self.normal_text),
            Status::Done => (self.done_fill, self.done_stroke, self.done_text),
            Status::Active => (self.active_fill, self.active_stroke, self.active_text),
            Status::Crit => (self.crit_fill, self.crit_stroke, self.crit_text),
        }
    }
}

/// 按估算宽度截断文字，超宽用省略号收尾；只用于「条形太窄」的兜底。
fn fit_text(text: &str, max_w: f32, font_size: f32) -> String {
    if max_w <= 0. || text_width(text, font_size) <= max_w {
        return text.to_string();
    }
    let budget = max_w - font_size;
    let mut out = String::new();
    for ch in text.chars() {
        let mut probe = out.clone();
        probe.push(ch);
        if text_width(&probe, font_size) > budget {
            break;
        }
        out = probe;
    }
    out.push('…');
    out
}

/// 条形上能读清的文字色：按「填充叠在页底色上」的实际亮度选深字或浅字。
///
/// 带透明度的填充先按 `l * a + bg * (1 - a)` 拍平估算（不追求精确的 HSL 复合），
/// 再根据主题明暗挑前景 / 背景色，换主题、切明暗都不会出现白字压白条。
fn text_on(fill: Hsla, bg: Hsla, fg: Hsla, dark: bool) -> Hsla {
    let flat_l = fill.l * fill.a + bg.l * (1. - fill.a);
    // 暗色主题里「深字」就是页底色本身、「浅字」是前景色；浅色主题正好相反。
    let (dark_text, light_text) = if dark { (bg, fg) } else { (fg, bg) };
    if flat_l > 0.6 { dark_text } else { light_text }
}

/// 图的自然尺寸（未缩放），供等比适配包装计算缩放系数。
pub(super) fn size(chart: &GanttChart) -> (f32, f32) {
    let lay = layout(chart);
    (lay.width, lay.height)
}

pub(super) fn render(chart: &GanttChart, scale: f32, cx: &App) -> AnyElement {
    // 等比缩放：函数内所有 px 值（坐标、宽高、字号、线宽、行高）同乘 scale，
    // 布局坐标不动，几何天然相似，不变形。scale=1 时与未缩放输出一致。
    let px = move |v: f32| gpui_kit::px(v * scale);
    let pal = palette::of(cx);
    let dark = cx.theme().is_dark();
    let lay = layout(chart);
    let (bg, fg) = (pal.background, pal.text);

    // 已完成 / 未开始用淡淡的状态色，进行中用蓝、关键路径用红——
    // 与 Mermaid 官方示例同一套视觉语言，但颜色仍然来自主题。
    //
    // 淡色填充要先用页底色拍平再画：半透明的条会把周末竖带透出来，
    // 看上去像一条带纹理的脏色，压在底色上不成立。
    let normal_fill = pal.veil(0.09);
    let done_fill = pal.tint(pal.success, 0.25);
    let active_fill = pal.accent;
    let crit_fill = pal.danger;
    let bars = StatusPalette {
        normal_fill,
        normal_stroke: fg.opacity(0.20),
        normal_text: text_on(normal_fill, bg, fg, dark),
        done_fill,
        done_stroke: pal.success.opacity(0.65),
        done_text: text_on(done_fill, bg, fg, dark),
        active_fill,
        active_stroke: pal.outline,
        active_text: text_on(active_fill, bg, fg, dark),
        crit_fill,
        crit_stroke: pal.danger,
        crit_text: text_on(crit_fill, bg, fg, dark),
    };
    // 底面层次：section 交替带 → 周末带 → 网格 → 条形。透明度都很低，
    // 保证在任意主题下都只是「背景」，不会盖过条形。
    let band_even = pal.veil(0.04);
    let band_odd = pal.veil(0.075);
    let weekend_fill = pal.text_muted.opacity(0.12);
    let grid = pal.text.opacity(0.16);
    let today_line = pal.danger.opacity(0.8);
    let axis_text = pal.text_muted;
    let section_text = pal.text_muted;
    let task_text = pal.text;

    let draw = DrawData {
        bands: lay.bands.iter().map(|b| (b.y, b.h, b.alt)).collect(),
        weekends: lay.weekends.clone(),
        bars: lay
            .bars
            .iter()
            .map(|b| (b.x, b.y, b.w, b.milestone, b.status))
            .collect(),
        ticks: lay.ticks.iter().map(|t| t.0).collect(),
        today_x: lay.today_x,
        left: PAD,
        right: lay.width - PAD,
        bg: pal.background,
        chart_top: lay.chart_top,
        chart_bottom: lay.chart_bottom,
    };

    let paint = canvas(
        move |bounds, _, _| bounds.origin,
        move |_bounds, origin, window, _| {
            let (ox, oy) = (origin.x, origin.y);
            let at = |p: &(f32, f32)| point(ox + px(p.0), oy + px(p.1));

            // 1) section 交替底色带：贯通整行（含左侧标签列）。
            for alt in [false, true] {
                let mut band = PathBuilder::fill();
                for (y, h, is_alt) in &draw.bands {
                    if *is_alt != alt {
                        continue;
                    }
                    band.move_to(at(&(draw.left, *y)));
                    band.line_to(at(&(draw.right, *y)));
                    band.line_to(at(&(draw.right, y + h)));
                    band.line_to(at(&(draw.left, y + h)));
                    band.close();
                }
                if let Ok(path) = band.build() {
                    window.paint_path(path, if alt { band_odd } else { band_even });
                }
            }

            // 2) 周末淡色竖带：只在时间轴区域内，贯穿整张图的高度。
            if !draw.weekends.is_empty() {
                let mut wk = PathBuilder::fill();
                for (x0, x1) in &draw.weekends {
                    wk.move_to(at(&(*x0, draw.chart_top)));
                    wk.line_to(at(&(*x1, draw.chart_top)));
                    wk.line_to(at(&(*x1, draw.chart_bottom)));
                    wk.line_to(at(&(*x0, draw.chart_bottom)));
                    wk.close();
                }
                if let Ok(path) = wk.build() {
                    window.paint_path(path, weekend_fill);
                }
            }

            // 3) 竖网格线：只做定位参考，压在条形下面。
            let mut lines = PathBuilder::stroke(px(1.));
            for x in &draw.ticks {
                lines.move_to(at(&(*x, draw.chart_top)));
                lines.line_to(at(&(*x, draw.chart_bottom)));
            }
            if let Ok(path) = lines.build() {
                window.paint_path(path, grid);
            }

            // 4) 条形：一条路径只能一种颜色，所以按状态分组，同色条拼进同一条路径。
            //    先填后描边，描边让条在浅底 / 深底上都有清晰轮廓。
            for status in [Status::Normal, Status::Done, Status::Active, Status::Crit] {
                let (fill, stroke, _) = bars.of(status);
                let mut f = PathBuilder::fill();
                let mut s = PathBuilder::stroke(px(1.));
                let mut any = false;
                for (x, y, w, milestone, st) in &draw.bars {
                    if *st != status || *milestone {
                        continue;
                    }
                    any = true;
                    f.move_to(at(&(*x, *y)));
                    f.line_to(at(&(x + w, *y)));
                    f.line_to(at(&(x + w, y + BAR_H)));
                    f.line_to(at(&(*x, y + BAR_H)));
                    f.close();
                    s.move_to(at(&(*x, *y)));
                    s.line_to(at(&(x + w, *y)));
                    s.line_to(at(&(x + w, y + BAR_H)));
                    s.line_to(at(&(*x, y + BAR_H)));
                    s.close();
                }
                if !any {
                    continue;
                }
                if let Ok(path) = f.build() {
                    window.paint_path(path, fill);
                }
                if let Ok(path) = s.build() {
                    window.paint_path(path, stroke);
                }
            }

            // 5) 里程碑：空心菱形——页底色填充 + 状态色描边，中心落在里程碑日期上。
            //    单独一趟画在条形之后，菱形不会被相邻的条盖住。
            for status in [Status::Normal, Status::Done, Status::Active, Status::Crit] {
                let (_, stroke, _) = bars.of(status);
                let mut diamond = PathBuilder::fill();
                let mut edge = PathBuilder::stroke(px(1.5));
                let mut any = false;
                for (x, y, _, milestone, st) in &draw.bars {
                    if !*milestone || *st != status {
                        continue;
                    }
                    any = true;
                    let (cx, cy) = (x + MS_R, y + BAR_H / 2.);
                    diamond.move_to(at(&(cx, cy - MS_R)));
                    diamond.line_to(at(&(cx + MS_R, cy)));
                    diamond.line_to(at(&(cx, cy + MS_R)));
                    diamond.line_to(at(&(cx - MS_R, cy)));
                    diamond.close();
                    edge.move_to(at(&(cx, cy - MS_R)));
                    edge.line_to(at(&(cx + MS_R, cy)));
                    edge.line_to(at(&(cx, cy + MS_R)));
                    edge.line_to(at(&(cx - MS_R, cy)));
                    edge.close();
                }
                if !any {
                    continue;
                }
                if let Ok(path) = diamond.build() {
                    window.paint_path(path, draw.bg);
                }
                if let Ok(path) = edge.build() {
                    window.paint_path(path, stroke);
                }
            }

            // 5) 今日标记：贯穿图表的红色竖线。
            if let Some(x) = draw.today_x {
                let mut b = PathBuilder::stroke(px(1.5));
                b.move_to(at(&(x, draw.chart_top - 4.)));
                b.line_to(at(&(x, draw.chart_bottom + 4.)));
                if let Ok(path) = b.build() {
                    window.paint_path(path, today_line);
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

    // 标题：居中置顶。
    if !chart.title.is_empty() {
        container = container.child(
            div()
                .absolute()
                .left_0()
                .top(px(PAD - 6.))
                .w(px(lay.width))
                .text_size(px(13.))
                .text_color(task_text)
                .text_center()
                .child(chart.title.clone()),
        );
    }

    // 底部日期刻度：间距不够就跳过，避免叠字。
    let mut label_right = f32::MIN;
    for (x, text) in &lay.ticks {
        let w = text_width(text, TICK_FONT) + 8.;
        let left = x - w / 2.;
        if left < label_right + 6. {
            continue;
        }
        label_right = left + w;
        container = container.child(
            div()
                .absolute()
                .left(px(left))
                .top(px(lay.tick_label_y))
                .w(px(w))
                .text_size(px(TICK_FONT))
                .text_color(axis_text)
                .text_center()
                .child(text.clone()),
        );
    }

    // 左侧标签：section 名（或未分组任务名）。
    for label in &lay.labels {
        container = container.child(
            div()
                .absolute()
                .left(px(PAD + 6.))
                .top(px(label.y))
                .w(px(lay.axis_x - PAD - AXIS_GAP - 6.))
                .line_height(px(label.h))
                .text_size(px(ROW_FONT))
                .text_color(if label.muted { section_text } else { task_text })
                .child(label.text.clone()),
        );
    }

    // 条形内文字：放得下就居中写在条里，放不下挪到条尾右侧，再不行才截断。
    let plot_right = lay.width - PAD;
    for bar in &lay.bars {
        let (_, _, text_color) = bars.of(bar.status);
        let tw = text_width(&bar.name, BAR_FONT);
        let (left, width, text, color, centered) = if bar.milestone {
            let right = bar.x + MS_R + 6.;
            if right + tw <= plot_right {
                (right, tw, bar.name.clone(), axis_text, false)
            } else {
                ((bar.x - MS_R - 6. - tw).max(PAD), tw, bar.name.clone(), axis_text, false)
            }
        } else if bar.w >= tw + 8. {
            (bar.x, bar.w, bar.name.clone(), text_color, true)
        } else if bar.x + bar.w + 6. + tw <= plot_right {
            (bar.x + bar.w + 6., tw, bar.name.clone(), axis_text, false)
        } else {
            (bar.x, bar.w, fit_text(&bar.name, bar.w - 8., BAR_FONT), text_color, true)
        };
        container = container.child(
            div()
                .absolute()
                .left(px(left))
                .top(px(bar.y))
                .line_height(px(BAR_H))
                .w(px(width))
                .text_size(px(BAR_FONT))
                .text_color(color)
                .when(centered, |el| el.text_center())
                .child(text),
        );
    }

    container.into_any_element()
}

struct DrawData {
    bands: Vec<(f32, f32, bool)>,
    weekends: Vec<(f32, f32)>,
    bars: Vec<(f32, f32, f32, bool, Status)>,
    ticks: Vec<f32>,
    today_x: Option<f32>,
    left: f32,
    right: f32,
    bg: Hsla,
    chart_top: f32,
    chart_bottom: f32,
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
        assert_eq!(c.tick_days, Some(7.));
        assert!(c.tasks[0].status == Status::Done);
        assert!(c.tasks[1].after == vec!["des1"]);
        assert!(c.tasks[2].status == Status::Crit);
        assert!(c.tasks[3].milestone);
        let ranges = resolve_ranges(&c);
        assert_eq!(ranges[1].0, ranges[0].1, "after 应从依赖任务结束日开始");
        assert_eq!(ranges[2].0, ranges[1].1);
    }

    /// `excludes weekends` 只影响「时长怎么数」，不压缩日历轴：
    /// 周五起 3d 要跨过周末落到周三，条形因此会压住周末带。
    #[test]
    fn workday_duration_skips_weekend_but_keeps_axis() {
        let friday = days_from_civil(2026, 1, 9);
        assert_eq!(weekday(friday), 5);
        let end = add_workdays(friday, 3);
        assert_eq!(end, days_from_civil(2026, 1, 14), "周一、周二、周三各一天");
        // 起点在周末时顺延到周一。
        assert_eq!(
            add_workdays(days_from_civil(2026, 1, 10), 1),
            days_from_civil(2026, 1, 13)
        );
    }

    #[test]
    fn weekend_spans_merge_consecutive_days() {
        let monday = days_from_civil(2026, 1, 5);
        let spans = weekend_spans(monday, monday + 13);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].1 - spans[0].0, 2);
        assert_eq!(spans[1].1 - spans[1].0, 2);
    }

    #[test]
    fn layout_keeps_bars_inside() {
        let c = parse(SAMPLE).unwrap();
        let l = layout(&c);
        assert!(l.width > 0. && l.height > 0.);
        assert_eq!(l.bands.len(), 2, "两个 section 各一条底色带");
        assert_eq!(l.labels.len(), 2, "标签列只写 section 名");
        assert_eq!(l.bars.len(), 4);
        assert!(!l.weekends.is_empty(), "excludes weekends 应产生周末带");
        for b in &l.bars {
            assert!(b.x >= 0. && b.x + b.w <= l.width + 0.5);
            assert!(b.y >= 0. && b.y + BAR_H <= l.height + 0.5);
        }
        for (x, _) in &l.ticks {
            assert!(*x >= 0. && *x <= l.width + 0.5);
        }
        // 相邻 section 的带不能重叠。
        assert!(l.bands[0].y + l.bands[0].h <= l.bands[1].y);
    }

    /// 没有 `section` 时标签列必须退回任务名，否则图上就只剩条形没有名字。
    #[test]
    fn tasks_without_section_fall_back_to_row_labels() {
        let c = parse(
            "gantt\n dateFormat YYYY-MM-DD\n 写方案 : t1, 2026-02-02, 2d\n 评审 : t2, after t1, 1d\n",
        )
        .unwrap();
        let l = layout(&c);
        assert_eq!(l.labels.len(), 2);
        assert!(l.labels.iter().all(|l| !l.muted));
    }

    /// 刻度步长跟着每天占的像素走：格子越窄，跳的天数越多。
    #[test]
    fn tick_step_follows_day_width() {
        assert_eq!(auto_tick_step(80.), 1);
        assert_eq!(auto_tick_step(30.), 3);
        assert_eq!(auto_tick_step(12.), 7);
        assert_eq!(auto_tick_step(1.), 90);
    }

    /// 条内文字永远和底色反着来：亮底深字、暗底浅字，明暗主题都成立。
    #[test]
    fn bar_text_contrast_follows_fill() {
        let (white, black) = (Hsla::white(), Hsla::black());
        assert_eq!(text_on(white, white, black, false), black);
        assert_eq!(text_on(black, white, black, false), white);
        assert_eq!(text_on(white, black, white, true), black);
        assert_eq!(text_on(black, black, white, true), white);
    }

    /// 条太窄时任务名要截断并带省略号，不能画到条外面。
    #[test]
    fn long_names_are_truncated_to_the_bar() {
        let text = fit_text("这是一个很长的任务名", 34., BAR_FONT);
        assert!(text.ends_with('…'));
        assert!(text_width(&text, BAR_FONT) <= 34.);
        // 放得下就原样返回，不加省略号。
        assert_eq!(fit_text("联调", 80., BAR_FONT), "联调");
    }

    #[test]
    fn empty_chart_is_rejected() {
        assert!(parse("gantt\n title 空计划\n").is_none());
    }
}
