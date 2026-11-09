//! `TextView` 的 Markdown 插件集合：公式与 Mermaid 流程图。
//!
//! `gpui-base` 的 Markdown 渲染器本身不支持这两类语法，但提供了
//! [`MarkdownPlugin`](gpui_kit::component::text::MarkdownPlugin) 扩展点：插件自己
//! 从 mdast 里认出节点、自己画。这里把两个插件打包成 [`with_plugins`]，
//! 消费方一行挂上即可：`with_plugins(TextView::markdown("doc", md))`。
//!
//! 公式：`$行内$` 与 `$$独立块$$`，排版见 [`math`]（TeX 子集，可读近似）。
//! 流程图：` ```mermaid ` 围栏里的 `flowchart TD`，见 [`mermaid`]。
//!
//! 本 crate 只依赖 `gpui-kit`，不碰任何业务代码；测试用的 5 张图放在
//! `tests/fixtures/diagrams.md`。

mod math;
mod mermaid;

use gpui_kit::component::text::TextView;

pub use math::{InlineMathPlugin, MathBlockPlugin};
pub use mermaid::MermaidPlugin;

/// 给 `TextView` 挂上本模块的全部 Markdown 插件。
pub fn with_plugins(view: TextView) -> TextView {
    view.plugin(MermaidPlugin)
        .plugin(MathBlockPlugin)
        .plugin(InlineMathPlugin)
}

/// 估算一段文字在给定字号下的宽度（像素）。
///
/// 不量字体：只用来给节点框、分式横线这类自绘元素定尺寸。CJK 与全角符号按 1 em、
/// 其余按 0.55 em 计——对手册里的短标签足够准，比纯字符数乘字号接近实际。
pub(crate) fn estimate_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| if is_wide(ch) { 1.0 } else { 0.55 })
        .sum::<f32>()
        * font_size
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F        // 谚文字母
        | 0x2E80..=0x303E      // CJK 部首、标点
        | 0x3041..=0x33FF      // 假名、CJK 兼容
        | 0x3400..=0x4DBF      // CJK 扩展 A
        | 0x4E00..=0x9FFF      // CJK 基本区
        | 0xA000..=0xA4CF      // 彝文
        | 0xAC00..=0xD7A3      // 谚文音节
        | 0xF900..=0xFAFF      // CJK 兼容表意文字
        | 0xFE30..=0xFE6F      // CJK 兼容形式
        | 0xFF00..=0xFF60      // 全角
        | 0xFFE0..=0xFFE6
    )
}
