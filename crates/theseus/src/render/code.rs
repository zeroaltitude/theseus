//! Highlighted code (theseus-rn7s): a small lexer by hand for the dozen
//! languages a reply's fences name (Rust, Python, JS and TS, shell, JSON, TOML,
//! YAML, Go, C and C++, diff): keywords, strings, comments, numbers. It is not a
//! parser and does not try to be right about everything; where it is unsure it
//! leaves text plain. A language it does not know prints plain. Nothing here
//! runs until a code block appears, and the tables are statics, so there is
//! nothing to load.

use super::style::{Color, Style, StyledLine};

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    /// A type name, or a key.
    Type,
    /// An attribute, a macro, a variable, a table header.
    Meta,
}

impl Tok {
    /// The style a token is drawn in.
    pub fn style(self) -> Style {
        match self {
            Tok::Plain => Style::default(),
            Tok::Keyword => Style::fg(Color::Magenta),
            Tok::Str => Style::fg(Color::Green),
            Tok::Comment => Style::fg(Color::Gray),
            Tok::Number => Style::fg(Color::Yellow),
            Tok::Type => Style::fg(Color::Cyan),
            Tok::Meta => Style::fg(Color::Blue),
        }
    }
}

/// The languages known by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    Script,
    Shell,
    Json,
    Toml,
    Yaml,
    Go,
    C,
    Diff,
}

impl Lang {
    /// The language a fence's info string names (`rust`, `py`, `ts title=x`).
    pub fn from_info(info: &str) -> Option<Lang> {
        let word = info
            .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
            .next()?
            .to_ascii_lowercase();
        Some(match word.as_str() {
            "rust" | "rs" => Lang::Rust,
            "python" | "py" | "python3" => Lang::Python,
            "javascript" | "js" | "jsx" | "mjs" | "cjs" | "typescript" | "ts" | "tsx" => {
                Lang::Script
            }
            "bash" | "sh" | "shell" | "zsh" | "console" | "shellsession" => Lang::Shell,
            "json" | "jsonc" | "json5" => Lang::Json,
            "toml" => Lang::Toml,
            "yaml" | "yml" => Lang::Yaml,
            "go" | "golang" => Lang::Go,
            "c" | "h" | "cpp" | "c++" | "cc" | "cxx" | "hpp" => Lang::C,
            "diff" | "patch" => Lang::Diff,
            _ => return None,
        })
    }
}

/// How a language's text is cut into tokens.
struct Spec {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Characters that open a string ended by the same character.
    quotes: &'static str,
    /// `"""` and `'''` strings that run over lines.
    triple: bool,
    /// A quote whose string runs over lines (a template, a raw string).
    multiline: Option<char>,
    /// A quote that is a character literal, `'a'`, or else something else (a
    /// lifetime, a label).
    char_quote: Option<char>,
    /// An identifier that starts with a capital is a type.
    caps_are_types: bool,
    /// `$name` and `${name}`.
    vars: bool,
    /// An identifier ended by `!` is a macro.
    macros: bool,
    /// Where a line comment must follow whitespace (shell's `#`).
    comment_after_space: bool,
    /// Prefixes of an attribute or a directive, which run to the closing `]`
    /// (or the line's end).
    meta_prefixes: &'static [&'static str],
    /// A string followed by `:` is a key.
    string_keys: bool,
    /// A key at the line's start, ended by this character.
    line_key: Option<char>,
    /// A line that opens with `[` is a table's header.
    sections: bool,
}

const NONE: Spec = Spec {
    keywords: &[],
    types: &[],
    line_comment: &[],
    block_comment: None,
    quotes: "",
    triple: false,
    multiline: None,
    char_quote: None,
    caps_are_types: false,
    vars: false,
    macros: false,
    comment_after_space: false,
    meta_prefixes: &[],
    string_keys: false,
    line_key: None,
    sections: false,
};

