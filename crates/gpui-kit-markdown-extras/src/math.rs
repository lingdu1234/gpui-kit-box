//! Markdown 插件：公式（`$…$` / `$$…$$`）渲染。
//!
//! `gpui-base` 的解析器已经打开了 `math_text` / `math_flow` 两个 construct，
//! 所以 mdast 会直接给出 `InlineMath` / `Math` 节点，这里只管排版。
//!
//! 排版不是完整的 TeX 引擎，而是一个**可读的近似实现**：把常见的命令换成
//! Unicode 符号，`^` / `_` 排成上下标，`\frac` 排成真正的上下分式，
//! `\sqrt` 排成根号。复杂的矩阵、积分上下限等会退化成一行文字，不会崩。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, Styled, Window, div, px,
};
use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};

use super::estimate_width;

/// 行内公式：`$E = mc^2$`。
pub struct InlineMathPlugin;

/// 独立公式块：`$$\bar{x} = \frac{\sum x_i}{n}$$`。
pub struct MathBlockPlugin;

impl InlineMathPlugin {
    /// 插件没有可配置项，构造是零成本的。保留 `new()` 是为了调用点写法统一
    /// （`markdown(src).plugin(InlineMathPlugin::new())`），也给将来加配置留位置。
    pub fn new() -> Self {
        Self
    }
}

impl Default for InlineMathPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MathBlockPlugin {
    /// 同 [`InlineMathPlugin::new`]：零成本构造，只为调用点写法统一。
    pub fn new() -> Self {
        Self
    }
}

impl Default for MathBlockPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownPlugin for InlineMathPlugin {
    fn name(&self) -> &str {
        "inline-math"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::InlineMath(math) = node else {
            return None;
        };
        Some(
            MarkdownNode::new("inline-math", MathSource(math.value.clone()))
                .text(format!("${}$", math.value))
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let source = node.data::<MathSource>().map(|s| s.0.as_str()).unwrap_or("");
        h_flex()
            .items_center()
            .child(math_flow(source, 14., false, cx))
    }
}

impl MarkdownPlugin for MathBlockPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "math-block"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Math(math) = node else {
            return None;
        };
        Some(
            MarkdownNode::new("math-block", MathSource(math.value.clone()))
                .text(format!("$${}$$", math.value))
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let source = node.data::<MathSource>().map(|s| s.0.as_str()).unwrap_or("");
        let muted = cx.theme().muted_foreground;
        div()
            .w_full()
            .py_2()
            .text_color(muted)
            .child(
                div()
                    .mx_auto()
                    .child(math_flow(source, 17., true, cx)),
            )
    }
}

