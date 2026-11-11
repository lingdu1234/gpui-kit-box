//! Markdown 插件：公式（`$…$` / `$$…$$`）渲染。
//!
//! `gpui-base` 的解析器已经打开了 `math_text` / `math_flow` 两个 construct，
//! 所以 mdast 会直接给出 `InlineMath` / `Math` 节点，这里只管排版。
//!
//! 排版不是完整的 TeX 引擎，而是一个**可读的近似实现**：把常见的命令换成
//! Unicode 符号，`^` / `_` 排成可堆叠的上下标，`\frac` 排成真正的上下分式，
//! `\sqrt[n]{x}` 排成带根指数的根号，`\begin{matrix}…\end{matrix}` 等环境排成
//! 网格并配上对应的定界符。积分上下限、复杂矩阵对齐等会退化成可读的文字，
//! 不会崩。独立公式块（`$$…$$`）外面套一层等比适配包装，宿主宽度不足时整体
//! 同比例缩小，不变形。

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
        let pieces = Parser::new(source).sequence(None);
        let (w, h) = measure(&pieces, 17.);
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        // 宿主不告知可用宽高：套一层等比适配包装，由它在布局期测量父容器空间，
        // 不足时把整块公式（含文字）同比例缩小。包装自带水平居中。
        div()
            .w_full()
            .py_2()
            .child(crate::fit::FitDiagram::new(w + 8., h + 8., move |scale, _window, _cx| {
                div()
                    .text_color(muted)
                    .child(render_pieces(&pieces, 17. * scale, true, border))
                    .into_any_element()
            }))
    }
}

#[derive(Clone)]
struct MathSource(String);

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 环境（矩阵 / cases / aligned …）的左右定界符。空串表示不画那一侧。
#[derive(Clone, Debug)]
struct Delim {
    left: &'static str,
    right: &'static str,
}

#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    /// 上下标：`x_i^2` 把 sub / sup 挂在同一个底上。单独的 `^` / `_` 也归到这里。
    Script {
        base: Vec<Piece>,
        sub: Option<Vec<Piece>>,
        sup: Option<Vec<Piece>>,
    },
    Frac(Vec<Piece>, Vec<Piece>),
    /// `\sqrt[n]{x}`：`index` 为 `None` 时是普通平方根。
    Root {
        index: Option<Vec<Piece>>,
        radicand: Vec<Piece>,
    },
    /// `\bar{x}` / `\overline{...}`：画一条上划线。
    Over(Vec<Piece>),
    /// `\underline{...}`：画一条下划线。
    Under(Vec<Piece>),
    /// 矩阵 / 环境：`rows[row][col]` 是单元格内容，`delim` 决定左右定界符。
    Matrix {
        rows: Vec<Vec<Vec<Piece>>>,
        delim: Delim,
    },
    /// `\binom{n}{k}`：带圆括号、无横线的上下堆叠。
    Binom(Vec<Piece>, Vec<Piece>),
    /// `\overset{a}{b}` / `\underset{a}{b}`：在底的上方 / 下方堆一行小字。
    Stack {
        top: Option<Vec<Piece>>,
        bottom: Option<Vec<Piece>>,
        base: Vec<Piece>,
    },
    /// `\boxed{x}`：画一个方框。
    Boxed(Vec<Piece>),
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    /// 处于环境内部时为真：`&` 分列、`\\` 分行、`\end` 收尾都要在此停下。
    in_env: bool,
}

impl Parser {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            in_env: false,
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

    /// 从当前 `\` 往后看，命令名是否恰好是 `name`（后面不接字母）。
    fn cmd_ahead(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let mut i = self.pos + 1;
        for c in name.chars() {
            if self.chars.get(i) != Some(&c) {
                return false;
            }
            i += 1;
        }
        !self.chars.get(i).is_some_and(|c| c.is_ascii_alphabetic())
    }