const RUST: Spec = Spec {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    types: &[
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64", "bool", "char", "str",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: "\"",
    char_quote: Some('\''),
    caps_are_types: true,
    macros: true,
    meta_prefixes: &["#[", "#!["],
    ..NONE
};

const PYTHON: Spec = Spec {
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
        "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True",
        "try", "while", "with", "yield",
    ],
    types: &[
        "int", "str", "float", "bool", "list", "dict", "set", "tuple", "bytes",
    ],
    line_comment: &["#"],
    quotes: "\"'",
    triple: true,
    caps_are_types: true,
    meta_prefixes: &["@"],
    ..NONE
};

const SCRIPT: Spec = Spec {
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "private",
        "public",
        "readonly",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    types: &["string", "number", "boolean", "any", "unknown", "never"],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: "\"'",
    multiline: Some('`'),
    caps_are_types: true,
    ..NONE
};

const SHELL: Spec = Spec {
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if",
        "in", "local", "return", "select", "then", "until", "while",
    ],
    line_comment: &["#"],
    quotes: "\"'",
    vars: true,
    comment_after_space: true,
    ..NONE
};

const JSON: Spec = Spec {
    keywords: &["true", "false", "null"],
    quotes: "\"",
    string_keys: true,
    line_comment: &["//"],
    ..NONE
};

const TOML: Spec = Spec {
    keywords: &["true", "false"],
    line_comment: &["#"],
    quotes: "\"'",
    line_key: Some('='),
    sections: true,
    ..NONE
};

const YAML: Spec = Spec {
    keywords: &["true", "false", "null", "yes", "no", "on", "off"],
    line_comment: &["#"],
    quotes: "\"'",
    comment_after_space: true,
    line_key: Some(':'),
    string_keys: true,
    ..NONE
};

