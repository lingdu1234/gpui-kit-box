> 语言 / Language: **中文** | [English](./README.en.md)

# gpui-kit-box（gpui-kit 程序基础框架）

`gpui-kit-box` 是 [gpui-kit](https://crates.io/crates/gpui-kit) 程序的基础框架 / 脚手架结构，用于快速搭建桌面应用。它提供一套可复用的应用外壳：标题栏、侧边栏、页面容器、面包屑，以及主题与字号切换等通用能力。

在本仓库中，`gpui-kit-box` 作为**演示程序**使用——只保留一个 Markdown 文档页，用来验证 `gpui-kit-markdown-extras` 的公式与 Mermaid 流程图渲染效果。

## 功能（框架提供的通用外壳）

- **应用外壳**：标题栏、侧边栏、页面容器、面包屑，开箱即用。
- **主题与字号**：亮 / 暗主题切换、字号选择。
- **区域设置**：`zh-CN`，默认字体 `Microsoft YaHei`。
- **文档页（演示）**：正文在编译期通过 `include_str!` 嵌入（`docs/showcase.md`），随程序发布，不读磁盘；支持行内 `$…$` 与块 `$$…$$` 公式，以及 ` ```mermaid ` 围栏中的 `flowchart` 子集。

## 作为框架使用

以本 crate 的应用外壳为起点，替换页面内容即可快速构建自己的 gpui-kit 程序；渲染增强（公式 / 流程图）来自依赖 crate `gpui-kit-markdown-extras`。

## 运行（本仓库的演示形态）

```bash
cargo run -p gpui-kit-box
```

窗口默认尺寸 1440×900。文档内容改动后需重新编译。

## 说明

本仓库的 `gpui-kit-box` 仅用于演示与验证渲染能力，不包含任何实际业务资料。它是二进制应用，**不发布到 crates.io**；可发布的库是 `gpui-kit-markdown-extras`。