    /// 环境内部的单元格边界：`&`、`\\`（分行）或 `\end`。
    fn at_env_break(&self) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        self.chars.get(self.pos + 1) == Some(&'\\') || self.cmd_ahead("end")
    }

    /// 解析到 `stop` 为止（不消费 stop）。
    fn sequence(&mut self, stop: Option<char>) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut text = String::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == stop {
                break;
            }
            if self.in_env && (ch == '&' || self.at_env_break()) {
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
                    attach_script(&mut pieces, arg, ch == '^');
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
        self.skip_spaces();
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

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\t')) {
            self.bump();
        }
    }

    fn expect(&mut self, ch: char) {
        if self.peek() == Some(ch) {
            self.bump();
        }
    }

    /// 读一个 `{name}` 组并返回其中的原始文字（用于环境名）。
    fn read_group_name(&mut self) -> String {
        self.skip_spaces();
        let mut name = String::new();
        if self.peek() == Some('{') {
            self.bump();
            while let Some(ch) = self.peek() {
                if ch == '}' {
                    self.bump();
                    break;
                }
                name.push(ch);
                self.bump();
            }
        }
        name
    }

    /// 解析一个环境体：`&` 分列、`\\` 分行，直到 `\end{name}`。
    fn environment(&mut self, name: &str) -> Piece {
        let delim = match name {
            "pmatrix" => Delim { left: "(", right: ")" },
            "bmatrix" => Delim { left: "[", right: "]" },
            "Bmatrix" => Delim { left: "{", right: "}" },
            "vmatrix" => Delim { left: "|", right: "|" },
            "Vmatrix" => Delim { left: "‖", right: "‖" },
            "cases" => Delim { left: "{", right: "" },
            // matrix / aligned / align / array / gathered … 都不带定界符。
            _ => Delim { left: "", right: "" },
        };
        let prev = self.in_env;
        self.in_env = true;
        let mut rows: Vec<Vec<Vec<Piece>>> = Vec::new();
        let mut row: Vec<Vec<Piece>> = Vec::new();
        loop {
            let cell = self.sequence(None);
            row.push(cell);
            match self.peek() {
                Some('&') => {
                    self.bump();
                }
                Some('\\') if self.chars.get(self.pos + 1) == Some(&'\\') => {
                    self.bump();
                    self.bump();
                    rows.push(std::mem::take(&mut row));
                }
                _ => {
                    rows.push(std::mem::take(&mut row));
                    break;
                }
            }
        }
        self.in_env = prev;
        // 消费收尾的 `\end{name}`（缺失时也不 panic，只是少画一个定界环境）。
        if self.cmd_ahead("end") {
            self.bump();
            for _ in 0..3 {
                self.bump();
            }
            let _ = self.read_group_name();
        }
        // 末尾多出一个空行（最后一行 `\\` 后没有内容）时去掉它。
        if rows.len() > 1
            && rows
                .last()
                .is_some_and(|r| r.iter().all(|c| c.is_empty()))
        {
            rows.pop();
        }
        Piece::Matrix { rows, delim }
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
            "binom" | "dbinom" | "tbinom" => {
                let top = self.argument();
                let bottom = self.argument();
                Some(Piece::Binom(top, bottom))
            }
            "sqrt" => {
                self.skip_spaces();
                let index = if self.peek() == Some('[') {
                    self.bump();
                    let inner = self.sequence(Some(']'));
                    self.expect(']');
                    Some(inner)
                } else {
                    None
                };
                let radicand = self.argument();
                Some(Piece::Root { index, radicand })
            }
            "bar" | "overline" | "overrightarrow" | "overleftarrow" => {
                Some(Piece::Over(self.argument()))
            }
            "underline" | "underbar" => Some(Piece::Under(self.argument())),
            "boxed" | "fbox" => Some(Piece::Boxed(self.argument())),
            // 没有合适排版的命令：退化成内容本身，至少不丢字符、不漏命令名。
            "cancel" => Some(Piece::Text(plain_text(&self.argument()))),
            "hat" | "widehat" | "tilde" | "widetilde" | "vec" | "dot" | "ddot" => {
                // 帽子/波浪/向量/点没有合适的排版，用组合字符叠在字符上。
                let mark = match name.as_str() {
                    "hat" | "widehat" => '\u{0302}',
                    "tilde" | "widetilde" => '\u{0303}',
                    "dot" => '\u{0307}',
                    "ddot" => '\u{0308}',
                    _ => '\u{20D7}',
                };
                Some(Piece::Text(format!("{}{mark}", plain_text(&self.argument()))))
            }
            "overset" | "stackrel" => {
                let top = self.argument();
                let base = self.argument();
                Some(Piece::Stack { top: Some(top), bottom: None, base })
            }
            "underset" => {
                let bottom = self.argument();
                let base = self.argument();
                Some(Piece::Stack { top: None, bottom: Some(bottom), base })
            }
            "text" | "mathrm" | "operatorname" | "mathbf" | "mathit" | "mathsf" | "mathtt" => {
                Some(Piece::Text(plain_text(&self.argument())))
            }
            "mathbb" | "mathcal" | "mathfrak" => {
                let raw = plain_text(&self.argument());
                let mapped: String = raw
                    .chars()
                    .map(|c| match name.as_str() {
                        "mathbb" => blackboard(c),
                        "mathcal" => script_font(c),
                        _ => fraktur(c),
                    })
                    .collect();
                Some(Piece::Text(mapped))
            }
            "begin" => {
                let env = self.read_group_name();
                Some(self.environment(&env))
            }
            "end" => {
                // 游离的 `\end`（没有配对的 `\begin`）：消费掉组名，不显示。
                let _ = self.read_group_name();
                None
            }
            "tag" => Some(Piece::Text(format!("({})", plain_text(&self.argument())))),
            "left" | "right" => {
                // 定界符：`\left.` / `\right.` 是空定界符（不显示）；`\left\{`
                // 这种带反斜杠的定界符要连反斜杠一起消费，命名定界符查符号表。
                let text = match self.bump() {
                    Some('.') => String::new(),
                    Some('\\') => {
                        let mut nm = String::new();
                        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                            nm.push(self.bump().unwrap());
                        }
                        if nm.is_empty() {
                            self.bump().map(|c| c.to_string()).unwrap_or_default()
                        } else {
                            symbol(&nm).unwrap_or("").to_string()
                        }
                    }
                    Some(c) => c.to_string(),
                    None => String::new(),
                };
                Some(Piece::Text(text))
            }
            "quad" => Some(Piece::Text("\u{2003}".into())),
            "qquad" => Some(Piece::Text("\u{2003}\u{2003}".into())),
            "enspace" | "thinspace" => Some(Piece::Text(" ".into())),
            // 纯样式 / 极限位置开关：对这套近似排版没有意义，直接忽略。
            "displaystyle" | "textstyle" | "scriptstyle" | "limits" | "nolimits" | "not" => None,
            other => Some(Piece::Text(symbol(other).unwrap_or(other).to_string())),
        }
    }
}

