# Markdown 基础

除公式与图表之外，`TextView` 的常规 Markdown 元素示例：列表、引用、代码块与强调，样式全部跟随主题。

## 1 列表

1. 有序列表第一项
2. 第二项，含行内代码 `markdown(src).plugin(FlowchartPlugin::new())`
3. 第三项

- 无序项 A
- 无序项 B
  - 嵌套项 B.1
  - 嵌套项 B.2

## 2 引用与代码块

> 引用块用来放结论或注意事项：公式与五类图表都是原生元素，可随主题变色，也可以被选中复制。

```rust
use gpui_kit::component::text::markdown;
use gpui_kit_markdown_extras::prelude::*;

// 只挂这一页真正需要的插件；要全挂就用 with_plugins(view)
markdown(SOURCE)
    .plugin(FlowchartPlugin::new())
    .selectable(true)
```

## 3 强调与表格

**加粗**、*斜体*、~~删除线~~、`行内代码`、[链接](https://gpui-kit.com)。

| 能力 | feature | 默认 |
|---|---|---|
| 公式 | `math` | 关闭 |
| 流程图 | `mermaid-flowchart` | 关闭 |
| 时序图 | `mermaid-sequence` | 关闭 |
| 类图 | `mermaid-class` | 关闭 |
| 状态图 | `mermaid-state` | 关闭 |
| 甘特图 | `mermaid-gantt` | 关闭 |
