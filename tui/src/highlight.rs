//! Syntax highlighting for specifications, YAML test data and C#: one [`Tok`] per character.

use crate::theme::Theme;
use ratatui::style::{Color, Style};
use speci_local::parser::{MACROS, closing_quotes, macro_alias};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Text,
    Header,
    Macro,
    Keyword,
    Ty,
    Number,
    Str,
    Comment,
    Op,
    Punct,
    Func,
    Key,
}

impl Tok {
    pub const fn color(self, t: &Theme) -> Color {
        match self {
            Self::Text => t.fg,
            Self::Header => t.header,
            Self::Macro => t.macro_,
            Self::Keyword => t.keyword,
            Self::Ty => t.ty,
            Self::Number => t.number,
            Self::Str => t.string,
            Self::Comment => t.comment,
            Self::Op => t.op,
            Self::Punct => t.punct,
            Self::Func => t.func,
            Self::Key => t.key,
        }
    }
    pub fn style(self, t: &Theme) -> Style {
        Style::default().fg(self.color(t))
    }
}

pub const HEADERS: &[&str] = &[
    "Be", "Sa", "Ki", "Fv", "Ef", "Uf", "In", "Aux", "Out", "Fn", "Pre", "Post", "Def",
];
pub const KEYWORDS: &[&str] = &[
    "and",
    "or",
    "not",
    "és",
    "vagy",
    "nem",
    "mod",
    "div",
    "eleme",
    "in",
    "exists",
    "forall",
    "létezik",
    "minden",
    "true",
    "false",
    "igaz",
    "hamis",
    "ha",
    "egyébként",
    "if",
    "otherwise",
];
const GLYPHS: &str = "∈∀∃≠≤≥∤¬–−";

fn take_while(s: &str, f: impl Fn(char) -> bool) -> usize {
    s.char_indices()
        .find(|(_, c)| !f(*c))
        .map_or(s.len(), |(i, _)| i)
}

fn push(out: &mut Vec<Tok>, seg: &str, tok: Tok) {
    out.extend(seg.chars().map(|_| tok));
}

pub fn spec(text: &[char]) -> Vec<Tok> {
    let text: String = text.iter().collect();
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_str();
    let mut line_start = true;
    while let Some(c) = rest.chars().next() {
        let (n, tok) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), Tok::Comment)
        } else if let Some(inner) = rest.strip_prefix("/*") {
            (inner.find("*/").map_or(rest.len(), |i| i + 4), Tok::Comment)
        } else if let Some(close) = closing_quotes(c) {
            let body = &rest[c.len_utf8()..];
            let len = match body
                .char_indices()
                .find(|(_, d)| close.contains(d) || *d == '\n')
            {
                Some((i, d)) if d != '\n' => i + d.len_utf8(),
                Some((i, _)) => i,
                None => body.len(),
            };
            (c.len_utf8() + len, Tok::Str)
        } else if c.is_ascii_digit() {
            let n = take_while(rest, |c| c.is_ascii_digit() || c == '.');
            (rest[..n].find("..").unwrap_or(n), Tok::Number)
        } else if c.is_alphabetic() || c == '_' {
            let n = take_while(rest, |c| c.is_alphanumeric() || c == '_');
            let w = &rest[..n];
            let after = &rest[n..];
            if line_start && HEADERS.contains(&w) && after.starts_with(':') {
                (n + 1, Tok::Header)
            } else if macro_alias(w).is_some() || MACROS.iter().any(|m| m.0 == w) {
                (n, Tok::Macro)
            } else if KEYWORDS.contains(&w) {
                (n, Tok::Keyword)
            } else if w.starts_with(char::is_uppercase) {
                (n, Tok::Ty)
            } else if after.starts_with('(') {
                (n, Tok::Func)
            } else {
                (n, Tok::Text)
            }
        } else if GLYPHS.contains(c) || ["->", "<=", ">=", "!="].iter().any(|o| rest.starts_with(o))
        {
            (if c.is_ascii() { 2 } else { c.len_utf8() }, Tok::Op)
        } else if c.is_whitespace() {
            (c.len_utf8(), Tok::Text)
        } else {
            (c.len_utf8(), Tok::Punct)
        };
        let n = n.max(c.len_utf8());
        let seg = &rest[..n];
        push(&mut out, seg, tok);
        if !c.is_whitespace() {
            line_start = false;
        }
        if seg.ends_with('\n') || c == '\n' {
            line_start = true;
        }
        rest = &rest[n..];
    }
    out
}

