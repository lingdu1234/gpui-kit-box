//! Mermaid 图表的统一配色：所有图只从这一处取色。
//!
//! 之前每种图各自从主题里挑 token：流程图灰框灰线、时序图 `muted_foreground`
//! 细线、类图青色 `info`、状态图蓝色 `chart_2`——同一页文档放几张图，线条颜色
//! 互不相干。这里把状态图那套「主题蓝强调色 + 逐级透明度」沉淀成公共设施，
//! 其余图按角色取色，新增图类型也只允许往这里加角色。
//!
//! | 角色 | 取值 | 用在哪 |
//! |---|---|---|
//! | `background` | `theme.background` | 页底色：空心节点、标签遮挡底 |
//! | `surface` | `theme.muted` | 节点 / 参与者 / 类框底色 |
//! | `accent` | `theme.chart_2` | 强调色原色：起止圆点、同步条、甘特进行中 |
//! | `outline` | accent 85% | 节点框、类框、分隔线、端点符号 |
//! | `line` | accent 72% | 关系线、消息线、生命线、箭头 |
//! | `frame_outline` | accent 70% | 复合状态 / 组合片段外框 |
//! | `frame_fill` | accent 5% | 外框底纹 |
//! | `header_fill` | accent 14% | 复合状态标题栏 |
//! | `note_fill` / `note_outline` | `muted` / accent 40% | 注释框与引线 |
//! | `text` / `text_muted` | `foreground` / `muted_foreground` | 正文 / 次级文字 |
//! | `success` `warning` `danger` `info` | 同名主题 token | 甘特状态、告警等语义场景 |
//!
//! 落地约定：渲染模块（`node_link` / `sequence` / `class` / `gantt`，以及以后
//! 新增的图）**不许**直接摸 `theme.xxx` 取色，只能经 [`DiagramPalette`] 拿角色。
//! `renderers_take_colors_only_from_this_module` 会扫描源码守住这条约定。

use gpui_kit::App;
use gpui_kit::Hsla;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::theme::ThemeColor;

/// 一套按语义命名的图表配色：几何用线色，文字用文字色，底纹用低透明度强调色。
#[derive(Clone, Copy)]
pub(super) struct DiagramPalette {
    /// 页底色：空心节点填充、贴线标签的遮挡底。
    pub background: Hsla,
    /// 弱表面：节点框 / 参与者框 / 类框 / 注释框底色。
    pub surface: Hsla,
    /// 强调色原色：起止圆点、同步条、甘特「进行中」条形。
    pub accent: Hsla,
    /// 框线：节点框、类框、分隔线（accent 85%）。
    pub outline: Hsla,
    /// 连线：关系线、消息线、生命线、箭头（accent 72%）。
    pub line: Hsla,
    /// 容器外框：复合状态、组合片段（accent 70%）。
    pub frame_outline: Hsla,
    /// 容器底纹（accent 5%）。
    pub frame_fill: Hsla,
    /// 容器标题栏（accent 14%）。
    pub header_fill: Hsla,
    /// 正文：类名、成员、节点名、参与者名。
    pub text: Hsla,
    /// 次级文字：注释、通道标签、多重度、坐标轴。
    pub text_muted: Hsla,
    /// 注释框底色。
    pub note_fill: Hsla,
    /// 注释框描边与引线（accent 40%）。
    pub note_outline: Hsla,
    /// 语义色：已完成。
    pub success: Hsla,
    /// 语义色：待办 / 警告。当前没有图用到，随公共调色板一起保留，
    /// 后续加新图时不要再另挑一个主题色。
    #[allow(dead_code)]
    pub warning: Hsla,
    /// 语义色：关键路径 / 今日标记。
    pub danger: Hsla,
    /// 语义色：提示。同上，预留。
    #[allow(dead_code)]
    pub info: Hsla,
}

