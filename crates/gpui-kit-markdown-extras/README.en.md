> Language / 语言: [中文](./README.md) | **English**

# gpui-kit-markdown-extras

Markdown extension plugins for GPUI's [`TextView`](https://docs.rs/gpui-kit): adds **math formulas** and **five Mermaid diagram types** that `gpui-kit` does not support natively.

> This crate only depends on `gpui-kit` and contains no business logic. Every capability is a separate Cargo feature and **all features are off by default**; diagram types you do not enable are not compiled, and their fenced source falls back to a plain code block.

## Capabilities

| Feature | What it adds | Coverage |
|---|---|---|
| `math` | Formulas | Inline `$…$` and block `$$…$$`; readable TeX-subset approximation: fractions / roots / scripts, matrices and `cases`, `aligned` / `gathered` environments, `\overset` / `\underset` / `\stackrel`, `\boxed`, `\mathbb` / `\mathcal` / `\mathfrak`, and more |
| `mermaid-flowchart` | Flowcharts | `flowchart` / `graph`; 14 node shapes (rect / round / stadium / cylinder / circle / double-circle / subroutine / hexagon / diamond / parallelogram / trapezoid / flag, …), solid / dotted / thick lines, circle `--o` and cross `--x` ends, both edge-label forms, `&` fan-out and fan-in, `subgraph` (edges anchor on the frame border), `LR` / `RL` / `BT` directions, `~~~` invisible links; long cross-rank edges route through side lanes to avoid crossings |
| `mermaid-sequence` | Sequence diagrams | `participant` / `actor`; 10 forward arrow forms (including bidirectional `<<->>` / `<<-->>`) plus 4 reverse arrows (extension syntax); `autonumber [start] [step]`; `+` / `-` shortcut activation and explicit `activate` / `deactivate` pairing; `Note left/right/over`; `create` / `destroy`; `loop` / `alt` / `else` / `opt` / `par` / `and` / `break` / `critical` / `rect` / `box` fragments and `[guards]` |
| `mermaid-class` | Class diagrams | Multi-line members and visibility, `<<interface>>`, generics `~T~`, `direction TB/BT/LR/RL`, seven relation kinds (inheritance, realization, composition, aggregation, association, dependency, link) and self-relations, multiplicities, `note for`; `classDef` / `cssClass` / `style` / `click` directives are recognized and skipped, never mis-parsed as classes |
| `mermaid-state` | State diagrams | `stateDiagram-v2`, `[*]` terminals, `state "name" as id` and `state X: description`, composite states, `<<choice>>` / `<<fork>>` / `<<join>>` (sync bar only for machine ids, named boxes otherwise), `direction TB/LR/RL/BT`, `--` concurrent regions, `note left/right of` |
| `mermaid-gantt` | Gantt charts | `title` / `dateFormat` / `axisFormat` / `tickInterval` / `todayMarker` / `section`; duration units `ms` / `s` / `m` (minutes) / `h` / `d` / `w` / `M` (months) / `y`, case-sensitive, decimals allowed; `excludes weekends` plus specific holidays (multi-line accumulation, merged with adjacent weekends into one band); `done` / `active` / `crit` / `milestone`; explicit dates and `after` dependencies (multiple predecessors allowed) |
| `mermaid` | Convenience bundle | Enables all five diagram features above |

Every diagram is drawn with native GPUI `canvas` plus absolutely positioned `div` elements. All colors are derived from `cx.theme()` by a single shared palette (`mermaid/palette.rs`): the five diagram kinds all take colors from the same theme roles, and a source-scanning test keeps new diagram types on that path, so every diagram shares one color scale in both light and dark themes. No browser, no Mermaid.js, no extra dependencies.

Diagrams and formula blocks **scale down proportionally** (text included) when the container is too small — no clipping, no overflow (`fit.rs`).

Unknown diagrams (`erDiagram`, `pie`, …) and diagrams whose feature is disabled fall back to the default code block — **content is never swallowed**. Showcase syntax stays compatible with official Mermaid: the same source renders on GitHub / VS Code / mermaid.live.

## Install (opt-in)

`default` is empty, so enable only what you need:

```toml
[dependencies]
gpui-kit-markdown-extras = { version = "0.3", default-features = false,
                             features = ["math", "mermaid-sequence", "mermaid-gantt"] }
```

Everything:

```toml
gpui-kit-markdown-extras = { version = "0.3", features = ["math", "mermaid"] }
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
├── fit.rs                   proportional fit wrapper: shrink whole diagram (text included) when space is tight
└── mermaid/
    ├── mod.rs               five per-diagram plugins plus shared fence detection
    ├── common.rs            rounded elbows / dashed lines / arrows / text width
    ├── palette.rs           shared palette: the only place diagrams take colors from
    ├── layout.rs            layered node-link layout + LR/RL/BT direction transforms (shared by flowchart + state)
    ├── node_link.rs         node-link drawing (shared by flowchart + state)
    ├── flowchart.rs         flowchart parser and regression tests
    ├── sequence.rs          sequence diagram: lifeline layout
    ├── class.rs             class diagram: variable-height box layout
    ├── state.rs             state diagram parser
    └── gantt.rs             Gantt: dependency-free date math and timeline
```

## Tests

```bash
cargo test -p gpui-kit-markdown-extras --all-features
```

> Because `default = []`, running without `--all-features` (or explicit features) produces no tests. All regression tests read the same showcase pages under `showcase/` (5 kinds / 29 diagrams): dispatch checks, fallback for disabled features, zero-crossing and node-penetration checks for flowcharts, LR / RL / BT reorientation bounds, class-box routing and label placement, sequence activation pairing and text overlap checks, concurrent regions wrapped by their frame, Gantt workday math and excluded-day band merging, and layout bounds for every diagram type — plus palette regressions: accent contrast in light/dark themes and a source scan asserting no renderer bypasses `palette.rs`.

## Limitations

- Math is a **readable approximation**, not a full TeX engine; suitable for common formulas in manuals and docs, not typesetting-critical work.
- Diagram support is a practical subset, not a full Mermaid.js port: `erDiagram`, `pie`, `journey`, `gitGraph`, mindmaps and `click` interactions are out of scope.
- `TB` / `LR` / `RL` / `BT` directions in flowcharts and state diagrams are realized as geometric transforms applied after layout; under non-`TB` directions edges follow a generic re-routing whose details differ slightly from `TB`.
- The 4 reverse arrows of sequence diagrams (`<-` / `<--` / `<<-` / `<<--`) are an extension of this crate, not part of official Mermaid; prefer forward arrows in docs meant to render everywhere.
- Styling directives in class and state diagrams (`classDef` / `cssClass` / `style` / `click`, …) are recognized and skipped, not applied.
- State diagrams follow Mermaid's conventions: `[*]` renders as a filled start dot or a ringed end marker, `<<choice>>` as a hollow diamond, and `<<fork>>`/`<<join>>` as a sync bar only when the name is a machine id such as `fork1`, otherwise as a named state box.
- The gantt axis is always calendar-based (one column per day) and task names sit centered inside their bars, moving to the bar's right end when they do not fit, and truncating only as a last resort.

## License

[MIT](LICENSE).