pub fn data(text: &[char]) -> Vec<Tok> {
    let text: String = text.iter().collect();
    let mut out = Vec::with_capacity(text.len());
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            out.push(Tok::Text);
        }
        let mut rest = line;
        let ws = take_while(rest, char::is_whitespace);
        push(&mut out, &rest[..ws], Tok::Text);
        rest = &rest[ws..];
        if let Some(r) = rest.strip_prefix("- ") {
            push(&mut out, "- ", Tok::Punct);
            rest = r;
        }
        let k = take_while(rest, |c| c.is_alphanumeric() || c == '_');
        if k > 0 && rest[k..].starts_with(':') {
            push(&mut out, &rest[..k], Tok::Key);
            push(&mut out, ":", Tok::Punct);
            rest = &rest[k + 1..];
        }
        while let Some(c) = rest.chars().next() {
            let (n, tok) = if c == '#' {
                (rest.len(), Tok::Comment)
            } else if c == '"' || c == '\'' {
                (rest[1..].find(c).map_or(rest.len(), |i| i + 2), Tok::Str)
            } else if c.is_ascii_digit()
                || (c == '-' && rest[1..].starts_with(|d: char| d.is_ascii_digit()))
            {
                (
                    1 + take_while(&rest[1..], |c| c.is_ascii_digit() || c == '.'),
                    Tok::Number,
                )
            } else if c.is_alphanumeric() || c == '_' {
                let n = take_while(rest, |c| c.is_alphanumeric() || c == '_');
                let w = &rest[..n];
                if rest[n..].trim_start().starts_with(':') {
                    (n, Tok::Key)
                } else if matches!(w, "true" | "false" | "igaz" | "hamis") {
                    (n, Tok::Keyword)
                } else {
                    (n, Tok::Str)
                }
            } else if c.is_whitespace() {
                (c.len_utf8(), Tok::Text)
            } else {
                (c.len_utf8(), Tok::Punct)
            };
            push(&mut out, &rest[..n], tok);
            rest = &rest[n..];
        }
    }
    out
}

const CS_KEYWORDS: &[&str] = &[
    "using",
    "namespace",
    "class",
    "struct",
    "enum",
    "static",
    "public",
    "private",
    "protected",
    "internal",
    "readonly",
    "const",
    "new",
    "return",
    "if",
    "else",
    "for",
    "foreach",
    "while",
    "do",
    "switch",
    "case",
    "break",
    "continue",
    "default",
    "in",
    "out",
    "ref",
    "is",
    "as",
    "true",
    "false",
    "null",
    "this",
    "var",
    "void",
    "try",
    "catch",
    "finally",
    "throw",
    "get",
    "set",
];
const CS_TYPES: &[&str] = &[
    "int", "long", "double", "float", "bool", "string", "char", "byte", "short", "uint", "ulong",
    "decimal", "object",
];

/// Length of a quoted text starting at `rest` (`"` or `\'`), honouring `\` escapes; an unclosed
/// text ends with its line.
fn quoted_len(rest: &str) -> usize {
    let quote = rest.chars().next().unwrap_or('"');
    let mut escaped = false;
    for (i, c) in rest.char_indices().skip(1) {
        if c == '\n' {
            return i;
        }
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return i + 1;
        }
    }
    rest.len()
}

pub fn csharp(text: &[char]) -> Vec<Tok> {
    let text: String = text.iter().collect();
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_str();
    // the word before: `class Name`, `struct Name`, `new Name` name a type
    let mut after_type_kw = false;
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    while let Some(c) = rest.chars().next() {
        let (n, tok) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), Tok::Comment)
        } else if rest.starts_with("/*") {
            (rest.find("*/").map_or(rest.len(), |k| k + 2), Tok::Comment)
        } else if c == '"' || c == '\'' {
            (quoted_len(rest), Tok::Str)
        } else if c.is_ascii_digit() {
            (
                take_while(rest, |c| c.is_ascii_alphanumeric() || c == '.'),
                Tok::Number,
            )
        } else if ident(c) || c == '@' {
            let n = c.len_utf8() + take_while(&rest[c.len_utf8()..], ident);
            let word = rest[..n].trim_start_matches('@');
            let next = rest[n..].trim_start_matches([' ', '\t']).chars().next();
            let tok = if CS_KEYWORDS.contains(&word) {
                Tok::Keyword
            } else if CS_TYPES.contains(&word) || after_type_kw {
                Tok::Ty
            } else if next == Some('(') {
                Tok::Func
            } else if word.starts_with(char::is_uppercase) && matches!(next, Some('.' | ' ')) {
                Tok::Ty
            } else {
                Tok::Text
            };
            after_type_kw = matches!(word, "class" | "struct" | "enum" | "new");
            (n, tok)
        } else if c.is_whitespace() {
            (take_while(rest, char::is_whitespace), Tok::Text)
        } else if "{}()[];,.".contains(c) {
            (c.len_utf8(), Tok::Punct)
        } else {
            (c.len_utf8(), Tok::Op)
        };
        if !c.is_whitespace() && !ident(c) && c != '@' {
            after_type_kw = false;
        }
        push(&mut out, &rest[..n], tok);
        rest = &rest[n..];
    }
    out
}
