> 语言 / Language: **中文** | [English](./README.en.md)

# gpui-kit-markdown-extras

GPUI [`TextView`](https://docs.rs/gpui-kit) 的 Markdown 扩展插件集合：补上 `gpui-kit` 原生不支持的**数学公式**与 **Mermaid 五类图表**。

> 本 crate 只依赖 `gpui-kit`，不碰业务代码。每项能力是一个独立 Cargo feature，**默认全部关闭**，按需启用；未启用的图类型不会被编译，围栏源码会原样回退成代码块。

## 功能

| feature | 能力 | 覆盖范围 |
|---|---|---|
| `math` | 公式 | 行内 `$…$` 与独立块 `$$…$$`；TeX 子集可读近似（`\frac`、`\sqrt`、上下标、常用命令） |
| `mermaid-flowchart` | 流程图 | `flowchart` / `graph`，三种节点形状、三种箭头、两种边标注；跨层长边走侧通道避免交叉 |
| `mermaid-sequence` | 时序图 | `participant` / `actor`、8 种消息箭头、`+`/`-` 激活、`Note left/right/over`、`autonumber`、`loop`/`alt`/`else`/`opt`/`par`/`critical`/`rect`/`box` 片段框 |
| `mermaid-class` | 类图 | 多行成员与可见性、`<<interface>>` 注解、`direction TB/BT/LR/RL`、继承/组合/聚合/关联/依赖/实现/链接七类关系、多重度、`note for` |
| `mermaid-state` | 状态图 | `stateDiagram-v2`、`[*]` 起止、`state "名" as id`、复合状态、`<<choice>>`/`<<fork>>`/`<<join>>`、`note left/right of` |
| `mermaid-gantt` | 甘特图 | `title` / `dateFormat` / `axisFormat` / `tickInterval` / `excludes weekends` / `todayMarker` / `section`，`done`/`active`/`crit`/`milestone`，显式日期与 `after` 依赖 |
| `mermaid` | 快捷聚合 | 等价于同时打开上面五项 |

所有图都用 GPUI 原生 `canvas` + 绝对定位 `div` 绘制。配色统一由 `mermaid/palette.rs` 从 `cx.theme()` 推导：把状态图那套「主题强调色 + 逐级透明度」沉淀成公共设施，流程图、时序图、类图、甘特图都从同一组角色取色，新增图类型也不例外（有测试扫描源码守住这条约定），因此五类图的线条、边框、文字在明暗主题下始终是同一套色阶。不依赖浏览器、Mermaid.js 或任何新依赖。解析不了的图（如 `erDiagram`、`pie`）与未启用 feature 的图都会回退成默认代码块，**不会吞内容**。

## 安装（按需启用）

`default` 为空，只装要用的能力：

```toml
[dependencies]
gpui-kit-markdown-extras = { version = "0.2", default-features = false,
                             features = ["math", "mermaid-sequence", "mermaid-gantt"] }
```

全开：

```toml
gpui-kit-markdown-extras = { version = "0.2", features = ["math", "mermaid"] }
```

本仓库的演示程序 `gpui-kit-box` 就采用全开配置，用来展示 `showcase/` 下各类图表的示例页。该目录是展示文档的唯一来源：应用页通过跨 crate `include_str!` 读取，本 crate 的回归测试读同一批文件，不再维护副本。

## 用法

每个插件单独挂到视图上，`plugin()` 是链式的，**能力按视图选**：

```rust
use gpui_kit::component::text::markdown;
use gpui_kit_markdown_extras::prelude::*;

// 只画流程图：解析时不会去试公式、时序图、类图……
markdown(FLOWCHART_DOC).plugin(FlowchartPlugin::new())

// 公式文档：独立块与行内各挂一个插件
markdown(MATH_DOC)
    .plugin(MathBlockPlugin::new())
    .plugin(InlineMathPlugin::new())
```

`prelude` 按 feature 导出当前可用的插件（`FlowchartPlugin` / `SequencePlugin` / `ClassPlugin` / `StatePlugin` / `GanttPlugin` / `MathBlockPlugin` / `InlineMathPlugin`），未启用的 feature 对应的名字根本不存在，误用会直接编译失败。

想一次挂上当前 feature 组合下全部插件（演示程序里每类图一页、共用一套渲染入口就是这种场景），用 `with_plugins`：

```rust
use gpui_kit::component::text::TextView;
use gpui_kit_markdown_extras::with_plugins;

let view = with_plugins(TextView::markdown("doc", md));
```

`with_plugins` 等价于按顺序链上每个 `.plugin(...)`；全部 feature 关闭时是恒等函数，`TextView` 保持原生行为。插件在 `render` 里每帧重建是零成本的：`gpui-base` 判断扩展能否影响解析树时只比 parser 的形状（数量与名字），不比重建出来的对象身份，不会因此重新解析文档。

## 模块结构

```text
src/
├── lib.rs                    feature 门控、插件导出与 with_plugins 聚合
├── math.rs                   公式插件
└── mermaid/
    ├── mod.rs                五个按图类型划分的插件 + 共用的围栏识别
    ├── common.rs             圆角折线 / 虚线 / 箭头 / 文本宽度
    ├── palette.rs            统一配色：所有图只从这里取色
    ├── layout.rs             节点-连线图分层布局（flowchart + state 共用）
    ├── node_link.rs          节点-连线图绘制（flowchart + state 共用）
    ├── flowchart.rs          flowchart 解析与回归测试
    ├── sequence.rs           时序图：生命线布局
    ├── class.rs              类图：可变高度类框布局
    ├── state.rs              状态图解析
    └── gantt.rs              甘特图：纯 Rust 日期换算与时间轴
```

## 测试

```bash
cargo test -p gpui-kit-markdown-extras --all-features
```

> 由于 `default = []`，不带 `--all-features`（或显式 features）时没有测试可跑。回归测试统一读取 `showcase/` 下的展示文档：5 类 19 张图的分发校验、未启用 feature 的回退校验、流程图零交叉/不穿框、类图不穿框与标签归位、时序图激活配对、各图布局边界检查；另有配色回归——强调色在明暗主题下的对比度，以及「渲染模块不许绕过 `palette.rs` 直接取主题色」的源码扫描。

## 限制

- 公式是**可读近似**，不是完整 TeX 引擎；适合手册与文档中的常见公式，不用于需要精确排版的场景。
- 图表覆盖的是常用语法子集，不是 Mermaid.js 的完整实现：`subgraph`、`erDiagram`、`pie`、`journey`、`gitGraph`、`flowchart` 的 `click` 交互等不在范围内。
- `direction` 在类图中生效；状态图沿用共用的自上而下分层布局器，不改变排布方向。
- 状态图按 Mermaid 的画法渲染：`[*]` 是实心起点 / 圆环终点，`<<choice>>` 是空心菱形，`<<fork>>`/`<<join>>` 只在名字是内部标识符（如 `fork1`）时才画同步条，否则画成带名字的节点。
- 甘特图时长后缀按 `d` 天 / `w` 周 / `m` 月（30 天）/ `h` 小时换算；`excludes weekends` 把周末画成淡色竖带、时长只数工作日，不含自定义节假日。
- 甘特图横轴始终是日历轴（一天一格），任务名居中写在条形里；放不下时挪到条尾右侧，再放不下才截断。

## 许可证

[MIT](LICENSE)。版权归作者所有。
