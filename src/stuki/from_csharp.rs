//! C# method(s) → structograms: a hand-rolled recursive-descent scanner (no C# compiler behind
//! it) tuned for textbook algorithms. Every method found becomes its own structogram.
//!
//! Supported: `if`/`else if`/`else`, `while`, `do … while`, `for` (an init statement, a pre-test
//! loop and the increment as its last statement), `foreach` (`x in collection`, `x` goes to the
//! variables box), `switch`/`case`/`default`, `break`/`continue`, local declarations (pulled into
//! the variables box with their C# types) and a trailing top-level `return` (the footer).
//! `try`/`catch`/`finally`, `using`/`lock`/`fixed` and `checked`/`unchecked`/`unsafe` are
//! flattened to their bodies. `Console.WriteLine(x)` becomes `out x`, `x = Console.ReadLine()`
//! becomes `inp x`; `int.Parse(..)`, `Convert.ToInt32(..)` and `ref`/`out`/`in` are dropped.
//! Anything unrecognized stays as a literal statement. Not supported: lambdas/LINQ, local
//! functions, properties, attributes.

use super::model::{Case, Diagram, Kind, Node, Program};
use super::rich::Rich;

type Res<T> = Result<T, String>;

/// What the conversion made.
pub struct Converted {
    pub program: Program,
    /// The names of the methods that became structograms, in source order.
    pub methods: Vec<String>,
}

