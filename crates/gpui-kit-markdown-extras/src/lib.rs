//! `TextView` 的 Markdown 插件集合：公式与五类 Mermaid 图表。
//!
//! `gpui-base` 的 Markdown 渲染器本身不支持这些语法，但提供了
//! [`MarkdownPlugin`](gpui_kit::component::text::MarkdownPlugin) 扩展点：插件
//! 自己从 mdast 里认出节点、自己画。**每个插件都能单独挂**，所以能力是按视图
//! 选的，不是按 crate 选的：
//!
//! ```ignore
//! use gpui_kit::component::text::markdown;
//! use gpui_kit_markdown_extras::prelude::*;
//!
//! // 只画流程图的文档：解析时不会去试公式、时序图、类图……
//! markdown(FLOWCHART_DOC).plugin(FlowchartPlugin::new())
//!
//! // 公式文档：独立块与行内各一个插件
//! markdown(MATH_DOC)
//!     .plugin(MathBlockPlugin::new())
//!     .plugin(InlineMathPlugin::new())
//! ```
//!
//! 想一次挂上当前 feature 组合下所有插件（例如演示程序每类图一页、共用一套渲染
//! 入口），用 [`with_plugins`]。
//!
//! # 挂载顺序
//!
//! `.plugin()` 可以链任意多个，解析时按挂载顺序尝试：块级插件先到先得，行内
//! 插件同理；同名插件后挂的 renderer 会覆盖先挂的，所以每个插件在一个视图里
//! 只该挂一次。本 crate 各插件的语法互不重叠，顺序不影响结果。
//!
//! # 按需启用（Cargo features）
//!
//! 本 crate 的 `default` 为**空**，什么都不启用；按需在依赖声明里打开：
//!
//! ```toml
//! gpui-kit-markdown-extras = { version = "0.3", default-features = false,
//!                              features = ["math", "mermaid-sequence", "mermaid-gantt"] }
//! ```
//!
//! | feature | 插件 | 能力 |
//! |---|---|---|
//! | `math` | `InlineMathPlugin` / `MathBlockPlugin` | `$行内$` 与 `$$独立块$$` 公式（TeX 子集，可读近似） |
//! | `mermaid` | 下面五项 | 五个 `mermaid-*` 的快捷聚合 |
//! | `mermaid-flowchart` | `FlowchartPlugin` | `flowchart` / `graph` |
//! | `mermaid-sequence` | `SequencePlugin` | `sequenceDiagram` |
//! | `mermaid-class` | `ClassPlugin` | `classDiagram` |
//! | `mermaid-state` | `StatePlugin` | `stateDiagram-v2` |
//! | `mermaid-gantt` | `GanttPlugin` | `gantt` |
//!
//! 未启用的图类型不会被编译；围栏里的源码会回退成默认代码块，**不会吞内容**。
//! 各图类型的解析、布局与绘制分别在 `src/mermaid/<类型>.rs`，公共几何工具在
//! `common.rs`，节点-连线图共用的分层布局在 `layout.rs`，统一配色在
//! `palette.rs`（所有图只从这里取色，新增图类型也不例外）。
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

// 等比适配包装：五类 Mermaid 图与独立公式块共用（见 `fit.rs` 顶部说明）。
// 只要有任何一种需要「空间不足时整体缩小」的内容就编译。
#[cfg(any(
    feature = "math",
    feature = "mermaid-flowchart",
    feature = "mermaid-sequence",
    feature = "mermaid-class",
    feature = "mermaid-state",
    feature = "mermaid-gantt"
))]
mod fit;

use gpui_kit::component::text::TextView;

#[cfg(feature = "math")]
pub use math::{InlineMathPlugin, MathBlockPlugin};
#[cfg(feature = "mermaid-class")]
pub use mermaid::ClassPlugin;
#[cfg(feature = "mermaid-flowchart")]
pub use mermaid::FlowchartPlugin;
#[cfg(feature = "mermaid-gantt")]
pub use mermaid::GanttPlugin;
#[cfg(feature = "mermaid-sequence")]
pub use mermaid::SequencePlugin;
#[cfg(feature = "mermaid-state")]
pub use mermaid::StatePlugin;