#[derive(Clone)]
struct MathSource(String);

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Piece {
    Text(String),
    Sup(Vec<Piece>),
    Sub(Vec<Piece>),
    Frac(Vec<Piece>, Vec<Piece>),
    Sqrt(Vec<Piece>),
    /// `\bar{x}` / `\overline{...}`：画一条上划线。
    Over(Vec<Piece>),
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    /// 解析到 `stop` 为止（不消费 stop）。
    fn sequence(&mut self, stop: Option<char>) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut text = String::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == stop {
                break;
            }
            match ch {
                '\\' => {
                    self.bump();
                    flush(&mut text, &mut pieces);
                    if let Some(piece) = self.command() {
                        pieces.push(piece);
                    }
                }
                '^' | '_' => {
                    self.bump();
                    flush(&mut text, &mut pieces);
                    let arg = self.argument();
                    if ch == '^' {
                        pieces.push(Piece::Sup(arg));
                    } else {
                        pieces.push(Piece::Sub(arg));
                    }
                }
                '{' => {
                    self.bump();
                    let inner = self.sequence(Some('}'));
                    self.expect('}');
                    flush(&mut text, &mut pieces);
                    pieces.extend(inner);
                }
                '}' => break,
                _ => {
                    self.bump();
                    text.push(ch);
                }
            }
        }
        flush(&mut text, &mut pieces);
        pieces
    }

    /// 命令后的一整个参数：`{...}` 或单个字符。
    fn argument(&mut self) -> Vec<Piece> {
        while matches!(self.peek(), Some(' ' | '\n' | '\t')) {
            self.bump();
        }
        match self.peek() {
            Some('{') => {
                self.bump();
                let inner = self.sequence(Some('}'));
                self.expect('}');
                inner
            }
            Some(_) => {
                let ch = self.bump().unwrap();
                vec![Piece::Text(ch.to_string())]
            }
            None => Vec::new(),
        }
    }

    fn expect(&mut self, ch: char) {
        if self.peek() == Some(ch) {
            self.bump();
        }
    }

    fn command(&mut self) -> Option<Piece> {
        let mut name = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphabetic() {
                name.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        if name.is_empty() {
            // 单字符命令：`\,` `\;` `\!` `\{` `\}` …
            return match self.bump()? {
                ',' | ';' | ':' => Some(Piece::Text(" ".into())),
                '!' => None,
                ' ' => Some(Piece::Text(" ".into())),
                other => Some(Piece::Text(other.to_string())),
            };
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let numer = self.argument();
                let denom = self.argument();
                Some(Piece::Frac(numer, denom))
            }
            "sqrt" => Some(Piece::Sqrt(self.argument())),
            "bar" | "overline" => Some(Piece::Over(self.argument())),
            "hat" | "widehat" | "tilde" | "widetilde" | "vec" => {
                // 帽子/波浪/向量没有合适的排版，用组合字符叠在字符上。
                let mark = match name.as_str() {
                    "hat" | "widehat" => '\u{0302}',
                    "tilde" | "widetilde" => '\u{0303}',
                    _ => '\u{20D7}',
                };
                Some(Piece::Text(format!("{}{mark}", plain_text(&self.argument()))))
            }
            "text" | "mathrm" | "operatorname" | "mathbf" | "mathit" => {
                Some(Piece::Text(plain_text(&self.argument())))
            }
            "left" | "right" => {
                // 只保留定界符本身，`\left(` / `\right)` 里的括号照常显示。
                Some(Piece::Text(self.bump().map(String::from).unwrap_or_default()))
            }
            "quad" => Some(Piece::Text("\u{2003}".into())),
            "qquad" => Some(Piece::Text("\u{2003}\u{2003}".into())),
            other => Some(Piece::Text(symbol(other).unwrap_or(other).to_string())),
        }
    }
}

fn flush(text: &mut String, pieces: &mut Vec<Piece>) {
    if !text.is_empty() {
        pieces.push(Piece::Text(std::mem::take(text)));
    }
}

fn plain_text(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.clone(),
            Piece::Sup(inner) | Piece::Sub(inner) => plain_text(inner),
            Piece::Sqrt(inner) => format!("√{}", plain_text(inner)),
            Piece::Over(inner) => plain_text(inner),
            Piece::Frac(a, b) => format!("{}/{}", plain_text(a), plain_text(b)),
        })
        .collect()
}

/// 常见 TeX 命令 → Unicode。见模块说明：这是可读近似，不是完整 TeX。
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ",
        "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ",
        "mu" => "μ", "nu" => "ν", "xi" => "ξ", "pi" => "π", "rho" => "ρ",
        "sigma" => "σ", "tau" => "τ", "upsilon" => "υ", "phi" => "φ",
        "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ",
        "Pi" => "Π", "Sigma" => "Σ", "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "pm" => "±", "mp" => "∓", "times" => "×", "div" => "÷", "cdot" => "·",
        "le" | "leq" => "≤", "ge" | "geq" => "≥", "ne" | "neq" => "≠",
        "approx" => "≈", "equiv" => "≡", "propto" => "∝", "sim" => "∼",
        "infty" => "∞", "partial" => "∂", "nabla" => "∇",
        "sum" => "∑", "prod" => "∏", "int" => "∫",
        "angle" => "∠", "degree" => "°", "circ" => "∘",
        "to" | "rightarrow" => "→", "leftarrow" => "←", "Rightarrow" => "⇒",
        "leftrightarrow" => "↔", "cdots" => "⋯", "ldots" | "dots" => "…",
        "in" => "∈", "notin" => "∉", "subset" => "⊂", "subseteq" => "⊆",
        "cup" => "∪", "cap" => "∩", "forall" => "∀", "exists" => "∃",
        "therefore" => "∴", "because" => "∵", "emptyset" => "∅",
        "star" => "⋆", "prime" => "′", "lvert" | "rvert" => "|", "Vert" => "‖",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

