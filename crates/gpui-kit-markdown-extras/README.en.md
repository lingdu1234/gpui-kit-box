> Language / 语言: [中文](./README.md) | **English**

# gpui-kit-markdown-extras

Markdown extension plugins for GPUI's [`TextView`](https://docs.rs/gpui-kit): adds **math formulas** and **five Mermaid diagram types** that `gpui-kit` does not support natively.

> This crate only depends on `gpui-kit` and contains no business logic. Every capability is a separate Cargo feature and **all features are off by default**; diagram types you do not enable are not compiled, and their fenced source falls back to a plain code block.

## Capabilities

| Feature | What it adds | Coverage |
|---|---|---|
| `math` | Formulas | Inline `$…$` and block `$$…$$`; readable TeX-subset approximation (`\frac`, `\sqrt`, scripts, common commands) |
| `mermaid-flowchart` | Flowcharts | `flowchart` / `graph`, three node shapes, three arrow styles, two edge-label forms; long edges route through side lanes to avoid crossings |
| `mermaid-sequence` | Sequence diagrams | `participant` / `actor`, eight message arrows, `+`/`-` activation, `Note left/right/over`, `autonumber`, `loop`/`alt`/`else`/`opt`/`par`/`critical`/`rect`/`box` fragments |
| `mermaid-class` | Class diagrams | Multi-line members and visibility, `<<interface>>`, `direction TB/BT/LR/RL`, seven relation kinds (inheritance, composition, aggregation, association, dependency, realization, link), multiplicities, `note for` |
| `mermaid-state` | State diagrams | `stateDiagram-v2`, `[*]` terminals, `state "name" as id`, composite states, `<<choice>>`/`<<fork>>`/`<<join>>`, `note left/right of` |
| `mermaid-gantt` | Gantt charts | `title` / `dateFormat` / `axisFormat` / `tickInterval` / `excludes weekends` / `todayMarker` / `section`, `done`/`active`/`crit`/`milestone`, explicit dates and `after` dependencies |
| `mermaid` | Convenience bundle | Enables all five diagram features above |

Every diagram is drawn with native GPUI `canvas` plus absolutely positioned `div` elements. All colors are derived from `cx.theme()` by a single shared palette (`mermaid/palette.rs`): the state-diagram scheme of one theme accent plus graduated opacities is now a common facility, and flowcharts, sequence diagrams, class diagrams and Gantt charts all take colors from the same roles. A source-scanning test keeps new diagram types on that path, so every diagram shares one color scale in both light and dark themes. No browser, no Mermaid.js, no extra dependencies. Unknown diagrams (`erDiagram`, `pie`, …) and diagrams whose feature is disabled fall back to the default code block — **content is never swallowed**.

## Install (opt-in)

`default` is empty, so enable only what you need:

```toml
[dependencies]
gpui-kit-markdown-extras = { version = "0.2", default-features = false,
                             features = ["math", "mermaid-sequence", "mermaid-gantt"] }
```

Everything:

```toml
gpui-kit-markdown-extras = { version = "0.2", features = ["math", "mermaid"] }
```

The demo app `gpui-kit-box` in this repository uses the full configuration to render the per-diagram example pages under `showcase/`. That directory is the single source of truth: the app pages read it via cross-crate `include_str!`, and this crate's regression tests read the very same files.

## Usage

Each plugin is mounted on its own with the chainable `plugin()`, so capability is chosen per view:

```rust
use gpui_kit::component::text::markdown;
use gpui_kit_markdown_extras::prelude::*;

// Flowchart only: the parser never tries math, sequence, class, ...
markdown(FLOWCHART_DOC).plugin(FlowchartPlugin::new())

// A math document: one plugin for display blocks, one for inline
markdown(MATH_DOC)
    .plugin(MathBlockPlugin::new())
    .plugin(InlineMathPlugin::new())
```

`prelude` exports the plugins available under the current feature set (`FlowchartPlugin` / `SequencePlugin` / `ClassPlugin` / `StatePlugin` / `GanttPlugin` / `MathBlockPlugin` / `InlineMathPlugin`). Names behind a disabled feature do not exist, so misuse fails at compile time.

To mount everything available in one call — as the demo app does, sharing one render path across per-diagram pages — use `with_plugins`:

```rust
use gpui_kit::component::text::TextView;
use gpui_kit_markdown_extras::with_plugins;

let view = with_plugins(TextView::markdown("doc", md));
```

`with_plugins` is exactly the equivalent chain of `.plugin(...)` calls. With every feature off it is an identity function and `TextView` keeps its native behavior. Rebuilding plugins every frame inside `render` costs nothing: `gpui-base` compares the parser shape (count and names) rather than object identity to decide whether the document needs reparsing.

## Module layout

```text
src/
├── lib.rs                   feature gates, plugin exports, with_plugins
├── math.rs                  math plugins
└── mermaid/
    ├── mod.rs                five per-diagram plugins plus shared fence detection
    ├── common.rs             rounded elbows / dashed lines / arrows / text width
    ├── palette.rs            shared palette: the only place diagrams take colors from
    ├── layout.rs             layered node-link layout (shared by flowchart + state)
    ├── node_link.rs          node-link drawing (shared by flowchart + state)
    ├── flowchart.rs          flowchart parser and regression tests
    ├── sequence.rs           sequence diagram: lifeline layout
    ├── class.rs              class diagram: variable-height box layout
    ├── state.rs              state diagram parser
    └── gantt.rs              Gantt: dependency-free date math and timeline
```

## Tests

```bash
cargo test -p gpui-kit-markdown-extras --all-features
```

> Because `default = []`, running without `--all-features` (or explicit features) produces no tests. All regression tests read the same showcase pages under `showcase/`: dispatch of 5 kinds / 19 diagrams, fallback for disabled features, zero-crossing and node-penetration checks for flowcharts, class-box routing and label placement, sequence activation pairing, layout bounds for every diagram type, plus palette regressions: accent contrast in light/dark themes and a source scan asserting no renderer bypasses `palette.rs`.

## Limitations

- Math is a **readable approximation**, not a full TeX engine; suitable for common formulas in manuals and docs, not typesetting-critical work.
- Diagram support is a practical subset, not a full Mermaid.js port: `subgraph`, `erDiagram`, `pie`, `journey`, `gitGraph` and `click` interactions are out of scope.
- `direction` is honored in class diagrams; state diagrams reuse the shared top-to-bottom layered layout and do not change direction.
- State diagrams follow Mermaid's conventions: `[*]` renders as a filled start dot or a ringed end marker, `<<choice>>` as a hollow diamond, and `<<fork>>`/`<<join>>` as a sync bar only when the name is a machine id such as `fork1`, otherwise as a named state box.
- Gantt durations use `d` day / `w` week / `m` month (30 days) / `h` hour; `excludes weekends` shades weekends as light bands and counts working days only, and custom holidays are not supported.
- The gantt axis is always calendar-based (one column per day) and task names sit centered inside their bars, moving to the bar's right end when they do not fit, and truncating only as a last resort.

## License

[MIT](LICENSE).