/// 一次性导入当前 feature 组合下所有可用插件，外加 [`with_plugins`]。
///
/// ```ignore
/// use gpui_kit::component::text::markdown;
/// use gpui_kit_markdown_extras::prelude::*;
///
/// markdown(DOC).plugin(GanttPlugin::new())
/// ```
pub mod prelude {
    #[cfg(feature = "math")]
    pub use crate::math::{InlineMathPlugin, MathBlockPlugin};
    #[cfg(feature = "mermaid-class")]
    pub use crate::mermaid::ClassPlugin;
    #[cfg(feature = "mermaid-flowchart")]
    pub use crate::mermaid::FlowchartPlugin;
    #[cfg(feature = "mermaid-gantt")]
    pub use crate::mermaid::GanttPlugin;
    #[cfg(feature = "mermaid-sequence")]
    pub use crate::mermaid::SequencePlugin;
    #[cfg(feature = "mermaid-state")]
    pub use crate::mermaid::StatePlugin;
    pub use crate::with_plugins;
}

/// 挂上当前 feature 组合下所有可用的 Markdown 插件。
///
/// 等价于按本函数里的顺序链上每个 `.plugin(...)`；只想挂其中一部分（这是常见
/// 情况——某个视图只画流程图，就没必要让解析器去试公式与时序图），直接用链式
/// 写法即可。全部 feature 关闭时是恒等函数，`TextView` 保持原生行为。
pub fn with_plugins(view: TextView) -> TextView {
    #[cfg(feature = "mermaid-flowchart")]
    let view = view.plugin(FlowchartPlugin::new());
    #[cfg(feature = "mermaid-sequence")]
    let view = view.plugin(SequencePlugin::new());
    #[cfg(feature = "mermaid-class")]
    let view = view.plugin(ClassPlugin::new());
    #[cfg(feature = "mermaid-state")]
    let view = view.plugin(StatePlugin::new());
    #[cfg(feature = "mermaid-gantt")]
    let view = view.plugin(GanttPlugin::new());
    #[cfg(feature = "math")]
    let view = view
        .plugin(MathBlockPlugin::new())
        .plugin(InlineMathPlugin::new());
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

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use gpui_kit::component::text::MarkdownPlugin as _;

    /// 插件名是 `MarkdownExtensions` 里 renderer 表的键：同名会互相覆盖，挂两个
    /// 等于只挂上一个。`with_plugins` 会同时挂上这里的全部插件，所以名字必须互不
    /// 相同；单独链式挂载时同理。
    #[test]
    fn plugin_names_are_unique() {
        // `name()` 借用插件本身，所以这里拷成 `String` 再比较，避免把临时值
        // 的借用带出语句。
        let mut names: Vec<String> = Vec::new();
        #[cfg(feature = "math")]
        {
            names.push(crate::MathBlockPlugin::new().name().to_string());
            names.push(crate::InlineMathPlugin::new().name().to_string());
        }
        #[cfg(feature = "mermaid-flowchart")]
        names.push(crate::FlowchartPlugin::new().name().to_string());
        #[cfg(feature = "mermaid-sequence")]
        names.push(crate::SequencePlugin::new().name().to_string());
        #[cfg(feature = "mermaid-class")]
        names.push(crate::ClassPlugin::new().name().to_string());
        #[cfg(feature = "mermaid-state")]
        names.push(crate::StatePlugin::new().name().to_string());
        #[cfg(feature = "mermaid-gantt")]
        names.push(crate::GanttPlugin::new().name().to_string());

        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "插件名重复：后挂的 renderer 会覆盖先挂的");
    }

    /// 目标用法就是链式：`markdown(src).plugin(XxxPlugin::new())`。这里守住它
    /// 在任意 feature 组合下都能编译——`markdown()` 是 `TextView` 的自由函数
    /// 版本，id 取调用点代码位置，适合一次性视图。
    #[test]
    fn chained_plugins_compile() {
        #[cfg(feature = "math")]
        {
            let _ = gpui_kit::component::text::markdown("$E = mc^2$")
                .plugin(crate::MathBlockPlugin::new())
                .plugin(crate::InlineMathPlugin::new());
        }
        #[cfg(feature = "mermaid-flowchart")]
        {
            let _ = gpui_kit::component::text::markdown("```mermaid\nflowchart TD\n  A-->B\n```")
                .plugin(crate::FlowchartPlugin::new());
        }
        #[cfg(feature = "mermaid-gantt")]
        {
            let _ = gpui_kit::component::text::markdown("```mermaid\ngantt\n  title x\n```")
                .plugin(crate::GanttPlugin::new());
        }
    }
}