impl DiagramPalette {
    /// 从主题推导整套配色。纯函数，便于在明 / 暗两套主题下做回归测试。
    pub(super) fn new(theme: &ThemeColor) -> Self {
        // 自定义主题可能把 `chart_2` 调回和弱化边框一样淡：依次退回 info、
        // 前景色，保证强调色在任何主题下都看得见。
        let accent = [theme.chart_2, theme.info, theme.foreground]
            .into_iter()
            .find(|c| *c != theme.border && *c != theme.muted_foreground)
            .unwrap_or(theme.foreground);
        Self {
            background: theme.background,
            surface: theme.muted,
            accent,
            outline: accent.opacity(0.85),
            line: accent.opacity(0.72),
            frame_outline: accent.opacity(0.70),
            frame_fill: accent.opacity(0.05),
            header_fill: accent.opacity(0.14),
            text: theme.foreground,
            text_muted: theme.muted_foreground,
            note_fill: theme.muted,
            note_outline: accent.opacity(0.40),
            success: theme.success,
            warning: theme.warning,
            danger: theme.danger,
            info: theme.info,
        }
    }

    /// 在页底色上叠一层前景色，得到「比表面更淡」的底纹（甘特交替带、竖网格）。
    ///
    /// 淡色底纹必须先在页底色上拍平再画：半透明色会透出底下的周末竖带，
    /// 看上去像带纹理的脏色。
    pub(super) fn veil(&self, opacity: f32) -> Hsla {
        self.background.blend(self.text.opacity(opacity))
    }

    /// 把一个语义色按透明度拍平到页底色（甘特「已完成」淡绿等）。
    pub(super) fn tint(&self, color: Hsla, opacity: f32) -> Hsla {
        self.background.blend(color.opacity(opacity))
    }
}

/// 渲染层的唯一取色入口。
pub(super) fn of(cx: &App) -> DiagramPalette {
    DiagramPalette::new(cx.theme())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_visible_in_light_and_dark_themes() {
        for theme in [ThemeColor::light(), ThemeColor::dark()] {
            let p = DiagramPalette::new(&theme);
            assert_eq!(p.accent, theme.chart_2, "强调色应取主题蓝");
            assert_eq!(p.text, theme.foreground);
            assert_eq!(p.text_muted, theme.muted_foreground);
            for (name, c) in [
                ("outline", p.outline),
                ("line", p.line),
                ("frame_outline", p.frame_outline),
                ("note_outline", p.note_outline),
            ] {
                assert!(
                    (c.h - p.accent.h).abs() < 1e-4,
                    "{name} 必须与强调色同色相，否则又会出现第二种线条颜色"
                );
                assert_ne!(c, theme.border, "{name} 不能退回弱对比的 border");
                assert_ne!(c, theme.muted_foreground, "{name} 不能退回弱化文字色");
            }
            let gap = (p.accent.l - theme.background.l).abs();
            assert!(gap > 0.15, "强调色与页底色的亮度差只有 {gap:.2}，太淡了");
        }
    }

    /// 公共设施的硬约束：渲染模块不许直接摸 `theme.xxx` 取色。
    ///
    /// 新增图类型时若绕开本模块，同一个文档里的图会再次出现「各自的蓝」；
    /// 这条扫描让越界在 CI 里就暴露，而不是等到看截图才发现。
    #[test]
    fn renderers_take_colors_only_from_this_module() {
        const SOURCES: &[(&str, &str)] = &[
            ("node_link.rs", include_str!("node_link.rs")),
            ("sequence.rs", include_str!("sequence.rs")),
            ("class.rs", include_str!("class.rs")),
            ("gantt.rs", include_str!("gantt.rs")),
        ];
        for (name, src) in SOURCES {
            for (ix, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                let Some(at) = line.find("theme.") else {
                    continue;
                };
                let rest = &line[at + "theme.".len()..];
                assert!(
                    rest.starts_with("is_dark()"),
                    "{name}:{} 绕过公共配色直接从主题取色：{}",
                    ix + 1,
                    line.trim()
                );
            }
        }
    }
}