/// 把上下标挂到最后一个 piece 上：若它已是 `Script`，合并到同一个底，
/// 这样 `x_i^2` 的 sub / sup 会堆在一起而不是各占一个独立位置。
fn attach_script(pieces: &mut Vec<Piece>, arg: Vec<Piece>, is_sup: bool) {
    let (base, mut sub, mut sup) = match pieces.pop() {
        Some(Piece::Script { base, sub, sup }) => (base, sub, sup),
        Some(other) => (vec![other], None, None),
        None => (Vec::new(), None, None),
    };
    if is_sup {
        sup = Some(arg);
    } else {
        sub = Some(arg);
    }
    pieces.push(Piece::Script { base, sub, sup });
}

fn flush(text: &mut String, pieces: &mut Vec<Piece>) {
    if !text.is_empty() {
        pieces.push(Piece::Text(std::mem::take(text)));
    }
}

fn plain_text(pieces: &[Piece]) -> String {
    pieces.iter().map(piece_text).collect()
}

fn piece_text(piece: &Piece) -> String {
    match piece {
        Piece::Text(text) => text.clone(),
        Piece::Script { base, sub, sup } => format!(
            "{}{}{}",
            plain_text(base),
            sub.as_deref().map(plain_text).unwrap_or_default(),
            sup.as_deref().map(plain_text).unwrap_or_default(),
        ),
        Piece::Root { index, radicand } => match index {
            Some(idx) => format!("{}√{}", plain_text(idx), plain_text(radicand)),
            None => format!("√{}", plain_text(radicand)),
        },
        Piece::Over(inner) | Piece::Under(inner) | Piece::Boxed(inner) => plain_text(inner),
        Piece::Frac(a, b) | Piece::Binom(a, b) => format!("{}/{}", plain_text(a), plain_text(b)),
        Piece::Stack { top, bottom, base } => format!(
            "{}{}{}",
            top.as_deref().map(plain_text).unwrap_or_default(),
            plain_text(base),
            bottom.as_deref().map(plain_text).unwrap_or_default(),
        ),
        Piece::Matrix { rows, .. } => rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| plain_text(cell))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// 常见 TeX 命令 → Unicode。见模块说明：这是可读近似，不是完整 TeX。
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ",
        "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ",
        "mu" => "μ", "nu" => "ν", "xi" => "ξ", "pi" => "π", "varpi" => "ϖ", "rho" => "ρ",
        "varrho" => "ϱ", "sigma" => "σ", "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ",
        "phi" => "φ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ",
        "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ", "Phi" => "Φ",
        "Psi" => "Ψ", "Omega" => "Ω",
        "pm" => "±", "mp" => "∓", "times" => "×", "div" => "÷", "cdot" => "·",
        "ast" => "∗", "star" => "⋆", "bullet" => "•", "circ" => "∘",
        "le" | "leq" => "≤", "ge" | "geq" => "≥", "ne" | "neq" => "≠",
        "ll" => "≪", "gg" => "≫", "approx" => "≈", "equiv" => "≡", "cong" => "≅",
        "simeq" => "≃", "sim" => "∼", "propto" => "∝", "prec" => "≺", "succ" => "≻",
        "infty" => "∞", "partial" => "∂", "nabla" => "∇",
        "sum" => "∑", "prod" => "∏", "int" => "∫", "iint" => "∬", "oint" => "∮",
        "bigcup" => "⋃", "bigcap" => "⋂",
        "angle" => "∠", "degree" => "°", "triangle" => "△",
        "to" | "rightarrow" => "→", "leftarrow" => "←", "Rightarrow" => "⇒",
        "Leftarrow" => "⇐", "leftrightarrow" => "↔", "Leftrightarrow" => "⇔",
        "mapsto" => "↦", "iff" => "⟺", "implies" => "⟹", "impliedby" => "⟸",
        "cdots" => "⋯", "ldots" | "dots" => "…", "vdots" => "⋮", "ddots" => "⋱",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "subseteq" => "⊆",
        "supset" => "⊃", "supseteq" => "⊇", "cup" => "∪", "cap" => "∩",
        "setminus" => "∖", "emptyset" | "varnothing" => "∅",
        "forall" => "∀", "exists" => "∃", "nexists" => "∄",
        "land" | "wedge" => "∧", "lor" | "vee" => "∨", "neg" | "lnot" => "¬",
        "oplus" => "⊕", "otimes" => "⊗", "odot" => "⊙",
        "therefore" => "∴", "because" => "∵",
        "perp" => "⊥", "parallel" => "∥", "mid" => "∣", "nmid" => "∤",
        "vdash" => "⊢", "dashv" => "⊣", "models" => "⊨", "top" => "⊤", "bot" => "⊥",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋",
        "lceil" => "⌈", "rceil" => "⌉", "lbrace" => "{", "rbrace" => "}",
        "lvert" | "rvert" | "vert" => "|", "lVert" | "rVert" | "Vert" => "‖",
        "prime" => "′", "hbar" => "ℏ", "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ",
        "aleph" => "ℵ", "wp" => "℘", "checkmark" => "✓", "dagger" => "†",
        "sharp" => "♯", "flat" => "♭", "natural" => "♮",
        _ => return None,
    })
}