fn math_flow(source: &str, size: f32, display: bool, cx: &App) -> AnyElement {
    let pieces = Parser::new(source).sequence(None);
    render_pieces(&pieces, size, display, cx.theme().border)
}

fn render_pieces(
    pieces: &[Piece],
    size: f32,
    display: bool,
    border: gpui_kit::Hsla,
) -> AnyElement {
    let mut row = h_flex().items_center().gap(px(1.));
    for piece in pieces {
        row = row.child(render_piece(piece, size, display, border));
    }
    row.into_any_element()
}

fn render_piece(
    piece: &Piece,
    size: f32,
    display: bool,
    border: gpui_kit::Hsla,
) -> AnyElement {
    match piece {
        Piece::Text(text) => div()
            .text_size(px(size))
            .child(text.clone())
            .into_any_element(),
        Piece::Sup(inner) => div()
            .relative()
            .top(px(-size * 0.34))
            .text_size(px(size * 0.68))
            .child(render_pieces(inner, size * 0.68, display, border))
            .into_any_element(),
        Piece::Sub(inner) => div()
            .relative()
            .top(px(size * 0.14))
            .text_size(px(size * 0.68))
            .child(render_pieces(inner, size * 0.68, display, border))
            .into_any_element(),
        Piece::Frac(numer, denom) => {
            let width = (estimate_width(&plain_text(numer), size) * 0.85)
                .max(estimate_width(&plain_text(denom), size) * 0.85)
                .max(14.)
                + 6.;
            gpui_kit::component::v_flex()
                .items_center()
                .gap_0()
                .child(render_pieces(numer, size * 0.82, display, border))
                .child(div().w(px(width)).h(px(1.)).bg(border))
                .child(render_pieces(denom, size * 0.82, display, border))
                .into_any_element()
        }
        Piece::Sqrt(inner) => h_flex()
            .items_start()
            .child(div().text_size(px(size * 1.1)).child("√"))
            .child(
                div()
                    .border_t_1()
                    .border_color(border)
                    .px(px(2.))
                    .child(render_pieces(inner, size, display, border)),
            )
            .into_any_element(),
        Piece::Over(inner) => div()
            .border_t_1()
            .border_color(border)
            .px(px(2.))
            .child(render_pieces(inner, size, display, border))
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(source: &str) -> String {
        plain_text(&Parser::new(source).sequence(None))
    }

    #[test]
    fn converts_common_commands() {
        assert_eq!(text(r"\alpha \le \beta \pm 1"), "α ≤ β ± 1");
        assert_eq!(text(r"x \times y \approx z"), "x × y ≈ z");
        assert_eq!(text(r"\mu \cdot \Delta"), "μ · Δ");
    }

    #[test]
    fn renders_fractions_and_roots() {
        assert_eq!(text(r"\frac{a+b}{2}"), "a+b/2");
        assert_eq!(text(r"\sqrt{x}"), "√x");
        // 嵌套：分式里再套分式不能丢字符。
        assert_eq!(text(r"\frac{\frac{1}{2}}{3}"), "1/2/3");
    }

    #[test]
    fn keeps_superscripts_and_subscripts() {
        assert_eq!(text("x_i^2"), "xi2");
        assert_eq!(text(r"\sum_{i=1}^{n} x_i"), "∑i=1n xi");
        let pieces = Parser::new("x^2").sequence(None);
        assert!(matches!(pieces.last(), Some(Piece::Sup(_))));
    }

    #[test]
    fn delimiter_commands_do_not_leak_names() {
        // `\left` / `\right` 只保留定界符，命令名本身不能漏出来。
        assert_eq!(text(r"\left(a\right)"), "(a)");
    }
}
