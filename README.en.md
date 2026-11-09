> Language / 语言: [中文](./README.md) | **English**

# gpui-kit-box

A Rust desktop UI workspace built on [gpui-kit](https://crates.io/crates/gpui-kit). This repository contains both a **demo application** and a publishable **rendering library**, used to validate and showcase formula and five Mermaid diagram types for `gpui-kit`'s `TextView` in Markdown.

> Repository: <https://github.com/lingdu1234/gpui-kit-box>

## Workspace layout

| Crate | Type | Description |
|---|---|---|
| `gpui-kit-box` | binary (app framework) | Base framework / scaffold for gpui-kit programs, for quickly building desktop apps; used here as a demo app with a single Markdown doc page showcasing math and five diagram types: flowchart, sequenceDiagram, classDiagram, stateDiagram-v2 and gantt. |
| `gpui-kit-markdown-extras` | library | Markdown extension plugins for `gpui-kit`'s `TextView`: `$…$` / `$$…$$` math and five `mermaid` diagram types, enabled per Cargo feature. To be published to crates.io. |

See each crate's `README.md` / `README.en.md`:

- [`crates/gpui-kit-box`](crates/gpui-kit-box)
- [`crates/gpui-kit-markdown-extras`](crates/gpui-kit-markdown-extras)

## Build & run

Requires Rust 1.99 or newer (uses the 2024 edition).

```bash
# Run the demo app
cargo run -p gpui-kit-box

# Run the library's tests (math parsing / all diagram layouts); the library enables no features by default
cargo test -p gpui-kit-markdown-extras --all-features
```

> On Windows, `build.rs` + `embed-resource` embeds the app resources (`.rc` / `.ico`); no extra steps needed.

## License

Code is released under the MIT License; see each crate's `LICENSE` file (`gpui-kit-markdown-extras/LICENSE`).