/// `\mathbb` 黑板粗体：常见字母有专用码位，其余落在 U+1D538 起的连续区。
fn blackboard(ch: char) -> char {
    match ch {
        'C' => 'ℂ', 'H' => 'ℍ', 'N' => 'ℕ', 'P' => 'ℙ', 'Q' => 'ℚ', 'R' => 'ℝ', 'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

/// `\mathcal` 花体：常见字母有专用码位，其余落在 U+1D49C 起的连续区。
fn script_font(ch: char) -> char {
    match ch {
        'B' => 'ℬ', 'E' => 'ℰ', 'F' => 'ℱ', 'H' => 'ℋ', 'I' => 'ℐ', 'L' => 'ℒ',
        'M' => 'ℳ', 'R' => 'ℛ',
        'A'..='Z' => char::from_u32(0x1D49C + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        'a'..='z' => char::from_u32(0x1D4B6 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

/// `\mathfrak` 哥特体：常见字母有专用码位，其余落在 U+1D504 起的连续区。
fn fraktur(ch: char) -> char {
    match ch {
        'C' => 'ℭ', 'H' => 'ℌ', 'I' => 'ℑ', 'R' => 'ℜ', 'Z' => 'ℨ',
        'A'..='Z' => char::from_u32(0x1D504 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        'a'..='z' => char::from_u32(0x1D51E + (ch as u32 - 'a' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

// ---------------------------------------------------------------------------
// 自然尺寸估算（供独立公式块的等比适配包装用）
// ---------------------------------------------------------------------------

/// 一行 pieces 横排的自然尺寸：宽相加（含 gap），高取最大。只是估算，
/// 略偏宽裕——宁可早一点缩小，也不要低估导致溢出。
fn measure(pieces: &[Piece], size: f32) -> (f32, f32) {
    let mut w = 0.;
    let mut h = size * 1.2;
    for (i, piece) in pieces.iter().enumerate() {
        let (pw, ph) = measure_piece(piece, size);
        if i > 0 {
            w += 1.;
        }
        w += pw;
        h = h.max(ph);
    }
    (w, h)
}

fn measure_piece(piece: &Piece, size: f32) -> (f32, f32) {
    match piece {
        Piece::Text(text) => (estimate_width(text, size), size * 1.2),
        Piece::Script { base, sub, sup } => {
            let (bw, bh) = measure(base, size);
            let ss = size * 0.68;
            let sw = sub
                .as_deref()
                .map(|s| measure(s, ss).0)
                .unwrap_or(0.)
                .max(sup.as_deref().map(|s| measure(s, ss).0).unwrap_or(0.));
            let extra = if sub.is_some() || sup.is_some() { ss * 1.1 } else { 0. };
            (bw + sw + 1., bh.max(size * 1.2) + extra)
        }
        Piece::Frac(a, b) => {
            let fs = size * 0.82;
            let (aw, ah) = measure(a, fs);
            let (bw, bh) = measure(b, fs);
            (aw.max(bw) + 6., ah + bh + 2.)
        }
        Piece::Root { index, radicand } => {
            let (rw, rh) = measure(radicand, size);
            let iw = index
                .as_deref()
                .map(|s| measure(s, size * 0.6).0)
                .unwrap_or(0.);
            (iw + size * 0.7 + rw + 4., rh.max(size * 1.3))
        }
        Piece::Over(inner) | Piece::Under(inner) => {
            let (w, h) = measure(inner, size);
            (w + 4., h + 2.)
        }
        Piece::Boxed(inner) => {
            let (w, h) = measure(inner, size);
            (w + 8., h + 6.)
        }
        Piece::Binom(a, b) => {
            let fs = size * 0.82;
            let (aw, ah) = measure(a, fs);
            let (bw, bh) = measure(b, fs);
            (aw.max(bw) + 6. + size * 0.8, ah + bh + 2.)
        }
        Piece::Stack { top, bottom, base } => {
            let (bw, bh) = measure(base, size);
            let ss = size * 0.7;
            let (tw, th) = top.as_deref().map(|s| measure(s, ss)).unwrap_or((0., 0.));
            let (uw, uh) = bottom.as_deref().map(|s| measure(s, ss)).unwrap_or((0., 0.));
            (bw.max(tw).max(uw), bh + th + uh)
        }
        Piece::Matrix { rows, delim } => {
            let cs = size * 0.9;
            let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            let mut col_w = vec![0f32; cols];
            let mut row_h = Vec::new();
            for row in rows {
                let mut rh = cs * 1.2;
                for (c, cell) in row.iter().enumerate() {
                    let (cw, ch) = measure(cell, cs);
                    col_w[c] = col_w[c].max(cw);
                    rh = rh.max(ch);
                }
                row_h.push(rh);
            }
            let grid_w: f32 = col_w.iter().sum::<f32>() + cols.saturating_sub(1) as f32 * 8.;
            let grid_h: f32 = row_h.iter().sum::<f32>() + rows.len().saturating_sub(1) as f32 * 2.;
            let dw = if delim.left.is_empty() { 0. } else { size * 0.5 }
                + if delim.right.is_empty() { 0. } else { size * 0.5 };
            (grid_w + dw + 4., grid_h.max(size * 1.2))
        }
    }
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
        Piece::Script { base, sub, sup } => {
            let ss = size * 0.68;
            let mut col = gpui_kit::component::v_flex().items_start().gap_0();
            if let Some(sup) = sup {
                col = col.child(render_pieces(sup, ss, display, border));
            }
            if let Some(sub) = sub {
                col = col.child(render_pieces(sub, ss, display, border));
            }
            // 只有上标时上移、只有下标时下移、两者都有时大致居中于底。
            let shift = match (sup.is_some(), sub.is_some()) {
                (true, false) => -size * 0.34,
                (false, true) => size * 0.14,
                _ => -size * 0.1,
            };
            h_flex()
                .items_center()
                .child(render_pieces(base, size, display, border))
                .child(div().relative().top(px(shift)).child(col))
                .into_any_element()
        }
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
        Piece::Root { index, radicand } => {
            let mut row = h_flex().items_start();
            if let Some(index) = index {
                row = row.child(
                    div()
                        .relative()
                        .top(px(-size * 0.3))
                        .text_size(px(size * 0.6))
                        .child(render_pieces(index, size * 0.6, display, border)),
                );
            }
            row = row
                .child(div().text_size(px(size * 1.1)).child("√"))
                .child(
                    div()
                        .border_t_1()
                        .border_color(border)
                        .px(px(2.))
                        .child(render_pieces(radicand, size, display, border)),
                );
            row.into_any_element()
        }
        Piece::Over(inner) => div()
            .border_t_1()
            .border_color(border)
            .px(px(2.))
            .child(render_pieces(inner, size, display, border))
            .into_any_element(),
        Piece::Under(inner) => div()
            .border_b_1()
            .border_color(border)
            .px(px(2.))
            .child(render_pieces(inner, size, display, border))
            .into_any_element(),
        Piece::Boxed(inner) => div()
            .border_1()
            .border_color(border)
            .px(px(3.))
            .child(render_pieces(inner, size, display, border))
            .into_any_element(),
        Piece::Binom(top, bottom) => {
            let width = (estimate_width(&plain_text(top), size) * 0.85)
                .max(estimate_width(&plain_text(bottom), size) * 0.85)
                .max(14.)
                + 6.;
            h_flex()
                .items_center()
                .child(div().text_size(px(size * 1.2)).child("("))
                .child(
                    gpui_kit::component::v_flex()
                        .items_center()
                        .gap_0()
                        .w(px(width))
                        .child(render_pieces(top, size * 0.82, display, border))
                        .child(render_pieces(bottom, size * 0.82, display, border)),
                )
                .child(div().text_size(px(size * 1.2)).child(")"))
                .into_any_element()
        }
        Piece::Stack { top, bottom, base } => {
            let mut col = gpui_kit::component::v_flex().items_center().gap_0();
            if let Some(top) = top {
                col = col.child(render_pieces(top, size * 0.7, display, border));
            }
            col = col.child(render_pieces(base, size, display, border));
            if let Some(bottom) = bottom {
                col = col.child(render_pieces(bottom, size * 0.7, display, border));
            }
            col.into_any_element()
        }
        Piece::Matrix { rows, delim } => {
            let mut grid = gpui_kit::component::v_flex().items_center().gap(px(2.));
            for row in rows {
                let mut r = h_flex().items_center().gap(px(8.));
                for cell in row {
                    r = r.child(render_pieces(cell, size * 0.9, display, border));
                }
                grid = grid.child(r);
            }
            let mut wrap = h_flex().items_center();
            if !delim.left.is_empty() {
                wrap = wrap.child(div().text_size(px(size * 1.2)).child(delim.left));
            }
            wrap = wrap.child(grid);
            if !delim.right.is_empty() {
                wrap = wrap.child(div().text_size(px(size * 1.2)).child(delim.right));
            }
            wrap.into_any_element()
        }
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
        // `x_i^2` 的上下标应堆在同一个底上，而不是两个独立位置。
        let pieces = Parser::new("x_i^2").sequence(None);
        assert!(matches!(
            pieces.last(),
            Some(Piece::Script { sub: Some(_), sup: Some(_), .. })
        ));
    }

    #[test]
    fn delimiter_commands_do_not_leak_names() {
        // `\left` / `\right` 只保留定界符，命令名本身不能漏出来。
        assert_eq!(text(r"\left(a\right)"), "(a)");
        // `\left.` 是空定界符；`\left\{` 要连反斜杠一起消费。
        assert_eq!(text(r"\left.x\right|"), "x|");
        assert_eq!(text(r"\left\{x\right\}"), "{x}");
        // `\langle` 后的空格是命令终止符，按近似排版保留为文字。
        assert_eq!(text(r"\left\langle x\right\rangle"), "⟨ x⟩");
    }

    #[test]
    fn root_index_is_kept() {
        assert_eq!(text(r"\sqrt[3]{x}"), "3√x");
        let pieces = Parser::new(r"\sqrt[3]{x}").sequence(None);
        assert!(matches!(
            pieces.last(),
            Some(Piece::Root { index: Some(_), .. })
        ));
        // 不带指数仍是普通平方根。
        assert!(matches!(
            Parser::new(r"\sqrt{x}").sequence(None).last(),
            Some(Piece::Root { index: None, .. })
        ));
    }

    #[test]
    fn environments_parse_into_matrices() {
        assert_eq!(text(r"\begin{matrix}a&b\\c&d\end{matrix}"), "a b; c d");
        let pieces = Parser::new(r"\begin{pmatrix}1&2\\3&4\end{pmatrix}").sequence(None);
        match pieces.last() {
            Some(Piece::Matrix { rows, delim }) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(delim.left, "(");
                assert_eq!(delim.right, ")");
            }
            other => panic!("应是矩阵，实际 {other:?}"),
        }
        // cases 只有左花括号；末尾多余的 `\\` 不产生空行。
        let cases = Parser::new(r"\begin{cases}a&x>0\\b&x\le 0\\\end{cases}").sequence(None);
        match cases.last() {
            Some(Piece::Matrix { rows, delim }) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(delim.left, "{");
                assert_eq!(delim.right, "");
            }
            other => panic!("应是 cases，实际 {other:?}"),
        }
    }

    #[test]
    fn blackboard_script_and_fraktur_fonts() {
        assert_eq!(text(r"\mathbb{R}"), "ℝ");
        assert_eq!(text(r"\mathbb{N}"), "ℕ");
        assert_eq!(text(r"\mathcal{L}"), "ℒ");
        assert_eq!(text(r"\mathfrak{g}"), "𝔤");
        assert_eq!(text(r"\mathfrak{A}"), "𝔄");
    }

    #[test]
    fn expanded_symbols() {
        assert_eq!(text(r"\langle x \rangle"), "⟨ x ⟩");
        assert_eq!(text(r"a \land b \lor \neg c"), "a ∧ b ∨ ¬ c");
        assert_eq!(text(r"x \mapsto y \iff z"), "x ↦ y ⟺ z");
        assert_eq!(text(r"\lfloor x \rfloor"), "⌊ x ⌋");
        assert_eq!(text(r"A \oplus B"), "A ⊕ B");
    }

    #[test]
    fn binom_underline_and_boxed() {
        assert_eq!(text(r"\binom{n}{k}"), "n/k");
        assert_eq!(text(r"\underline{x}"), "x");
        assert_eq!(text(r"\boxed{E=mc^2}"), "E=mc2");
        assert_eq!(text(r"\overset{a}{b}"), "ab");
        assert_eq!(text(r"\underset{a}{b}"), "ba");
    }

    /// 展示页里的公式都必须能解析且自然尺寸为正——等比适配包装据此缩放。
    #[test]
    fn showcase_math_measures_positive() {
        let page = include_str!("../showcase/math.md");
        let mut count = 0;
        for line in page.lines() {
            let trimmed = line.trim();
            // 独立块 `$$…$$`。
            if let Some(inner) = trimmed.strip_prefix("$$").and_then(|s| s.strip_suffix("$$")) {
                let pieces = Parser::new(inner).sequence(None);
                let (w, h) = measure(&pieces, 17.);
                assert!(w > 0. && h > 0., "公式块尺寸为负：{inner}");
                count += 1;
            }
        }
        assert!(count >= 10, "展示页公式块数量异常：{count}");
    }

    /// 其余环境定界符：bmatrix/Bmatrix/vmatrix/Vmatrix 各配各的括号，
    /// matrix/aligned/gathered/align 不带定界符。
    #[test]
    fn environment_delimiters_cover_every_variant() {
        let delim = |src: &str| -> (&'static str, &'static str) {
            match Parser::new(src).sequence(None).last() {
                Some(Piece::Matrix { delim, .. }) => (delim.left, delim.right),
                other => panic!("应是矩阵，实际 {other:?}"),
            }
        };
        assert_eq!(delim(r"\begin{bmatrix}a\end{bmatrix}"), ("[", "]"));
        assert_eq!(delim(r"\begin{Bmatrix}a\end{Bmatrix}"), ("{", "}"));
        assert_eq!(delim(r"\begin{vmatrix}a\end{vmatrix}"), ("|", "|"));
        assert_eq!(delim(r"\begin{Vmatrix}a\end{Vmatrix}"), ("‖", "‖"));
        for env in ["matrix", "aligned", "gathered", "align"] {
            let src = format!(r"\begin{{{env}}}a\end{{{env}}}");
            assert_eq!(delim(&src), ("", ""), "{env} 不应带定界符");
        }
    }

    /// 嵌套环境：矩阵里套分式 / 根号，矩阵里再套矩阵，字符都不能丢。
    #[test]
    fn nested_environments_keep_all_content() {
        assert_eq!(
            text(r"\begin{pmatrix}\frac{a}{b}&1\\2&\sqrt{x}\end{pmatrix}"),
            "a/b 1; 2 √x"
        );
        let pieces =
            Parser::new(r"\begin{bmatrix}\begin{matrix}1&2\end{matrix}&3\end{bmatrix}")
                .sequence(None);
        match pieces.last() {
            Some(Piece::Matrix { rows, delim }) => {
                assert_eq!(delim.left, "[");
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].len(), 2);
                assert!(
                    matches!(rows[0][0].last(), Some(Piece::Matrix { .. })),
                    "内层矩阵应整体作为一个单元格"
                );
            }
            other => panic!("应是嵌套矩阵，实际 {other:?}"),
        }
    }

    /// aligned / gathered 按 `&` 分列、`\\` 分行。
    #[test]
    fn aligned_and_gathered_split_rows_and_columns() {
        assert_eq!(text(r"\begin{aligned}a&=b\\c&=d\end{aligned}"), "a =b; c =d");
        assert_eq!(text(r"\begin{gathered}a\\b\\c\end{gathered}"), "a; b; c");
        let pieces = Parser::new(r"\begin{aligned}x&=1\\y&=2\end{aligned}").sequence(None);
        match pieces.last() {
            Some(Piece::Matrix { rows, .. }) => {
                assert_eq!(rows.len(), 2);
                assert!(rows.iter().all(|r| r.len() == 2));
            }
            other => panic!("应是 aligned，实际 {other:?}"),
        }
    }

    /// 根指数可以是表达式，根号可以嵌套；指数与被开方数都不丢。
    #[test]
    fn root_index_accepts_expressions_and_nesting() {
        assert_eq!(text(r"\sqrt[n+1]{x}"), "n+1√x");
        assert_eq!(text(r"\sqrt[3]{\frac{a}{b}}"), "3√a/b");
        assert_eq!(text(r"\sqrt{\sqrt{x}}"), "√√x");
        let pieces = Parser::new(r"\sqrt[n+1]{x}").sequence(None);
        match pieces.last() {
            Some(Piece::Root { index: Some(idx), radicand }) => {
                assert_eq!(plain_text(idx), "n+1");
                assert_eq!(plain_text(radicand), "x");
            }
            other => panic!("应是带指数的根号，实际 {other:?}"),
        }
    }

    /// 上下标书写顺序无关：`x^2_i` 与 `x_i^2` 都堆在同一个底上。
    #[test]
    fn stacked_scripts_are_order_independent() {
        for src in ["x_i^2", "x^2_i"] {
            let pieces = Parser::new(src).sequence(None);
            assert_eq!(pieces.len(), 1, "{src} 应只有一个堆叠脚本");
            match &pieces[0] {
                Piece::Script { base, sub, sup } => {
                    assert_eq!(plain_text(base), "x");
                    assert_eq!(sub.as_deref().map(plain_text), Some("i".to_string()));
                    assert_eq!(sup.as_deref().map(plain_text), Some("2".to_string()));
                }
                other => panic!("{src} 应是 Script，实际 {other:?}"),
            }
        }
        // 多字符上下标用花括号括起来。
        assert_eq!(text(r"x^{2n}"), "x2n");
        assert_eq!(text(r"x_{i+1}"), "xi+1");
    }

    /// 字体映射覆盖小写与专用码位；非字母原样保留。
    #[test]
    fn font_maps_cover_lowercase_and_special_codepoints() {
        assert_eq!(text(r"\mathcal{abc}"), "𝒶𝒷𝒸");
        assert_eq!(text(r"\mathfrak{abc}"), "𝔞𝔟𝔠");
        assert_eq!(text(r"\mathbb{ABC}"), "𝔸𝔹ℂ");
        // 专用码位字母不走连续区。
        assert_eq!(text(r"\mathbb{RZ}"), "ℝℤ");
        assert_eq!(text(r"\mathcal{HL}"), "ℋℒ");
        assert_eq!(text(r"\mathfrak{RI}"), "ℜℑ");
        // 数字 / 符号不被字体映射改动。
        assert_eq!(text(r"\mathbb{123}"), "123");
    }

    /// 扩充符号表里尚未被其它测试覆盖的一批：集合 / 逻辑 / 箭头 / 杂项。
    #[test]
    fn expanded_symbols_cover_sets_logic_and_misc() {
        assert_eq!(text(r"A \subseteq B \supseteq C"), "A ⊆ B ⊇ C");
        assert_eq!(text(r"A \setminus B \cup C \cap D"), "A ∖ B ∪ C ∩ D");
        assert_eq!(text(r"\forall x \exists y \nexists z"), "∀ x ∃ y ∄ z");
        assert_eq!(text(r"a \oplus b \otimes c \odot d"), "a ⊕ b ⊗ c ⊙ d");
        assert_eq!(text(r"a \perp b \parallel c"), "a ⊥ b ∥ c");
        assert_eq!(text(r"\vdash \models \hbar \ell \aleph"), "⊢ ⊨ ℏ ℓ ℵ");
        assert_eq!(text(r"\checkmark \dagger"), "✓ †");
        assert_eq!(text(r"x \implies y \impliedby z"), "x ⟹ y ⟸ z");
    }

    /// tag / cancel / stackrel 与 over/under 的各种别名。
    #[test]
    fn tag_cancel_stackrel_and_over_under_aliases() {
        assert_eq!(text(r"\tag{1}"), "(1)");
        assert_eq!(text(r"\cancel{abc}"), "abc");
        assert_eq!(text(r"\stackrel{!}{=}"), "!=");
        assert_eq!(text(r"\overrightarrow{AB}"), "AB");
        assert_eq!(text(r"\overleftarrow{AB}"), "AB");
        assert_eq!(text(r"\underbar{x}"), "x");
        assert!(matches!(
            Parser::new(r"\overrightarrow{AB}").sequence(None).last(),
            Some(Piece::Over(_))
        ));
        assert!(matches!(
            Parser::new(r"\underbar{x}").sequence(None).last(),
            Some(Piece::Under(_))
        ));
    }

    /// 纯样式 / 极限位置开关被忽略，间距命令换成空格，命令名都不漏出来。
    #[test]
    fn style_switches_and_spacing_commands() {
        assert_eq!(text(r"\displaystyle\sum x"), "∑ x");
        assert_eq!(text(r"\textstyle\frac{a}{b}"), "a/b");
        assert_eq!(text(r"\sum\limits_{i=1}^{n}"), "∑i=1n");
        assert_eq!(text(r"a\,b\;c\:d"), "a b c d");
        assert_eq!(text(r"a\!b"), "ab");
        assert_eq!(text(r"a\quad b"), "a\u{2003} b");
        assert_eq!(text(r"a\qquad b"), "a\u{2003}\u{2003} b");
    }

    /// 命名定界符与空定界符：lbrace / lfloor / | 都查符号表，`.` 不显示。
    #[test]
    fn named_and_empty_delimiters() {
        assert_eq!(text(r"\left\lbrace x\right\rbrace"), "{ x}");
        assert_eq!(text(r"\left\lfloor x\right\rfloor"), "⌊ x⌋");
        assert_eq!(text(r"\left| x\right|"), "| x|");
        assert_eq!(text(r"\left.x\right."), "x");
    }

    /// 自然尺寸估算：内容越多越宽，分式比单行高，矩阵尺寸为正。
    #[test]
    fn measure_grows_with_content_and_is_positive() {
        let m = |src: &str| measure(&Parser::new(src).sequence(None), 17.);
        let (w1, h1) = m("x");
        let (w2, _) = m("xxxxx");
        assert!(w2 > w1, "更长的行应更宽");
        let (_, hf) = m(r"\frac{a}{b}");
        assert!(hf > h1, "分式应比单行高");
        let (wm, hm) = m(r"\begin{pmatrix}1&2\\3&4\end{pmatrix}");
        assert!(wm > 0. && hm > 0., "矩阵尺寸应为正");
        // 空 pieces 仍有基础行高，宽为 0。
        let (we, he) = measure(&[], 17.);
        assert_eq!(we, 0.);
        assert!(he > 0.);
    }

    /// 未知命令退化成命令名本身（不漏反斜杠），未知环境不带定界符，都不 panic。
    #[test]
    fn unknown_commands_and_environments_degrade() {
        assert_eq!(text(r"\unknowncmd"), "unknowncmd");
        assert_eq!(text(r"\begin{whatever}a&b\end{whatever}"), "a b");
        let pieces = Parser::new(r"\begin{whatever}a\end{whatever}").sequence(None);
        match pieces.last() {
            Some(Piece::Matrix { delim, .. }) => {
                assert_eq!(delim.left, "");
                assert_eq!(delim.right, "");
            }
            other => panic!("未知环境也应排成网格，实际 {other:?}"),
        }
    }

    /// 畸形 / 截断输入只退化、不 panic——这是「可读近似」的底线。
    #[test]
    fn malformed_input_never_panics() {
        for src in [
            r"\frac{a}",
            r"\sqrt",
            r"\sqrt[",
            r"\begin{matrix}",
            r"\begin{pmatrix}1&2\end{pmatrix}",
            r"x^",
            r"x_",
            r"\left",
            r"\right",
            r"\overset{a}",
            r"\binom{n}",
            r"}{",
            r"\end{matrix}",
            r"\begin{cases}a&b\\",
        ] {
            let _ = text(src);
            let _ = measure(&Parser::new(src).sequence(None), 17.);
        }
    }
}
