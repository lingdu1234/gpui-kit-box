//! `TextView` 的 Markdown 插件集合：公式与五类 Mermaid 图表。
//!
//! `gpui-base` 的 Markdown 渲染器本身不支持这些语法，但提供了
//! [`MarkdownPlugin`](gpui_kit::component::text::MarkdownPlugin) 扩展点：插件自己
//! 从 mdast 里认出节点、自己画。这里把当前 feature 组合下可用的插件打包成
//! [`with_plugins`]，消费方一行挂上即可：
//! `with_plugins(TextView::markdown("doc", md))`。
//!
//! # 按需启用（Cargo features）
//!
//! 本 crate 的 `default` 为**空**，什么都不启用；按需在依赖声明里打开：
//!
//! ```toml
//! gpui-kit-markdown-extras = { version = "0.1", default-features = false,
//!                              features = ["math", "mermaid-sequence", "mermaid-gantt"] }
//! ```
//!
//! | feature | 能力 |
//! |---|---|
//! | `math` | `$行内$` 与 `$$独立块$$` 公式（TeX 子集，可读近似） |
//! | `mermaid` | 下面五项图的快捷聚合 |
//! | `mermaid-flowchart` | `flowchart` / `graph` |
//! | `mermaid-sequence` | `sequenceDiagram` |
//! | `mermaid-class` | `classDiagram` |
//! | `mermaid-state` | `stateDiagram-v2` |
//! | `mermaid-gantt` | `gantt` |
//!
//! 未启用的图类型不会被编译；围栏里的源码会回退成默认代码块，**不会吞内容**。
//! 各图类型的解析、布局与绘制分别在 `src/mermaid/<类型>.rs`，公共几何工具在
//! `common.rs`，节点-连线图共用的分层布局在 `layout.rs`。
//!
//! 本 crate 只依赖 `gpui-kit`，不碰任何业务代码；展示文档与回归夹具统一放在
//! `showcase/`，演示程序与测试读的是同一批文件。

#[cfg(feature = "math")]
mod math;
#[cfg(any(
    feature = "mermaid-flowchart",
    feature = "mermaid-sequence",
    feature = "mermaid-class",
    feature = "mermaid-state",
    feature = "mermaid-gantt"
))]
mod mermaid;

use gpui_kit::component::text::TextView;

#[cfg(feature = "math")]
pub use math::{InlineMathPlugin, MathBlockPlugin};
#[cfg(any(
    feature = "mermaid-flowchart",
    feature = "mermaid-sequence",
    feature = "mermaid-class",
    feature = "mermaid-state",
    feature = "mermaid-gantt"
))]
pub use mermaid::MermaidPlugin;

/// 挂上当前 feature 组合下所有可用的 Markdown 插件。
///
/// 全部 feature 关闭时是恒等函数，`TextView` 保持原生行为。
pub fn with_plugins(view: TextView) -> TextView {
    #[cfg(any(
        feature = "mermaid-flowchart",
        feature = "mermaid-sequence",
        feature = "mermaid-class",
        feature = "mermaid-state",
        feature = "mermaid-gantt"
    ))]
    let view = view.plugin(MermaidPlugin);
    #[cfg(feature = "math")]
    let view = view.plugin(MathBlockPlugin).plugin(InlineMathPlugin);
    view
}

/// 估算一段文字在给定字号下的宽度（像素）。
///
/// 不量字体：只用来给节点框、分式横线这类自绘元素定尺寸。CJK 与全角符号按 1 em、
/// 其余按 0.55 em 计——对手册里的短标签足够准，比纯字符数乘字号接近实际。
/// `default = []` 时没有任何插件用到它，属于预期情况。
#[allow(dead_code)]
pub(crate) fn estimate_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| if is_wide(ch) { 1.0 } else { 0.55 })
        .sum::<f32>()
        * font_size
}

#[allow(dead_code)]
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
