> Language / 语言: [中文](./README.md) | **English**

# gpui-kit-box (base framework for gpui-kit apps)

`gpui-kit-box` is a **base framework / scaffold** for [gpui-kit](https://crates.io/crates/gpui-kit) programs, meant for quickly building desktop applications. It ships a reusable app shell: title bar, sidebar, page container, breadcrumb, plus theme and font-size switching and other common capabilities.

In this repository, `gpui-kit-box` is used as a **demo program** — it keeps only a single Markdown doc page, used to validate the formula and Mermaid flowchart rendering from `gpui-kit-markdown-extras`.

## Features (the reusable shell)

- **App shell**: title bar, sidebar, page container, breadcrumb — ready to use.
- **Theme & font size**: light / dark theme switching, font-size selector.
- **Locale**: `zh-CN`, default font `Microsoft YaHei`.
- **Doc page (demo)**: the body is embedded at compile time via `include_str!` (`docs/showcase.md`), shipped with the binary (no disk reads); supports inline `$…$` / block `$$…$$` math and the `flowchart` subset inside ` ```mermaid ` fences.

## Using it as a framework

Start from this crate's app shell and swap in your own page content to quickly build a gpui-kit program. The rendering extras (math / flowcharts) come from the dependency crate `gpui-kit-markdown-extras`.

## Run (the demo form in this repo)

```bash
cargo run -p gpui-kit-box
```

The window defaults to 1440×900. Changing the doc content requires recompiling.

## Notes

In this repository, `gpui-kit-box` exists only to demonstrate and validate rendering; it contains no real business data. It is a binary app and is **not published to crates.io**; the publishable library is `gpui-kit-markdown-extras`.
