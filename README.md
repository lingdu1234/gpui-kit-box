> 语言 / Language: **中文** | [English](./README.en.md)

# gpui-kit-box

基于 [gpui-kit](https://crates.io/crates/gpui-kit) 的 Rust 桌面 UI 工作区。本仓库同时包含**演示程序**与可独立发布的**渲染库**，用于验证并展示 `gpui-kit` 的 `TextView` 在 Markdown 场景下的公式与 Mermaid 五类图表渲染能力。

> 仓库地址：<https://github.com/lingdu1234/gpui-kit-box>

## 工作区结构

| Crate | 类型 | 说明 |
|---|---|---|
| `gpui-kit-box` | 二进制（应用框架） | gpui-kit 程序的基础框架 / 脚手架，用于快速构建桌面应用；本仓库中作为演示程序，按图表类型拆分 Markdown 示例页，展示公式与 flowchart / sequenceDiagram / classDiagram / stateDiagram-v2 / gantt 五类图渲染。 |
| `gpui-kit-markdown-extras` | 库 | 为 `gpui-kit` 的 `TextView` 提供 Markdown 扩展插件：`$…$` / `$$…$$` 数学公式与五类 `mermaid` 图表；能力按 Cargo feature 按需启用。已发布至 crates.io：<https://crates.io/crates/gpui-kit-markdown-extras> |

详见各 crate 目录下的 `README.md` / `README.en.md`：

- [`crates/gpui-kit-box`](crates/gpui-kit-box)
- [`crates/gpui-kit-markdown-extras`](crates/gpui-kit-markdown-extras)

## 构建与运行

要求 Rust 1.99 及以上（使用了 2024 edition）。

```bash
# 运行演示程序
cargo run -p gpui-kit-box

# 运行库的自带测试（公式解析 / 五类图布局）；库默认不启用任何 feature
cargo test -p gpui-kit-markdown-extras --all-features
```

> Windows 平台通过 `build.rs` + `embed-resource` 嵌入应用资源（`.rc` / `.ico`），无需额外步骤。

## 许可证

代码以 MIT 许可证发布，详见各 crate 的 `LICENSE` 文件（`gpui-kit-markdown-extras/LICENSE`）。
