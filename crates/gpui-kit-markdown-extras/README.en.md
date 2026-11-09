> Language / 语言: [中文](./README.md) | **English**

# gpui-kit-markdown-extras

Markdown extension plugins for GPUI's [`TextView`](https://docs.rs/gpui-kit): adds rendering for **formulas** and **Mermaid flowcharts**, which `gpui-kit` does not support natively.

> This crate only depends on `gpui-kit` and contains no business logic; it can be reused as a standalone library by any `gpui-kit`-based project.

## Features

- **Math**: inline `$…$` and block `$$…$$`. Rendered as a readable TeX-subset approximation — common commands map to Unicode, `^`/`_` become super/subscripts, `\frac` becomes a real fraction, `\sqrt` a root. Complex matrices or integral bounds safely degrade to a single line of text and never panic.
- **Mermaid flowcharts**: the `flowchart TD/TB` subset inside ` ```mermaid ` fences — `ID[text]`, `ID{text}`, `ID(text)` node shapes, `-->` / `-.->` / `==>` arrows, and `-->|text|` / `-- text -->` edge labels. Diagrams that can't be parsed (`sequenceDiagram`, `subgraph`, …) fall back to the default code block instead of being swallowed.

## Install

```toml
[dependencies]
gpui-kit-markdown-extras = "0.1"
```

## Usage

Mount all plugins on a `TextView` in one line:

```rust
use gpui_kit::component::text::TextView;
use gpui_kit_markdown_extras::with_plugins;

let view = with_plugins(TextView::markdown("doc", md));
```

You can also mount a single plugin (`MermaidPlugin` / `MathBlockPlugin` / `InlineMathPlugin`) on its own.

## Limitations

- Math is a readable approximation, not a full TeX engine; it suits common formulas in manuals/docs, not typesetting-critical use.
- Mermaid covers only the `flowchart` subset used in the manuals; sequence diagrams, class diagrams, etc. are not supported.

## License

[MIT](LICENSE).
