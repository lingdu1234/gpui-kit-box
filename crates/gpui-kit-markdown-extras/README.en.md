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

Every diagram is drawn with native GPUI `canvas` plus absolutely positioned `div` elements; colors come from `cx.theme()` so light/dark themes work automatically. No browser, no Mermaid.js, no extra dependencies. Unknown diagrams (`erDiagram`, `pie`, …) and diagrams whose feature is disabled fall back to the default code block — **content is never swallowed**.

## Install (opt-in)

`default` is empty, so enable only what you need:

```toml
[dependencies]
gpui-kit-markdown-extras = { version = "0.1", default-features = false,
                             features = ["math", "mermaid-sequence", "mermaid-gantt"] }
```

Everything:

```toml
gpui-kit-markdown-extras = { version = "0.1", features = ["math", "mermaid"] }
```

The demo app `gpui-kit-box` in this repository uses the full configuration to render the per-diagram example pages under `showcase/`. That directory is the single source of truth: the app pages read it via cross-crate `include_str!`, and this crate's regression tests read the very same files.

## Usage

Mount all plugins available under the current feature set in one line:

```rust
use gpui_kit::component::text::TextView;
use gpui_kit_markdown_extras::with_plugins;

let view = with_plugins(TextView::markdown("doc", md));
```

Individual plugins (`MermaidPlugin` / `MathBlockPlugin` / `InlineMathPlugin`) can also be mounted on their own; each is exported only when its feature is enabled. With every feature off, `with_plugins` is an identity function and `TextView` keeps its native behavior.

## Module layout

```text
src/
├── lib.rs                   feature gates and with_plugins
├── math.rs                  math plugins
└── mermaid/
    ├── mod.rs                shared ```mermaid fence detection and keyword dispatch
    ├── common.rs             rounded elbows / dashed lines / arrows / text width
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

> Because `default = []`, running without `--all-features` (or explicit features) produces no tests. All regression tests read the same showcase pages under `showcase/`: dispatch of 5 kinds / 19 diagrams, fallback for disabled features, zero-crossing and node-penetration checks for flowcharts, class-box routing and label placement, sequence activation pairing, and layout bounds for every diagram type.

## Limitations

- Math is a **readable approximation**, not a full TeX engine; suitable for common formulas in manuals and docs, not typesetting-critical work.
- Diagram support is a practical subset, not a full Mermaid.js port: `subgraph`, `erDiagram`, `pie`, `journey`, `gitGraph` and `click` interactions are out of scope.
- `direction` is honored in class diagrams; state diagrams reuse the shared top-to-bottom layered layout and do not change direction.
- Gantt durations use `d` day / `w` week / `m` month (30 days) / `h` hour; `excludes weekends` skips weekends only, not custom holidays.

## License

[MIT](LICENSE).
