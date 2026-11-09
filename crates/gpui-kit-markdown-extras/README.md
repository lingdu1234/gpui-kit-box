> 语言 / Language: **中文** | [English](./README.en.md)

# gpui-kit-markdown-extras

GPUI [`TextView`](https://docs.rs/gpui-kit) 的 Markdown 扩展插件集合：把 `gpui-kit` 原生不支持的**公式**与 **Mermaid 流程图**渲染补上。

> 本 crate 只依赖 `gpui-kit`，不碰任何业务代码；可作为独立库被任意基于 gpui-kit 的项目复用。

## 功能

- **数学公式**：行内 `$…$` 与独立块 `$$…$$`。排版为可读的 TeX 子集近似实现——常见命令转 Unicode、`^`/`_` 上下标、`\frac` 分式、`\sqrt` 根号。复杂矩阵、积分上下限等会安全退化为一行文字，不会 panic。
- **Mermaid 流程图**：` ```mermaid ` 围栏中的 `flowchart TD/TB` 子集——支持 `ID[文本]`、`ID{文本}`、`ID(文本)` 三种节点形状，`-->` / `-.->` / `==>` 三种箭头，以及 `-->|文字|` 和 `-- 文字 -->` 两种边标注。解析不了的图（如 `sequenceDiagram`、`subgraph`）自动回退到默认代码块，不会吞内容。

## 安装

```toml
[dependencies]
gpui-kit-markdown-extras = "0.1"
```

## 用法

一行把全部插件挂到 `TextView` 上：

```rust
use gpui_kit::component::text::TextView;
use gpui_kit_markdown_extras::with_plugins;

let view = with_plugins(TextView::markdown("doc", md));
```

也可以单独挂载某个插件（`MermaidPlugin` / `MathBlockPlugin` / `InlineMathPlugin`）。

## 限制

- 公式不是完整 TeX 引擎，而是**可读近似**，适合手册、文档中的常见公式；请勿用于需要精确排版的场景。
- Mermaid 只覆盖手册实际用到的 `flowchart` 子集，不支持时序图、类图等其他图表类型。

## 许可证

[MIT](LICENSE)。版权归作者所有。