const GO: Spec = Spec {
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "false",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    types: &[
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
        "float32", "float64", "string", "bool", "byte", "rune", "error", "any",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: "\"",
    multiline: Some('`'),
    char_quote: Some('\''),
    caps_are_types: true,
    ..NONE
};

const C: Spec = Spec {
    keywords: &[
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "constexpr",
        "continue",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "extern",
        "false",
        "for",
        "goto",
        "if",
        "inline",
        "namespace",
        "new",
        "noexcept",
        "nullptr",
        "operator",
        "private",
        "protected",
        "public",
        "return",
        "sizeof",
        "static",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "true",
        "try",
        "typedef",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    types: &[
        "int", "long", "short", "char", "float", "double", "void", "unsigned", "signed", "bool",
        "size_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t",
        "int64_t", "string", "vector",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: "\"",
    char_quote: Some('\''),
    meta_prefixes: &["#"],
    ..NONE
};

fn spec(lang: Lang) -> &'static Spec {
    match lang {
        Lang::Rust => &RUST,
        Lang::Python => &PYTHON,
        Lang::Script => &SCRIPT,
        Lang::Shell => &SHELL,
        Lang::Json => &JSON,
        Lang::Toml => &TOML,
        Lang::Yaml => &YAML,
        Lang::Go => &GO,
        Lang::C | Lang::Diff => &C,
    }
}

/// What a line starts inside, from the line before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inside {
    Code,
    /// A block comment, until its end.
    Comment(&'static str),
    /// A string that runs over lines, until its closing text.
    Str(&'static str),
}

/// A language's lexer, line by line: it carries what a line leaves open.
pub struct Highlighter {
    lang: Lang,
    inside: Inside,
}

impl Highlighter {
    pub fn new(lang: Lang) -> Self {
        Self {
            lang,
            inside: Inside::Code,
        }
    }

    /// One line of code as a styled line.
    pub fn line(&mut self, text: &str) -> StyledLine {
        let mut out = StyledLine::default();
        for (tok, s) in self.tokens(text) {
            out.push(&s, &tok.style());
        }
        out
    }

    /// The line cut into tokens, in order, whole.
    fn tokens(&mut self, text: &str) -> Vec<(Tok, String)> {
        if self.lang == Lang::Diff {
            return vec![diff_line(text)];
        }
        let mut lx = Lexer {
            sp: spec(self.lang),
            chars: text.chars().collect(),
            i: 0,
            out: Vec::new(),
        };
        lx.run(&mut self.inside);
        lx.out
    }
}

/// A diff line: the added and the removed, a hunk's header, a file's.
fn diff_line(text: &str) -> (Tok, String) {
    let style = if text.starts_with("+++") || text.starts_with("---") {
        Tok::Meta
    } else if text.starts_with("@@") {
        Tok::Type
    } else if text.starts_with('+') {
        Tok::Str
    } else if text.starts_with('-') {
        Tok::Keyword
    } else if text.starts_with("diff ") || text.starts_with("index ") {
        Tok::Meta
    } else {
        Tok::Plain
    };
    (style, text.to_string())
}

struct Lexer {
    sp: &'static Spec,
    chars: Vec<char>,
    i: usize,
    out: Vec<(Tok, String)>,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Lexer {
    fn run(&mut self, inside: &mut Inside) {
        self.resume(inside);
        self.line_start();
        while self.i < self.chars.len() {
            self.step(inside);
        }
    }

    fn emit(&mut self, tok: Tok, to: usize) {
        let to = to.min(self.chars.len());
        if to <= self.i {
            return;
        }
        let s: String = self.chars[self.i..to].iter().collect();
        self.i = to;
        match self.out.last_mut() {
            Some((t, text)) if *t == tok => text.push_str(&s),
            _ => self.out.push((tok, s)),
        }
    }

    fn starts(&self, pat: &str) -> bool {
        pat.chars()
            .enumerate()
            .all(|(k, c)| self.chars.get(self.i + k) == Some(&c))
    }

    /// Where `pat` is found from `from`, as the index after it.
    fn find_end(&self, pat: &str, from: usize) -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        (from..self.chars.len().saturating_sub(p.len().saturating_sub(1)))
            .find(|&at| self.chars[at..at + p.len()] == p[..])
            .map(|at| at + p.len())
    }

    /// A line that starts inside a comment or a string goes on in it.
    fn resume(&mut self, inside: &mut Inside) {
        let (end, tok) = match *inside {
            Inside::Code => return,
            Inside::Comment(end) => (end, Tok::Comment),
            Inside::Str(end) => (end, Tok::Str),
        };
        match self.find_end(end, 0) {
            Some(to) => {
                self.emit(tok, to);
                *inside = Inside::Code;
            }
            None => self.emit(tok, self.chars.len()),
        }
    }

    /// TOML's headers, and a key at the line's start.
    fn line_start(&mut self) {
        if self.i >= self.chars.len() {
            return;
        }
        let indent = self.chars[self.i..]
            .iter()
            .take_while(|c| c.is_whitespace())
            .count();
        self.emit(Tok::Plain, self.i + indent);
        if self.sp.sections && self.chars.get(self.i) == Some(&'[') {
            let to = self.chars.len();
            let end = self.chars[self.i..].iter().position(|c| *c == '#');
            self.emit(Tok::Meta, end.map_or(to, |e| self.i + e));
            return;
        }
        if let Some(sep) = self.sp.line_key {
            self.key(sep);
        }
    }

    /// A key and its separator at the line's start (`- name: x`, `a.b = 1`).
    fn key(&mut self, sep: char) {
        if sep == ':' && self.starts("- ") {
            self.emit(Tok::Plain, self.i + 2);
        }
        let n = self.chars[self.i..]
            .iter()
            .take_while(|c| is_ident(**c) || **c == '-' || **c == '.')
            .count();
        let mut after = self.i + n;
        while self.chars.get(after) == Some(&' ') && sep == '=' {
            after += 1;
        }
        let ends = self.chars.get(after) == Some(&sep);
        let spaced = sep != ':' || self.chars.get(after + 1).is_none_or(|c| *c == ' ');
        if n > 0 && ends && spaced {
            self.emit(Tok::Type, self.i + n);
        }
    }

    fn step(&mut self, inside: &mut Inside) {
        if self.try_comment(inside) || self.try_meta() || self.try_string(inside) {
            return;
        }
        let c = self.chars[self.i];
        if c.is_ascii_digit() && !self.prev_is_ident() {
            self.number();
        } else if c == '$' && self.sp.vars {
            self.var();
        } else if is_ident_start(c) {
            self.ident();
        } else {
            self.emit(Tok::Plain, self.i + 1);
        }
    }

    fn prev_is_ident(&self) -> bool {
        self.i > 0 && is_ident(self.chars[self.i - 1])
    }

    fn prev_is_space(&self) -> bool {
        self.i == 0 || self.chars[self.i - 1].is_whitespace()
    }

    fn try_comment(&mut self, inside: &mut Inside) -> bool {
        let sp = self.sp;
        if let Some((open, close)) = sp.block_comment {
            if self.starts(open) {
                match self.find_end(close, self.i + open.len()) {
                    Some(to) => self.emit(Tok::Comment, to),
                    None => {
                        self.emit(Tok::Comment, self.chars.len());
                        *inside = Inside::Comment(close);
                    }
                }
                return true;
            }
        }
        let opens = sp.line_comment.iter().any(|m| self.starts(m));
        if opens && (!sp.comment_after_space || self.prev_is_space()) {
            self.emit(Tok::Comment, self.chars.len());
            return true;
        }
        false
    }

    /// An attribute or a directive.
    fn try_meta(&mut self) -> bool {
        let sp = self.sp;
        let at_start = self.chars[..self.i].iter().all(|c| c.is_whitespace());
        let Some(p) = sp.meta_prefixes.iter().find(|p| self.starts(p)) else {
            return false;
        };
        // `#` opens a directive only at a line's start; `@` a decorator too.
        if (*p == "#" || *p == "@") && !at_start {
            return false;
        }
        let to = if p.ends_with('[') {
            self.chars[self.i..]
                .iter()
                .position(|c| *c == ']')
                .map_or(self.chars.len(), |e| self.i + e + 1)
        } else {
            self.chars.len()
        };
        self.emit(Tok::Meta, to);
        true
    }

    fn try_string(&mut self, inside: &mut Inside) -> bool {
        let sp = self.sp;
        let c = self.chars[self.i];
        if sp.triple {
            for q in ["\"\"\"", "'''"] {
                if self.starts(q) {
                    return self.block_string(q, inside);
                }
            }
        }
        if sp.multiline == Some(c) {
            let q: &'static str = if c == '`' { "`" } else { "\"" };
            return self.block_string(q, inside);
        }
        if sp.char_quote == Some(c) {
            return self.char_literal();
        }
        if sp.quotes.contains(c) {
            self.quoted(c);
            return true;
        }
        false
    }

    /// A string that may run over lines, from `q` to its closing `q`.
    fn block_string(&mut self, q: &'static str, inside: &mut Inside) -> bool {
        match self.find_end(q, self.i + q.len()) {
            Some(to) => self.emit(Tok::Str, to),
            None => {
                self.emit(Tok::Str, self.chars.len());
                *inside = Inside::Str(q);
            }
        }
        true
    }

    /// `'a'` or `'\n'`; a lifetime or a label is left to be read as a name.
    fn char_literal(&mut self) -> bool {
        let at = self.i;
        let closes = match self.chars.get(at + 1) {
            Some('\\') => self.chars[at + 2..]
                .iter()
                .position(|c| *c == '\'')
                .map(|p| at + 3 + p),
            Some(_) if self.chars.get(at + 2) == Some(&'\'') => Some(at + 3),
            _ => None,
        };
        match closes {
            Some(to) => {
                self.emit(Tok::Str, to);
                true
            }
            None => false,
        }
    }

    /// A string of one quote character, with `\` escapes, to its end or the
    /// line's.
    fn quoted(&mut self, q: char) {
        let mut at = self.i + 1;
        while at < self.chars.len() {
            match self.chars[at] {
                '\\' => at += 2,
                c if c == q => {
                    at += 1;
                    break;
                }
                _ => at += 1,
            }
        }
        let to = at.min(self.chars.len());
        let key = self.sp.string_keys && self.followed_by_colon(to);
        self.emit(if key { Tok::Type } else { Tok::Str }, to);
    }

    fn followed_by_colon(&self, from: usize) -> bool {
        self.chars[from.min(self.chars.len())..]
            .iter()
            .find(|c| !c.is_whitespace())
            == Some(&':')
    }

    fn number(&mut self) {
        let mut at = self.i;
        while at < self.chars.len() {
            let c = self.chars[at];
            let dot = c == '.' && self.chars.get(at + 1).is_some_and(char::is_ascii_digit);
            if c.is_alphanumeric() || c == '_' || dot {
                at += 1;
            } else {
                break;
            }
        }
        self.emit(Tok::Number, at);
    }

    /// `$name`, `${name}`, `$1`, `$?`.
    fn var(&mut self) {
        let mut at = self.i + 1;
        if self.chars.get(at) == Some(&'{') {
            at = self.chars[at..]
                .iter()
                .position(|c| *c == '}')
                .map_or(self.chars.len(), |e| at + e + 1);
        } else if self.chars.get(at).is_some_and(|c| is_ident(*c)) {
            while self.chars.get(at).is_some_and(|c| is_ident(*c)) {
                at += 1;
            }
        } else if self.chars.get(at).is_some() {
            at += 1;
        }
        self.emit(Tok::Meta, at);
    }

    fn ident(&mut self) {
        let sp = self.sp;
        let end = self.chars[self.i..]
            .iter()
            .position(|c| !is_ident(*c))
            .map_or(self.chars.len(), |n| self.i + n);
        let word: String = self.chars[self.i..end].iter().collect();
        let after = self.chars.get(end);
        let tok = if sp.keywords.contains(&word.as_str()) {
            Tok::Keyword
        } else if sp.types.contains(&word.as_str())
            || (sp.caps_are_types && word.chars().next().is_some_and(char::is_uppercase))
        {
            Tok::Type
        } else if sp.macros && after == Some(&'!') && self.chars.get(end + 1) != Some(&'=') {
            self.emit(Tok::Meta, end + 1);
            return;
        } else {
            Tok::Plain
        };
        self.emit(tok, end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(lang: Lang, src: &str) -> Vec<Vec<(Tok, String)>> {
        let mut h = Highlighter::new(lang);
        src.lines().map(|l| h.tokens(l)).collect()
    }

    fn has(lang: Lang, src: &str, tok: Tok, text: &str) -> bool {
        toks(lang, src)
            .iter()
            .flatten()
            .any(|(t, s)| *t == tok && s == text)
    }

    #[test]
    fn a_fences_info_names_its_language() {
        assert_eq!(Lang::from_info("rust,ignore"), Some(Lang::Rust));
        assert_eq!(Lang::from_info("TS title=x"), Some(Lang::Script));
        assert_eq!(Lang::from_info("c++"), Some(Lang::C));
        assert_eq!(Lang::from_info("brainfuck"), None);
        assert_eq!(Lang::from_info(""), None);
    }

    #[test]
    fn each_language_marks_a_keyword_a_string_a_comment_and_a_number() {
        let rs = "fn main() { let x = \"a\"; // note\n let n = 42u8; }";
        assert!(has(Lang::Rust, rs, Tok::Keyword, "fn"));
        assert!(has(Lang::Rust, rs, Tok::Str, "\"a\""));
        assert!(has(Lang::Rust, rs, Tok::Comment, "// note"));
        assert!(has(Lang::Rust, rs, Tok::Number, "42u8"));
        let py = "def f(x):\n    return 'a' + \"\"\"b\nc\"\"\"  # t\nn = 3.5";
        assert!(has(Lang::Python, py, Tok::Keyword, "def"));
        assert!(has(Lang::Python, py, Tok::Str, "'a'"));
        assert!(has(Lang::Python, py, Tok::Comment, "# t"));
        assert!(has(Lang::Python, py, Tok::Number, "3.5"));
        let js = "const a = `x\ny`; /* c */ let b = 0x1F; // d";
        assert!(has(Lang::Script, js, Tok::Keyword, "const"));
        assert!(has(Lang::Script, js, Tok::Comment, "/* c */"));
        assert!(has(Lang::Script, js, Tok::Number, "0x1F"));
        let sh = "if [ -n \"$HOME\" ]; then echo 'x' # c\nfi";
        assert!(has(Lang::Shell, sh, Tok::Keyword, "then"));
        assert!(has(Lang::Shell, sh, Tok::Str, "'x'"));
        assert!(has(Lang::Shell, sh, Tok::Comment, "# c"));
        assert!(has(Lang::Shell, "echo $HOME ${A}", Tok::Meta, "$HOME"));
        let go = "func main() {\n\ts := `raw\nstr`\n\tr := 'x' // c\n}";
        assert!(has(Lang::Go, go, Tok::Keyword, "func"));
        assert!(has(Lang::Go, go, Tok::Str, "'x'"));
        assert!(has(Lang::Go, go, Tok::Comment, "// c"));
        let c = "#include <x.h>\nint main() { return 0; } /* a\nb */";
        assert!(has(Lang::C, c, Tok::Meta, "#include <x.h>"));
        assert!(has(Lang::C, c, Tok::Type, "int"));
        assert!(has(Lang::C, c, Tok::Keyword, "return"));
        assert!(has(Lang::C, c, Tok::Comment, "b */"));
    }

    #[test]
    fn data_languages_mark_keys_and_values() {
        let json = "{\"a\": [1, true, null, \"b\"]}";
        assert!(has(Lang::Json, json, Tok::Type, "\"a\""));
        assert!(has(Lang::Json, json, Tok::Number, "1"));
        assert!(has(Lang::Json, json, Tok::Keyword, "null"));
        assert!(has(Lang::Json, json, Tok::Str, "\"b\""));
        let toml = "[package]\nname = \"x\" # c\nn = 3\nok = true";
        assert!(has(Lang::Toml, toml, Tok::Meta, "[package]"));
        assert!(has(Lang::Toml, toml, Tok::Type, "name"));
        assert!(has(Lang::Toml, toml, Tok::Str, "\"x\""));
        assert!(has(Lang::Toml, toml, Tok::Comment, "# c"));
        assert!(has(Lang::Toml, toml, Tok::Keyword, "true"));
        let yaml = "- name: x # c\n  on: yes\n  n: 4";
        assert!(has(Lang::Yaml, yaml, Tok::Type, "name"));
        assert!(has(Lang::Yaml, yaml, Tok::Comment, "# c"));
        assert!(has(Lang::Yaml, yaml, Tok::Keyword, "yes"));
        assert!(has(Lang::Yaml, yaml, Tok::Number, "4"));
    }

    #[test]
    fn a_diff_marks_its_added_removed_and_hunk_lines() {
        let d = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n same";
        let t: Vec<Tok> = toks(Lang::Diff, d).iter().map(|l| l[0].0).collect();
        assert_eq!(
            t,
            [
                Tok::Meta,
                Tok::Meta,
                Tok::Type,
                Tok::Keyword,
                Tok::Str,
                Tok::Plain
            ]
        );
    }

    #[test]
    fn a_lifetime_is_not_a_string_and_a_lexer_keeps_every_character() {
        let src = "fn f<'a>(x: &'a str) -> char { 'z' }";
        assert!(!has(Lang::Rust, src, Tok::Str, "'a>(x: &'"));
        assert!(has(Lang::Rust, src, Tok::Str, "'z'"));
        for lang in [
            Lang::Rust,
            Lang::Python,
            Lang::Shell,
            Lang::Json,
            Lang::Toml,
            Lang::Yaml,
        ] {
            let text = "x = 'a \" # é\\ $q [z]: 日本 \"unterminated";
            let joined: String = toks(lang, text)[0]
                .iter()
                .map(|(_, s)| s.as_str())
                .collect();
            assert_eq!(joined, text);
        }
    }

    #[test]
    fn a_line_is_a_styled_line_of_its_tokens() {
        let mut h = Highlighter::new(Lang::Rust);
        let l = h.line("let x = 1;");
        assert_eq!(l.plain(), "let x = 1;");
        assert!(l.ansi().contains("\x1b[35mlet\x1b[0m"));
        assert!(l.ansi().contains("\x1b[33m1\x1b[0m"));
    }
}