/// The sample the old script called `--dummy`: `Main` and a Fibonacci function.
pub const DEMO_SOURCE: &str = "public class Program
{
    public static void Main(string[] args)
    {
        Console.WriteLine(\"Enter n:\");
        int n = int.Parse(Console.ReadLine());
        Console.WriteLine(Fibonacci(n));
    }

    public static int Fibonacci(int n)
    {
        int a = 0;
        int b = 1;
        for (int i = 0; i < n; i++)
        {
            int tmp = a + b;
            a = b;
            b = tmp;
        }
        return a;
    }
}
";

/// Converts the C# source (a whole class, or just methods) into structograms.
pub fn from_csharp(source: &str) -> Res<Converted> {
    let src: Vec<char> = strip_comments(source).chars().collect();
    let mut methods = find_methods(&src)?;
    if methods.is_empty() {
        // loose statements (a snippet, or a top-level program): one body called Main
        let has_type = ["class", "struct", "interface", "namespace"]
            .iter()
            .any(|k| (0..src.len()).any(|i| match_word(&src, i, k) && word_start(&src, i)));
        if has_type || src.iter().all(|c| c.is_whitespace()) {
            return Err(
                "Could not find a method (Type Name(...) { ... }) in the C# program".to_string(),
            );
        }
        methods.push(Method {
            name: "Main".into(),
            params: String::new(),
            body: (0, src.len()),
        });
    }
    let mut diagrams = Vec::new();
    let mut names = Vec::new();
    for m in &methods {
        let mut p = Parser {
            s: &src,
            vars: Vec::new(),
            ret: None,
        };
        p.params(&m.params);
        let body = p.statements(m.body.0, m.body.1, true)?;
        let mut d = Diagram::new();
        d.name = Rich::plain(&m.name);
        d.variables = Rich::plain(&super::model::group_variables(&p.vars, " : ").join("\n"));
        d.show_variables = !p.vars.is_empty();
        d.show_header = true;
        d.show_return = p.ret.is_some();
        d.ret = Rich::plain(p.ret.as_deref().unwrap_or(""));
        d.body = seq(body);
        names.push(m.name.clone());
        diagrams.push(d);
    }
    Ok(Converted {
        program: Program {
            diagrams,
            ..Program::default()
        },
        methods: names,
    })
}

fn seq(items: Vec<Node>) -> Vec<Node> {
    if items.is_empty() {
        vec![Node::noop()]
    } else {
        items
    }
}

fn stmt(text: String) -> Node {
    node(Kind::Statement(Rich::plain(&text)))
}

fn node(kind: Kind) -> Node {
    Node {
        id: super::model::fresh_id(),
        bg: None,
        kind,
    }
}

// ------------------------------------------------------------------ scanning helpers

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_start(s: &[char], i: usize) -> bool {
    i == 0 || !is_word(s[i - 1])
}

fn at(s: &[char], i: usize, lit: &str) -> bool {
    lit.chars().zip(i..).all(|(c, j)| s.get(j) == Some(&c))
}

/// `lit` at `i` as a whole word.
fn match_word(s: &[char], i: usize, lit: &str) -> bool {
    at(s, i, lit) && s.get(i + lit.chars().count()).is_none_or(|c| !is_word(*c))
}

fn skip_ws(s: &[char], mut i: usize, limit: usize) -> usize {
    while i < limit && s[i].is_whitespace() {
        i += 1;
    }
    i
}

/// `[A-Za-z_]\w*` at `i`.
fn peek_word(s: &[char], i: usize) -> Option<String> {
    let c = *s.get(i)?;
    if !(c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    Some(s[i..].iter().take_while(|c| is_word(**c)).collect())
}

fn text(s: &[char]) -> String {
    s.iter().collect()
}

/// If a string or character literal starts at `i`, the index after it.
fn skip_literal(s: &[char], i: usize) -> Option<usize> {
    let n = s.len();
    let q = s[i];
    if q != '"' && q != '\'' {
        return None;
    }
    let verbatim =
        q == '"' && ((i > 0 && s[i - 1] == '@') || (i > 1 && s[i - 1] == '$' && s[i - 2] == '@'));
    let mut j = i + 1;
    while j < n {
        if verbatim {
            if s[j] == '"' {
                if s.get(j + 1) == Some(&'"') {
                    j += 2;
                    continue;
                }
                return Some(j + 1);
            }
        } else if s[j] == '\\' {
            j += 2;
            continue;
        } else if s[j] == q {
            return Some(j + 1);
        }
        j += 1;
    }
    Some(n)
}

/// The match of the bracket at `open`, literals skipped.
fn find_matching(s: &[char], open: usize) -> Res<usize> {
    let o = s.get(open).copied().unwrap_or(' ');
    let c = match o {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return Err(format!("expected a bracket at character {open}")),
    };
    let (mut depth, mut i) = (0i32, open);
    while i < s.len() {
        if let Some(j) = skip_literal(s, i) {
            i = j;
            continue;
        }
        if s[i] == o {
            depth += 1;
        } else if s[i] == c {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
        i += 1;
    }
    Err(format!("Unbalanced {o} (starting at character {open})"))
}

/// The `;` that ends the statement starting at `i`.
fn find_stmt_end(s: &[char], i: usize, limit: usize) -> usize {
    let (mut depth, mut j) = (0i32, i);
    while j < limit {
        if let Some(k) = skip_literal(s, j) {
            j = k;
            continue;
        }
        match s[j] {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ';' if depth == 0 => return j,
            _ => {}
        }
        j += 1;
    }
    limit
}

/// Splits at `sep` outside brackets and literals (`angles`: `<…>` count as brackets too, for
/// parameter lists such as `Dictionary<int, string> d`).
fn split_top(s: &str, sep: char, angles: bool) -> Vec<String> {
    let s: Vec<char> = s.chars().collect();
    let (mut parts, mut depth, mut start, mut i) = (Vec::new(), 0i32, 0, 0);
    while i < s.len() {
        if let Some(j) = skip_literal(&s, i) {
            i = j;
            continue;
        }
        match s[i] {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '<' if angles => depth += 1,
            '>' if angles => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(text(&s[start..i]));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(text(&s[start..]));
    parts
}

/// Removes comments and the preprocessor lines that carry no code (`#region`, `#pragma`, …);
/// `#if`/`#else`/`#endif` stay so both sides of a condition remain visible.
fn strip_comments(src: &str) -> String {
    let mut pre = String::with_capacity(src.len());
    for (i, line) in src.split('\n').enumerate() {
        if i > 0 {
            pre.push('\n');
        }
        if let Some(rest) = line.trim_start_matches([' ', '\t']).strip_prefix('#') {
            let w: String = rest.chars().take_while(|c| is_word(*c)).collect();
            if matches!(
                w.as_str(),
                "region" | "endregion" | "pragma" | "nullable" | "define" | "undef"
            ) {
                continue;
            }
        }
        pre.push_str(line);
    }
    let s: Vec<char> = pre.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == '/' && s.get(i + 1) == Some(&'/') {
            while i < s.len() && s[i] != '\n' {
                i += 1;
            }
        } else if s[i] == '/' && s.get(i + 1) == Some(&'*') {
            i = (i + 2..s.len())
                .find(|&j| s[j] == '*' && s.get(j + 1) == Some(&'/'))
                .map_or(s.len(), |j| j + 2);
            out.push(' ');
        } else if let Some(j) = skip_literal(&s, i) {
            out.extend(&s[i..j.min(s.len())]);
            i = j;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

// ------------------------------------------------------------------ simplifications

const BUILTIN: [&str; 13] = [
    "int", "uint", "long", "ulong", "short", "ushort", "byte", "sbyte", "double", "float",
    "decimal", "bool", "char",
];
const CONVERT_TO: [&str; 14] = [
    "Int32", "Int64", "Int16", "Byte", "SByte", "UInt32", "UInt64", "UInt16", "Double", "Single",
    "Decimal", "Boolean", "Char", "String",
];

/// `int.Parse(` / `Convert.ToInt32(`: (start, index of the paren).
fn find_parse_wrapper(s: &[char]) -> Option<(usize, usize)> {
    for i in 0..s.len() {
        if !word_start(s, i) {
            continue;
        }
        for t in BUILTIN {
            if at(s, i, t) && at(s, i + t.len(), ".Parse") {
                let j = skip_ws(s, i + t.len() + 6, s.len());
                if s.get(j) == Some(&'(') {
                    return Some((i, j));
                }
            }
        }
        if at(s, i, "Convert.To") {
            let w = peek_word(s, i + 10).unwrap_or_default();
            if CONVERT_TO.contains(&w.as_str()) {
                let j = skip_ws(s, i + 10 + w.len(), s.len());
                if s.get(j) == Some(&'(') {
                    return Some((i, j));
                }
            }
        }
    }
    None
}

fn strip_parse_wrappers(t: &str) -> String {
    let mut s: Vec<char> = t.chars().collect();
    for _ in 0..25 {
        let Some((start, open)) = find_parse_wrapper(&s) else {
            break;
        };
        let Ok(close) = find_matching(&s, open) else {
            break;
        };
        let inner = text(&s[open + 1..close]).trim().to_string();
        let mut n: Vec<char> = s[..start].to_vec();
        n.extend(inner.chars());
        n.extend(&s[close + 1..]);
        s = n;
    }
    text(&s)
}

/// Replaces every place where `f` matches (it returns the match length and the replacement).
fn replace_all(t: &str, f: impl Fn(&[char], usize) -> Option<(usize, String)>) -> String {
    let s: Vec<char> = t.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < s.len() {
        match f(&s, i) {
            Some((len, rep)) if len > 0 => {
                out.push_str(&rep);
                i += len;
            }
            _ => {
                out.push(s[i]);
                i += 1;
            }
        }
    }
    out
}

/// `Console\s*.\s*` at `i`: the index after it.
fn console_dot(s: &[char], i: usize) -> Option<usize> {
    if !(word_start(s, i) && at(s, i, "Console")) {
        return None;
    }
    let j = skip_ws(s, i + 7, s.len());
    (s.get(j) == Some(&'.')).then(|| skip_ws(s, j + 1, s.len()))
}

/// `ReadLine(…)`, `Read(…)` or `ReadKey(…)` at `j`: the index after the closing paren.
fn read_call(s: &[char], j: usize) -> Option<usize> {
    let w = peek_word(s, j)?;
    if !matches!(w.as_str(), "ReadLine" | "Read" | "ReadKey") {
        return None;
    }
    let k = skip_ws(s, j + w.len(), s.len());
    if s.get(k) != Some(&'(') {
        return None;
    }
    let mut e = k + 1;
    while e < s.len() && s[e] != '(' && s[e] != ')' {
        e += 1;
    }
    (s.get(e) == Some(&')')).then_some(e + 1)
}

fn replace_console_streams(t: &str) -> String {
    let t = replace_all(t, |s, i| {
        let j = console_dot(s, i)?;
        // `Console.In.ReadLine()` or `Console.ReadLine()`
        if at(s, j, "In") {
            let k = skip_ws(s, j + 2, s.len());
            if s.get(k) == Some(&'.')
                && let Some(e) = read_call(s, skip_ws(s, k + 1, s.len()))
            {
                return Some((e - i, "STDIN".into()));
            }
        }
        read_call(s, j).map(|e| (e - i, "STDIN".into()))
    });
    let stream = |name: &'static str, to: &'static str| {
        move |s: &[char], i: usize| {
            let j = console_dot(s, i)?;
            match_word(s, j, name).then(|| (j + name.len() - i, to.to_string()))
        }
    };
    let t = replace_all(&t, stream("In", "STDIN"));
    let t = replace_all(&t, stream("Out", "STDOUT"));
    replace_all(&t, stream("Error", "STDERR"))
}

fn simplify_generic(t: &str, console: bool) -> String {
    let t = strip_parse_wrappers(t);
    let t = replace_all(&t, |s, i| {
        if !word_start(s, i) {
            return None;
        }
        BUILTIN.iter().find_map(|b| {
            (at(s, i, b)
                && at(s, i + b.len(), ".TryParse")
                && s.get(i + b.len() + 9).is_none_or(|c| !is_word(*c)))
            .then(|| (b.len() + 9, "TryParse".to_string()))
        })
    });
    // ref / out / in modifiers
    let t = replace_all(&t, |s, i| {
        if !word_start(s, i) {
            return None;
        }
        ["ref", "out", "in"].iter().find_map(|k| {
            if !at(s, i, k) {
                return None;
            }
            let j = skip_ws(s, i + k.len(), s.len());
            (j > i + k.len()
                && s.get(j)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_'))
            .then(|| (j - i, String::new()))
        })
    });
    let t = if console {
        replace_console_streams(&t)
    } else {
        t
    };
    t.trim().to_string()
}

/// `Console.WriteLine(` and friends at the start: (the stream group, index of the paren).
fn console_write(s: &[char]) -> Option<(Option<&'static str>, usize)> {
    if !at(s, 0, "Console") {
        return None;
    }
    let mut j = 7;
    let mut group = None;
    if at(s, j, ".Out") {
        group = Some("Out");
        j += 4;
    } else if at(s, j, ".Error") {
        group = Some("Error");
        j += 6;
    }
    if s.get(j) != Some(&'.') {
        return None;
    }
    let w = peek_word(s, j + 1)?;
    if w != "WriteLine" && w != "Write" {
        return None;
    }
    let k = skip_ws(s, j + 1 + w.len(), s.len());
    (s.get(k) == Some(&'(')).then_some((group, k))
}

/// `Console.ReadLine(` / `Console.In.Read(` at the start: the index of the paren.
fn console_read(s: &[char]) -> Option<usize> {
    if !at(s, 0, "Console") {
        return None;
    }
    let mut j = 7;
    if at(s, j, ".In") {
        j += 3;
    }
    if s.get(j) != Some(&'.') {
        return None;
    }
    let w = peek_word(s, j + 1)?;
    if !matches!(w.as_str(), "ReadLine" | "Read" | "ReadKey") {
        return None;
    }
    let k = skip_ws(s, j + 1 + w.len(), s.len());
    (s.get(k) == Some(&'(')).then_some(k)
}

/// Does the call whose paren is at `open` run to the end of the text?
fn whole_call(s: &[char], open: usize) -> Option<usize> {
    let close = find_matching(s, open).ok()?;
    let end = s.iter().rposition(|c| !c.is_whitespace())?;
    (close == end).then_some(close)
}

/// `target = rhs`, the target being a name with an optional `[…]`.
fn split_assign(s: &[char]) -> Option<(String, String)> {
    let w = peek_word(s, 0)?;
    let mut j = w.chars().count();
    if s.get(j) == Some(&'[') {
        let c = s[j..].iter().position(|c| *c == ']')? + j;
        j = c + 1;
    }
    let target = text(&s[..j]);
    let k = skip_ws(s, j, s.len());
    if s.get(k) != Some(&'=') {
        return None;
    }
    let rhs = text(&s[k + 1..]).trim().to_string();
    (!rhs.is_empty()).then_some((target, rhs))
}

fn simplify_stmt(t: &str) -> String {
    let t = t.trim();
    let s: Vec<char> = t.chars().collect();
    if let Some((group, open)) = console_write(&s)
        && let Some(close) = whole_call(&s, open)
    {
        let inner = simplify_generic(&text(&s[open + 1..close]), true);
        let prefix = if group == Some("Error") {
            "STDERR.out"
        } else {
            "out"
        };
        return if inner.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix} {inner}")
        };
    }
    if let Some(open) = console_read(&s)
        && whole_call(&s, open).is_some()
    {
        return "inp".into();
    }
    let simplified = simplify_generic(t, false);
    let sc: Vec<char> = simplified.chars().collect();
    if let Some((target, rhs)) = split_assign(&sc) {
        let rc: Vec<char> = rhs.chars().collect();
        if let Some(open) = console_read(&rc)
            && whole_call(&rc, open).is_some()
        {
            return format!("inp {target}");
        }
    }
    replace_console_streams(&simplified).trim().to_string()
}

fn simplify_cond(t: &str) -> String {
    simplify_generic(t.trim(), true)
}

/// Statement text on one line.
fn one_line(t: &str) -> String {
    t.split('\n').map(str::trim).collect::<Vec<_>>().join(" ")
}

// ------------------------------------------------------------------ the body parser

struct Parser<'a> {
    s: &'a [char],
    /// `(name, type)`, in declared order.
    vars: Vec<(String, String)>,
    ret: Option<String>,
}

/// Words that look like `Type name` at the start of a statement but are not declarations.
const NOT_TYPES: [&str; 12] = [
    "await", "goto", "new", "throw", "return", "yield", "case", "else", "in", "is", "as", "do",
];

impl Parser<'_> {
    fn add_var(&mut self, name: &str, typ: &str) {
        let e = (name.to_string(), typ.to_string());
        if !self.vars.contains(&e) {
            self.vars.push(e);
        }
    }

    fn params(&mut self, list: &str) {
        for p in split_top(list, ',', true) {
            let mut p = p.trim().to_string();
            if p.is_empty() {
                continue;
            }
            for m in ["ref", "out", "in", "params", "this"] {
                if let Some(r) = p
                    .strip_prefix(m)
                    .filter(|r| r.starts_with(char::is_whitespace))
                {
                    p = r.trim_start().to_string();
                    break;
                }
            }
            if let Some(i) = p.find('=') {
                p.truncate(i);
                p = p.trim().to_string();
            }
            match p.rsplit_once(char::is_whitespace) {
                Some((t, n)) => self.add_var(n.trim(), t.trim()),
                None => self.add_var(&p, "var"),
            }
        }
    }

    fn statements(&mut self, i: usize, limit: usize, top: bool) -> Res<Vec<Node>> {
        let mut items = Vec::new();
        let mut pos = i;
        loop {
            pos = skip_ws(self.s, pos, limit);
            if pos >= limit {
                break;
            }
            let (more, p) = self.statement(pos, limit, top)?;
            items.extend(more);
            pos = p;
        }
        Ok(items)
    }

    /// A `{ block }` or a single statement.
    fn body(&mut self, pos: usize, limit: usize) -> Res<(Vec<Node>, usize)> {
        let pos = skip_ws(self.s, pos, limit);
        if pos < limit && self.s[pos] == '{' {
            let close = find_matching(self.s, pos)?;
            let items = self.statements(pos + 1, close, false)?;
            return Ok((items, close + 1));
        }
        self.statement(pos, limit, false)
    }

    fn paren(&self, pos: usize, limit: usize, what: &str) -> Res<(String, usize)> {
        let pos = skip_ws(self.s, pos, limit);
        if self.s.get(pos) != Some(&'(') {
            return Err(format!("expected '(' after {what}"));
        }
        let close = find_matching(self.s, pos)?;
        Ok((text(&self.s[pos + 1..close]), close + 1))
    }

    fn statement(&mut self, pos: usize, limit: usize, top: bool) -> Res<(Vec<Node>, usize)> {
        let s = self.s;
        let word = peek_word(s, pos).unwrap_or_default();
        match word.as_str() {
            "if" => {
                let (n, p) = self.parse_if(pos, limit)?;
                return Ok((vec![n], p));
            }
            "while" => {
                let (cond, p) = self.paren(pos + 5, limit, "while")?;
                let (body, p) = self.body(p, limit)?;
                let kind = Kind::While {
                    cond: Rich::plain(&simplify_cond(&cond)),
                    body: seq(body),
                };
                return Ok((vec![node(kind)], p));
            }
            "do" => {
                let (body, p) = self.body(pos + 2, limit)?;
                let p = skip_ws(s, p, limit);
                if !match_word(s, p, "while") {
                    return Err("expected 'while' after the do block".into());
                }
                let (cond, p) = self.paren(p + 5, limit, "while")?;
                let mut p = skip_ws(s, p, limit);
                if s.get(p) == Some(&';') {
                    p += 1;
                }
                let kind = Kind::Repeat {
                    cond: Rich::plain(&simplify_cond(&cond)),
                    body: seq(body),
                };
                return Ok((vec![node(kind)], p));
            }
            "for" => return self.parse_for(pos, limit),
            "foreach" => return self.parse_foreach(pos, limit),
            "switch" => return self.parse_switch(pos, limit),
            "try" => return self.parse_try(pos, limit),
            "using" | "lock" | "fixed" => {
                let p = skip_ws(s, pos + word.len(), limit);
                let p = if s.get(p) == Some(&'(') {
                    skip_ws(s, find_matching(s, p)? + 1, limit)
                } else {
                    p
                };
                return self.body(p, limit);
            }
            "checked" | "unchecked" | "unsafe" => {
                let p = skip_ws(s, pos + word.len(), limit);
                if s.get(p) == Some(&'{') {
                    return self.body(p, limit);
                }
            }
            _ => {}
        }
        if s[pos] == '{' {
            let close = find_matching(s, pos)?;
            return Ok((self.statements(pos + 1, close, false)?, close + 1));
        }
        if word == "break" || word == "continue" {
            let semi = find_stmt_end(s, pos, limit);
            return Ok((vec![stmt(text(&s[pos..semi]).trim().to_string())], semi + 1));
        }
        if word == "return" {
            let semi = find_stmt_end(s, pos, limit);
            let expr = simplify_generic(&one_line(&text(&s[pos + 6..semi])), true);
            let next = semi + 1;
            if top && s[next.min(limit)..limit].iter().all(|c| c.is_whitespace()) {
                self.ret = Some(expr);
                return Ok((vec![], next));
            }
            let t = if expr.is_empty() {
                "return".to_string()
            } else {
                format!("return {expr}")
            };
            return Ok((vec![stmt(t)], next));
        }
        let semi = find_stmt_end(s, pos, limit);
        let raw = one_line(&text(&s[pos..semi]));
        let raw = raw.trim();
        let next = semi + 1;
        if raw.is_empty() {
            return Ok((vec![], next));
        }
        if let Some(items) = self.declaration(raw) {
            return Ok((items, next));
        }
        Ok((vec![stmt(simplify_stmt(raw))], next))
    }

    fn parse_try(&mut self, pos: usize, limit: usize) -> Res<(Vec<Node>, usize)> {
        let s = self.s;
        let (mut all, mut p) = self.body(pos + 3, limit)?;
        loop {
            let q = skip_ws(s, p, limit);
            let w = peek_word(s, q).unwrap_or_default();
            if w != "catch" && w != "finally" {
                break;
            }
            let mut q = skip_ws(s, q + w.len(), limit);
            if s.get(q) == Some(&'(') {
                q = skip_ws(s, find_matching(s, q)? + 1, limit);
            }
            if match_word(s, q, "when") {
                q = skip_ws(s, q + 4, limit);
                if s.get(q) == Some(&'(') {
                    q = skip_ws(s, find_matching(s, q)? + 1, limit);
                }
            }
            let (more, np) = self.body(q, limit)?;
            all.extend(more);
            p = np;
        }
        Ok((all, p))
    }

    /// `Type name [= value], …`: the variables go to the box, the values become assignments.
    fn declaration(&mut self, raw: &str) -> Option<Vec<Node>> {
        let raw = ["const ", "readonly ", "static "]
            .iter()
            .find_map(|m| raw.strip_prefix(m))
            .map_or(raw, str::trim_start);
        let c: Vec<char> = raw.chars().collect();
        let (typ, rest) = split_type(&c)?;
        let mut items = Vec::new();
        let mut found = Vec::new();
        for d in split_top(&rest, ',', false) {
            let d = d.trim();
            if d.is_empty() {
                continue;
            }
            let (name, expr) = d
                .split_once('=')
                .map_or((d, None), |(n, e)| (n.trim(), Some(e)));
            if peek_word(&name.chars().collect::<Vec<_>>(), 0).as_deref() != Some(name) {
                return None;
            }
            found.push(name.to_string());
            if let Some(e) = expr {
                items.push(stmt(simplify_stmt(&format!("{name} = {}", e.trim()))));
            }
        }
        if found.is_empty() {
            return None;
        }
        for n in found {
            self.add_var(&n, &typ);
        }
        Some(items)
    }

    fn parse_if(&mut self, pos: usize, limit: usize) -> Res<(Node, usize)> {
        let s = self.s;
        let (cond, p) = self.paren(pos + 2, limit, "if")?;
        let (yes, p) = self.body(p, limit)?;
        let q = skip_ws(s, p, limit);
        let (mut no, mut p) = (Vec::new(), p);
        if match_word(s, q, "else") {
            let r = skip_ws(s, q + 4, limit);
            if match_word(s, r, "if") {
                let (n, np) = self.parse_if(r, limit)?;
                no = vec![n];
                p = np;
            } else {
                let (b, np) = self.body(r, limit)?;
                no = b;
                p = np;
            }
        }
        let kind = Kind::If {
            cond: Rich::plain(&simplify_cond(&cond)),
            yes: seq(yes),
            no: seq(no),
        };
        Ok((node(kind), p))
    }

    fn parse_for(&mut self, pos: usize, limit: usize) -> Res<(Vec<Node>, usize)> {
        let (header, p) = self.paren(pos + 3, limit, "for")?;
        let mut parts: Vec<String> = split_top(&header, ';', false)
            .iter()
            .map(|p| p.trim().to_string())
            .collect();
        while parts.len() < 3 {
            parts.push(String::new());
        }
        // the loop variable is declared before the variables of the body
        let mut out = Vec::new();
        if !parts[0].is_empty() {
            match self.declaration(&parts[0]) {
                Some(items) => out.extend(items),
                None => out.push(stmt(simplify_stmt(&parts[0]))),
            }
        }
        let (mut body, p) = self.body(p, limit)?;
        if !parts[2].is_empty() {
            body.push(stmt(simplify_stmt(&parts[2])));
        }
        let cond = if parts[1].is_empty() {
            "true".to_string()
        } else {
            simplify_cond(&parts[1])
        };
        out.push(node(Kind::While {
            cond: Rich::plain(&cond),
            body: seq(body),
        }));
        Ok((out, p))
    }

    fn parse_foreach(&mut self, pos: usize, limit: usize) -> Res<(Vec<Node>, usize)> {
        let (header, p) = self.paren(pos + 7, limit, "foreach")?;
        let h: Vec<char> = header.trim().chars().collect();
        let cond = match split_foreach(&h) {
            Some((typ, name, coll)) => {
                self.add_var(&name, &typ);
                format!("{name} in {}", simplify_cond(&coll))
            }
            None => simplify_cond(&text(&h)),
        };
        let (body, p) = self.body(p, limit)?;
        let kind = Kind::While {
            cond: Rich::plain(&cond),
            body: seq(body),
        };
        Ok((vec![node(kind)], p))
    }

    fn parse_switch(&mut self, pos: usize, limit: usize) -> Res<(Vec<Node>, usize)> {
        let s = self.s;
        let (expr, p) = self.paren(pos + 6, limit, "switch")?;
        let expr = simplify_generic(expr.trim(), true);
        let p = skip_ws(s, p, limit);
        if s.get(p) != Some(&'{') {
            return Err("expected '{' after switch(...)".into());
        }
        let close = find_matching(s, p)?;
        let mut cases: Vec<Case> = Vec::new();
        // labels waiting for their body (None: `default`)
        let mut pending: Vec<Option<String>> = Vec::new();
        let mut items: Vec<Node> = Vec::new();
        let mut flush = |pending: &mut Vec<Option<String>>, items: &mut Vec<Node>| {
            if pending.is_empty() {
                return;
            }
            let mut body = std::mem::take(items);
            // the `break` that ends a case is the diagram's own end of the branch
            if matches!(body.last().map(|n| &n.kind), Some(Kind::Statement(t)) if t.text.trim() == "break")
            {
                body.pop();
            }
            let cond = if pending.iter().any(Option::is_none) {
                None
            } else {
                let labels: Vec<String> = pending
                    .iter()
                    .flatten()
                    .map(|l| format!("{expr} == {l}"))
                    .collect();
                Some(Rich::plain(&labels.join(" or ")))
            };
            pending.clear();
            cases.push(Case {
                id: super::model::fresh_id(),
                cond,
                body: seq(body),
            });
        };
        let mut q = p + 1;
        loop {
            q = skip_ws(s, q, close);
            if q >= close {
                break;
            }
            if match_word(s, q, "case") {
                if !items.is_empty() {
                    flush(&mut pending, &mut items);
                }
                let start = skip_ws(s, q + 4, close);
                let mut colon = start;
                let mut depth = 0;
                while colon < close {
                    if let Some(j) = skip_literal(s, colon) {
                        colon = j;
                        continue;
                    }
                    match s[colon] {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => depth -= 1,
                        ':' if depth == 0 => break,
                        _ => {}
                    }
                    colon += 1;
                }
                pending.push(Some(text(&s[start..colon.min(close)]).trim().to_string()));
                q = colon + 1;
                continue;
            }
            if match_word(s, q, "default") && s.get(skip_ws(s, q + 7, close)) == Some(&':') {
                if !items.is_empty() {
                    flush(&mut pending, &mut items);
                }
                pending.push(None);
                q = skip_ws(s, q + 7, close) + 1;
                continue;
            }
            let (more, np) = self.statement(q, close, false)?;
            items.extend(more);
            q = np;
        }
        flush(&mut pending, &mut items);
        // the default case is always the last one
        if let Some(i) = cases.iter().position(|c| c.cond.is_none()) {
            let d = cases.remove(i);
            cases.push(d);
        }
        Ok((vec![node(Kind::Switch { cases })], close + 1))
    }
}

/// `Type name rest` at the start of a statement: (the type, the text after the type).
fn split_type(c: &[char]) -> Option<(String, String)> {
    let n = c.len();
    let w = peek_word(c, 0)?;
    // the keyword types may be followed by `[]`-less names directly
    let mut j = w.chars().count();
    // qualified names: System.Collections.Generic.List
    while c.get(j) == Some(&'.') {
        let part = peek_word(c, j + 1)?;
        j += 1 + part.chars().count();
    }
    if c.get(j) == Some(&'<') {
        let k = find_close_angle(c, j)?;
        j = k + 1;
    }
    if c.get(j) == Some(&'?') {
        j += 1;
    }
    while c.get(j) == Some(&'[') {
        let mut k = j + 1;
        while k < n && (c[k] == ',' || c[k].is_whitespace()) {
            k += 1;
        }
        if c.get(k) != Some(&']') {
            return None;
        }
        j = k + 1;
    }
    let typ = text(&c[..j]);
    if NOT_TYPES.contains(&typ.as_str()) {
        return None;
    }
    let k = skip_ws(c, j, n);
    if k == j || k >= n {
        return None;
    }
    // `x = y`, `a < b` and `f(x)` are not declarations: a name must follow the type
    let name = peek_word(c, k)?;
    let after = skip_ws(c, k + name.chars().count(), n);
    if after < n && c[after] != '=' && c[after] != ',' {
        return None;
    }
    Some((typ, text(&c[k..])))
}

/// The `>` that closes the `<` at `j` (only type characters allowed in between).
fn find_close_angle(c: &[char], j: usize) -> Option<usize> {
    let mut depth = 0;
    for (k, ch) in c.iter().enumerate().skip(j) {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            ch if is_word(*ch) || " ,.?[]".contains(*ch) => {}
            _ => return None,
        }
    }
    None
}

/// `Type name in collection` → (type, name, collection).
fn split_foreach(h: &[char]) -> Option<(String, String, String)> {
    let n = h.len();
    for k in 1..n {
        if !h[k].is_whitespace() {
            continue;
        }
        let a = skip_ws(h, k, n);
        let Some(name) = peek_word(h, a) else {
            continue;
        };
        let b = a + name.chars().count();
        let c = skip_ws(h, b, n);
        if c == b || !match_word(h, c, "in") {
            continue;
        }
        let d = skip_ws(h, c + 2, n);
        if d == c + 2 || d >= n {
            continue;
        }
        let typ = text(&h[..k]).trim().to_string();
        if typ.is_empty() {
            continue;
        }
        return Some((typ, name, text(&h[d..]).trim().to_string()));
    }
    None
}

// ------------------------------------------------------------------ finding the methods

struct Method {
    name: String,
    params: String,
    /// Between the braces.
    body: (usize, usize),
}

const NOT_METHODS: [&str; 18] = [
    "if",
    "while",
    "for",
    "foreach",
    "switch",
    "catch",
    "using",
    "lock",
    "fixed",
    "return",
    "sizeof",
    "typeof",
    "nameof",
    "checked",
    "unchecked",
    "default",
    "new",
    "throw",
];

/// Is the `(` at `q` the end of `modifiers Type Name(`? The name, if so.
fn signature_at(s: &[char], q: usize, lo: usize) -> Option<String> {
    let mut e = q;
    while e > lo && s[e - 1].is_whitespace() {
        e -= 1;
    }
    if e > lo && s[e - 1] == '>' {
        // a generic method: Max<T>(…)
        let mut depth = 0;
        let mut k = e;
        while k > lo {
            k -= 1;
            match s[k] {
                '>' => depth += 1,
                '<' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        if depth != 0 {
            return None;
        }
        e = k;
    }
    let mut b = e;
    while b > lo && is_word(s[b - 1]) {
        b -= 1;
    }
    if b == e {
        return None;
    }
    let mut w = b;
    while w > lo && s[w - 1].is_whitespace() {
        w -= 1;
    }
    let ws = b - w;
    let type_char = |c: char| is_word(c) || c.is_whitespace() || ".<>[],".contains(c);
    if !(ws >= 2 || (ws == 1 && w > lo && type_char(s[w - 1]))) {
        return None;
    }
    let name = text(&s[b..e]);
    (!NOT_METHODS.contains(&name.as_str())).then_some(name)
}

fn find_methods(s: &[char]) -> Res<Vec<Method>> {
    let n = s.len();
    let (mut out, mut pos) = (Vec::new(), 0);
    while pos < n {
        let Some((q, name)) = (pos..n)
            .find_map(|q| (s[q] == '(').then(|| signature_at(s, q, pos).map(|nm| (q, nm)))?)
        else {
            break;
        };
        let Ok(close) = find_matching(s, q) else {
            pos = q + 1;
            continue;
        };
        let mut after = skip_ws(s, close + 1, n);
        if at(s, after, "where") {
            // a generic constraint: `where T : struct {`
            let brace = (after..n).find(|&i| s[i] == '{');
            let semi = (after..n).find(|&i| s[i] == ';');
            after = match (brace, semi) {
                (Some(b), Some(m)) if m > b => b,
                (Some(b), None) => b,
                _ => n,
            };
        }
        if after < n && s[after] == '{' {
            let end = find_matching(s, after)?;
            out.push(Method {
                name,
                params: text(&s[q + 1..close]),
                body: (after + 1, end),
            });
            pos = end + 1;
        } else {
            // an abstract signature, a property, a delegate, …
            pos = q + 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(p: &Program) -> &Diagram {
        &p.diagrams[0]
    }

    fn st(n: &Node) -> String {
        match &n.kind {
            Kind::Statement(t) => t.text.clone(),
            Kind::While { cond, .. } => format!("while {}", cond.text),
            Kind::Repeat { cond, .. } => format!("repeat {}", cond.text),
            Kind::If { cond, .. } => format!("if {}", cond.text),
            Kind::Switch { .. } => "switch".into(),
            Kind::Noop => "-".into(),
        }
    }

    #[test]
    fn demo_has_two_methods() {
        let c = from_csharp(DEMO_SOURCE).unwrap();
        assert_eq!(c.methods, ["Main", "Fibonacci"]);
        let main = first(&c.program);
        let lines: Vec<_> = main.body.iter().map(st).collect();
        assert_eq!(lines, ["out \"Enter n:\"", "inp n", "out Fibonacci(n)"]);
        assert_eq!(main.variables.text, "args : string[]\nn : int");
        let fib = &c.program.diagrams[1];
        assert_eq!(fib.ret.text, "a");
        assert!(fib.show_return && fib.show_variables);
        let lines: Vec<_> = fib.body.iter().map(st).collect();
        assert_eq!(lines, ["a = 0", "b = 1", "i = 0", "while i < n"]);
        let Kind::While { body, .. } = &fib.body[3].kind else {
            panic!()
        };
        let lines: Vec<_> = body.iter().map(st).collect();
        assert_eq!(lines, ["tmp = a + b", "a = b", "b = tmp", "i++"]);
        assert_eq!(fib.variables.text, "n, a, b, i, tmp : int");
    }

    #[test]
    fn control_flow() {
        let src = "void F(int[] a) {
            foreach (var x in a) { if (x > 1) break; else if (x < 0) continue; else x++; }
            do { n--; } while (n > 0);
            switch (k) { case 1: case 2: y = 1; break; default: y = 0; break; case 3: y = 3; break; }
            try { a[0] = 1; } catch (Exception e) when (e != null) { b = 2; } finally { c = 3; }
        }";
        let c = from_csharp(src).unwrap();
        let d = first(&c.program);
        let top: Vec<_> = d.body.iter().map(st).collect();
        assert_eq!(top[0], "while x in a");
        assert_eq!(top[1], "repeat n > 0");
        assert_eq!(top[2], "switch");
        assert_eq!(&top[3..], ["a[0] = 1", "b = 2", "c = 3"]);
        let Kind::Switch { cases } = &d.body[2].kind else {
            panic!()
        };
        assert_eq!(cases[0].cond.as_ref().unwrap().text, "k == 1 or k == 2");
        assert_eq!(cases[1].cond.as_ref().unwrap().text, "k == 3");
        assert!(cases[2].cond.is_none());
        assert_eq!(st(&cases[0].body[0]), "y = 1");
        assert!(d.variables.text.contains("x : var"));
    }

    #[test]
    fn simplifications() {
        assert_eq!(simplify_stmt("x = int.Parse(Console.ReadLine())"), "inp x");
        assert_eq!(
            simplify_stmt("Console.WriteLine(\"a\" + n)"),
            "out \"a\" + n"
        );
        assert_eq!(simplify_stmt("Console.Error.WriteLine(e)"), "STDERR.out e");
        assert_eq!(simplify_stmt("Console.ReadKey()"), "inp");
        assert_eq!(
            simplify_stmt("ok = int.TryParse(s, out v)"),
            "ok = TryParse(s, v)"
        );
        assert_eq!(simplify_cond("Convert.ToInt32(x) > 3"), "x > 3");
        assert_eq!(simplify_stmt("t = Console.In"), "t = STDIN");
    }

    #[test]
    fn robust_input() {
        // strings with quotes, comments, generics, multi-line declarations
        let src = "static T Max<T>(T a, Dictionary<int, string> d) where T : struct {
            string s = \"a\\\"//b\"; // comment
            /* block */ int[,] m = new int[2,2];
            var list = new List<int> {
               1, 2 };
            return s;
        }";
        let c = from_csharp(src).unwrap();
        let d = first(&c.program);
        assert_eq!(c.methods, ["Max"]);
        assert_eq!(d.ret.text, "s");
        assert!(d.variables.text.contains("d : Dictionary<int, string>"));
        assert!(d.body.iter().any(|n| st(n) == "s = \"a\\\"//b\""));
        assert!(
            d.body
                .iter()
                .any(|n| st(n) == "list = new List<int> { 1, 2 }")
        );
        assert!(from_csharp("class A { int x; }").is_err());
        assert!(from_csharp("if (x) { y(); }").is_ok());
        assert_eq!(from_csharp("x = 1;").unwrap().methods, ["Main"]);
    }

    #[test]
    fn survives_the_web_round_trip() {
        let c = from_csharp(DEMO_SOURCE).unwrap();
        let json = super::super::format::to_json(&c.program);
        let back = super::super::format::from_json(&json).unwrap();
        assert_eq!(back.diagrams.len(), 2);
        assert_eq!(back.diagrams[1].name.text, "Fibonacci");
    }
}
