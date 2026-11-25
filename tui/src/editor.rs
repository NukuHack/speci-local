//! A text editor pane: vim keys (the window app's engine), soft wrapping, line numbers,
//! syntax highlighting, error lines, `/` search, digraphs and word completion.

use crate::ascii;
use crate::highlight::{self, HEADERS, KEYWORDS, Tok};
use crate::theme::Theme;
use crate::vim::{self, Vim};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use speci_local::parser::MACROS;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Spec,
    Data,
    /// The C# program of the Code tool.
    Csharp,
    /// No highlighting: the console input of the Code tool.
    Plain,
}

/// What the app should do after a key the editor doesn't handle itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    Command,
    Search,
}

#[derive(Default)]
pub struct Out {
    pub changed: bool,
    pub req: Option<Req>,
}

struct Comp {
    start: usize,
    orig: Vec<char>,
    cands: Vec<String>,
    i: usize,
}

struct Row {
    line: usize,
    a: usize,
    b: usize,
}

pub struct Editor {
    pub t: Vec<char>,
    pub p: usize,
    pub lang: Lang,
    pub id: u64,
    pub version: u64,
    top: usize,
    hl: Option<(u64, Vec<Tok>)>,
    /// 1-based lines to tint as errors.
    pub errors: Vec<usize>,
    pub search: Option<Vec<char>>,
    digraph: Option<Vec<char>>,
    comp: Option<Comp>,
    view_h: usize,
    pub cursor_xy: Option<(u16, u16)>,
    pub hint: String,
}

/// Columns a character takes on the screen (more than one for symbols in ASCII mode).
fn cw(c: char) -> usize {
    if c == '\t' {
        2
    } else if let Some(a) = ascii::expand(c) {
        a.len()
    } else {
        c.width().unwrap_or(1).max(1)
    }
}

/// The symbol a backslash name stands for (`\in` is `∈`).
pub fn alias(name: &str) -> Option<char> {
    Some(match name {
        "in" => '∈',
        "all" | "forall" => '∀',
        "ex" | "exists" => '∃',
        "ne" | "neq" => '≠',
        "le" | "leq" => '≤',
        "ge" | "geq" => '≥',
        "not" | "neg" => '¬',
        "nd" | "ndiv" => '∤',
        _ => return None,
    })
}

/// Turn a `\name` that ends at `*p` into its symbol. True when it did.
pub fn expand_alias(t: &mut Vec<char>, p: &mut usize) -> bool {
    let end = (*p).min(t.len());
    let mut start = end;
    while start > 0 && t[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    if start == 0 || start == end || t[start - 1] != '\\' {
        return false;
    }
    let name: String = t[start..end].iter().collect();
    let Some(symbol) = alias(&name) else {
        return false;
    };
    t.splice(start - 1..end, [symbol]);
    *p = start;
    true
}

pub fn digraph(a: char, b: char) -> Option<char> {
    const T: &[(&str, char)] = &[
        ("(-", '∈'),
        ("FA", '∀'),
        ("TE", '∃'),
        ("!=", '≠'),
        ("=<", '≤'),
        (">=", '≥'),
        ("NO", '¬'),
        ("DI", '∤'),
        ("->", '→'),
    ];
    T.iter().find_map(|(k, c)| {
        let mut it = k.chars();
        let (x, y) = (it.next()?, it.next()?);
        ((x, y) == (a, b) || (x, y) == (b, a)).then_some(*c)
    })
}

impl Editor {
    pub fn new(lang: Lang, id: u64, text: &str) -> Self {
        Self {
            t: text.chars().collect(),
            p: 0,
            lang,
            id,
            version: 1,
            top: 0,
            hl: None,
            errors: vec![],
            search: None,
            digraph: None,
            comp: None,
            view_h: 10,
            cursor_xy: None,
            hint: String::new(),
        }
    }

    pub fn text(&self) -> String {
        self.t.iter().collect()
    }

    pub fn set_text(&mut self, s: &str) {
        self.t = s.chars().collect();
        self.p = self.p.min(self.t.len());
        self.version += 1;
        self.comp = None;
    }

    // ------------------------------------------------------------ positions

    fn ls(&self, p: usize) -> usize {
        let p = p.min(self.t.len());
        self.t[..p]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }
    fn le(&self, p: usize) -> usize {
        let p = p.min(self.t.len());
        self.t[p..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.t.len(), |i| p + i)
    }
    pub fn line_col(&self) -> (usize, usize) {
        let s = self.ls(self.p);
        (
            self.t[..s].iter().filter(|&&c| c == '\n').count() + 1,
            self.p - s + 1,
        )
    }
    /// The text of the line the cursor is on.
    pub fn current_line(&self) -> String {
        self.t[self.ls(self.p)..self.le(self.p)].iter().collect()
    }
    pub fn n_lines(&self) -> usize {
        self.t.iter().filter(|&&c| c == '\n').count() + 1
    }

    /// Put the cursor on (line, column), both 1-based.
    pub fn goto(&mut self, line: usize, col: usize) {
        let mut s = 0;
        for _ in 1..line.max(1) {
            match self.t[s..].iter().position(|&c| c == '\n') {
                Some(i) => s += i + 1,
                None => break,
            }
        }
        let e = self.le(s);
        self.p = (s + col.max(1) - 1).min(e);
    }

    pub fn move_lines(&mut self, n: isize) {
        let s = self.ls(self.p);
        let col = self.p - s;
        let mut start = s;
        if n < 0 {
            for _ in 0..(-n) {
                if start == 0 {
                    break;
                }
                start = self.ls(start - 1);
            }
        } else {
            for _ in 0..n {
                let e = self.le(start);
                if e >= self.t.len() {
                    break;
                }
                start = e + 1;
            }
        }
        let e = self.le(start);
        self.p = (start + col).min(e);
    }

    // ------------------------------------------------------------ editing

    const fn bump(&mut self) {
        self.version += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        for c in s.chars() {
            self.t.insert(self.p, c);
            self.p += 1;
        }
        self.bump();
    }

    /// Paste outside Insert mode: one undoable insertion.
    pub fn paste(&mut self, vim: &mut Vim, s: &str) {
        if vim.in_insert() {
            self.insert_str(s);
            return;
        }
        vim.feed(self.id, &mut self.t, &mut self.p, 'i');
        self.insert_str(s);
        vim.escape(self.id, &mut self.t, &mut self.p);
    }

    /// `\name` before the cursor becomes its symbol (typed in the specification only).
    fn alias_here(&mut self) {
        if self.lang == Lang::Spec && expand_alias(&mut self.t, &mut self.p) {
            self.bump();
        }
    }

    fn ins(&mut self, c: char) {
        if !c.is_ascii_alphanumeric() && c != '\\' {
            self.alias_here();
        }
        self.t.insert(self.p, c);
        self.p += 1;
        if self.lang == Lang::Spec && c == '=' && self.p >= 2 {
            let m = match self.t[self.p - 2] {
                '<' => Some('≤'),
                '>' => Some('≥'),
                '!' => Some('≠'),
                _ => None,
            };
            if let Some(m) = m {
                self.t[self.p - 2] = m;
                self.t.remove(self.p - 1);
                self.p -= 1;
            }
        }
        self.bump();
    }

    fn newline(&mut self) {
        self.alias_here();
        let s = self.ls(self.p);
        let mut indent: Vec<char> = self.t[s..self.p]
            .iter()
            .take_while(|c| **c == ' ' || **c == '\t')
            .copied()
            .collect();
        let before = self.p.checked_sub(1).map(|i| self.t[i]);
        let after = self.t.get(self.p).copied();
        // one level deeper after `[`/`{` (data) or `{` (C#); the closer goes on a line of its own
        let (opened, closer, unit) = match self.lang {
            Lang::Data => (
                matches!(before, Some('[' | '{')),
                matches!(after, Some(']' | '}')),
                2,
            ),
            Lang::Csharp => (
                self.t[s..self.p].iter().rev().find(|c| !c.is_whitespace()) == Some(&'{'),
                after == Some('}'),
                4,
            ),
            _ => (false, false, 2),
        };
        if opened {
            indent.extend(std::iter::repeat_n(' ', unit));
        }
        let split = opened && closer;
        let mut ins = vec!['\n'];
        ins.extend(&indent);
        let at = self.p;
        let n = ins.len();
        self.t.splice(at..at, ins);
        self.p = at + n;
        if split {
            let base = indent.len().saturating_sub(unit);
            let mut more = vec!['\n'];
            more.extend(std::iter::repeat_n(' ', base));
            let at = self.p;
            self.t.splice(at..at, more);
        }
        self.bump();
    }

    fn delete_range(&mut self, a: usize, b: usize) {
        if a < b {
            self.t.drain(a..b);
            self.p = a;
            self.bump();
        }
    }

    fn insert_key(&mut self, vim: &mut Vim, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let before = self.version;
        let completing = matches!(k.code, KeyCode::Tab | KeyCode::BackTab)
            || ctrl && matches!(k.code, KeyCode::Char('n' | 'p'));
        if !completing {
            self.comp = None;
        }
        match k.code {
            KeyCode::Esc => {
                self.alias_here();
                self.digraph = None;
                vim.escape(self.id, &mut self.t, &mut self.p);
            }
            KeyCode::Char('[') if ctrl => {
                vim.escape(self.id, &mut self.t, &mut self.p);
            }
            KeyCode::Enter => self.newline(),
            KeyCode::Backspace | KeyCode::Char('h') if k.code == KeyCode::Backspace || ctrl => {
                if self.p > 0 {
                    self.delete_range(self.p - 1, self.p);
                }
            }
            KeyCode::Delete => {
                if self.p < self.t.len() {
                    self.delete_range(self.p, self.p + 1);
                }
            }
            KeyCode::Char('w') if ctrl => {
                let mut a = self.p;
                while a > 0 && self.t[a - 1].is_whitespace() && self.t[a - 1] != '\n' {
                    a -= 1;
                }
                while a > 0 && (self.t[a - 1].is_alphanumeric() || self.t[a - 1] == '_') {
                    a -= 1;
                }
                if a == self.p && a > 0 {
                    a -= 1;
                }
                self.delete_range(a, self.p);
            }
            KeyCode::Char('u') if ctrl => {
                let s = self.ls(self.p);
                self.delete_range(if s == self.p && s > 0 { s - 1 } else { s }, self.p);
            }
            KeyCode::Char('k') if ctrl => self.digraph = Some(vec![]),
            KeyCode::Char('n') if ctrl => self.complete(true),
            KeyCode::Char('p') if ctrl => self.complete(false),
            KeyCode::Tab => {
                if self.comp.is_some() {
                    self.complete(true);
                } else if !(self.lang == Lang::Spec && expand_alias(&mut self.t, &mut self.p)) {
                    self.insert_str(if self.lang == Lang::Csharp {
                        "    "
                    } else {
                        "  "
                    });
                } else {
                    self.bump();
                }
            }
            KeyCode::BackTab => {
                if self.comp.is_some() {
                    self.complete(false);
                }
            }
            KeyCode::Left => self.p = self.p.saturating_sub(1),
            KeyCode::Right => self.p = (self.p + 1).min(self.t.len()),
            KeyCode::Up => self.move_lines(-1),
            KeyCode::Down => self.move_lines(1),
            KeyCode::Home => self.p = self.ls(self.p),
            KeyCode::End => self.p = self.le(self.p),
            KeyCode::Char(c) if alt && ('1'..='6').contains(&c) => {
                let g = ['∈', '∀', '∃', '≠', '≤', '≥'][c as usize - '1' as usize];
                self.ins(g);
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(mut d) = self.digraph.take() {
                    d.push(c);
                    if d.len() == 2 {
                        self.ins(digraph(d[0], d[1]).unwrap_or(d[1]));
                    } else {
                        self.digraph = Some(d);
                    }
                } else {
                    self.ins(c);
                }
            }
            _ => {}
        }
        self.version != before
    }

    // ------------------------------------------------------------ completion

    fn candidates(&self, prefix: &str, line_start: bool) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut add = |s: String| {
            if !out.contains(&s) {
                out.push(s);
            }
        };
        let lower = prefix.to_lowercase();
        let hit = |w: &str| w.to_lowercase().starts_with(&lower) && w.to_lowercase() != lower;
        if self.lang == Lang::Spec {
            if line_start {
                for h in HEADERS {
                    if hit(h) {
                        add(format!("{h}:"));
                    }
                }
            }
            for (w, sym) in [
                ("forall", "∀"),
                ("exists", "∃"),
                ("leq", "≤"),
                ("geq", "≥"),
                ("neq", "≠"),
                ("elem", "∈"),
            ] {
                if hit(w) || w == lower {
                    add(sym.into());
                }
            }
            for m in MACROS {
                if hit(m.0) {
                    add(format!("{}(", m.0));
                }
            }
            for k in KEYWORDS {
                if hit(k) {
                    add((*k).into());
                }
            }
        }
        // words already in the text
        let text: String = self.t.iter().collect();
        for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if w.chars().count() > 1 && hit(w) {
                add(w.into());
            }
        }
        out
    }

    fn complete(&mut self, fwd: bool) {
        if self.comp.is_none() {
            let mut s = self.p;
            while s > 0 && (self.t[s - 1].is_alphanumeric() || self.t[s - 1] == '_') {
                s -= 1;
            }
            let prefix: String = self.t[s..self.p].iter().collect();
            if prefix.is_empty() {
                return;
            }
            let line_start = self.t[self.ls(s)..s].iter().all(|c| c.is_whitespace());
            let cands = self.candidates(&prefix, line_start);
            if cands.is_empty() {
                self.hint = "no completion".into();
                return;
            }
            self.comp = Some(Comp {
                start: s,
                orig: prefix.chars().collect(),
                cands,
                i: usize::MAX,
            });
        }
        let Some(c) = self.comp.as_mut() else { return };
        let n = c.cands.len() + 1; // the last slot is the text as typed
        c.i = if c.i == usize::MAX {
            if fwd { 0 } else { n - 2 }
        } else if fwd {
            (c.i + 1) % n
        } else {
            (c.i + n - 1) % n
        };
        let word: Vec<char> = if c.i < c.cands.len() {
            c.cands[c.i].chars().collect()
        } else {
            c.orig.clone()
        };
        self.hint = if c.i < c.cands.len() {
            format!("completion {}/{}", c.i + 1, c.cands.len())
        } else {
            String::new()
        };
        let (s, e) = (c.start, self.p);
        self.t.splice(s..e, word.iter().copied());
        self.p = s + word.len();
        self.bump();
    }

    // ------------------------------------------------------------ keys

    pub fn key(&mut self, vim: &mut Vim, k: KeyEvent) -> Out {
        let before = self.version;
        let mut out = Out::default();
        if vim.id_is(self.id) && vim.in_insert() {
            self.insert_key(vim, k);
            out.changed = self.version != before;
            return out;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let idle = !vim.pending();
        if idle {
            match (k.code, ctrl) {
                (KeyCode::Char('d'), true) | (KeyCode::PageDown, _) => {
                    self.move_lines((self.view_h / 2).max(1) as isize);
                    return out;
                }
                (KeyCode::Char('u'), true) | (KeyCode::PageUp, _) => {
                    self.move_lines(-((self.view_h / 2).max(1) as isize));
                    return out;
                }
                (KeyCode::Char('f'), true) => {
                    self.move_lines(self.view_h.saturating_sub(2).max(1) as isize);
                    return out;
                }
                (KeyCode::Char('b'), true) => {
                    self.move_lines(-(self.view_h.saturating_sub(2).max(1) as isize));
                    return out;
                }
                (KeyCode::Char('e'), true) => {
                    self.top += 1;
                    return out;
                }
                (KeyCode::Char('y'), true) => {
                    self.top = self.top.saturating_sub(1);
                    return out;
                }
                (KeyCode::Char(':'), false) if !vim.is_visual() => {
                    out.req = Some(Req::Command);
                    return out;
                }
                (KeyCode::Char('/'), false) if !vim.is_visual() => {
                    out.req = Some(Req::Search);
                    return out;
                }
                (KeyCode::Char(c @ ('n' | 'N')), false) if self.search.is_some() => {
                    self.find_next(c == 'n');
                    return out;
                }
                (KeyCode::Esc, _) => {
                    self.search = None;
                }
                (KeyCode::Char('*' | '#'), false) => self.search = None,
                _ => {}
            }
        }
        let ch = match (k.code, ctrl) {
            (KeyCode::Char('r'), true) => Some(vim::CTRL_R),
            (KeyCode::Char('['), true) | (KeyCode::Esc, _) => Some(vim::ESC),
            (KeyCode::Char('z'), true) => Some('u'),
            (KeyCode::Char(c), false) => Some(c),
            (KeyCode::Enter, _) => Some(vim::ENTER),
            (KeyCode::Backspace, _) => Some(vim::BACK),
            (KeyCode::Left, _) => Some('h'),
            (KeyCode::Right, _) => Some('l'),
            (KeyCode::Up, _) => Some('k'),
            (KeyCode::Down, _) => Some('j'),
            (KeyCode::Home, _) => Some('0'),
            (KeyCode::End, _) => Some('$'),
            (KeyCode::Delete, _) => Some('x'),
            _ => None,
        };
        if let Some(c) = ch
            && vim.feed(self.id, &mut self.t, &mut self.p, c)
        {
            self.bump();
        }
        out.changed = self.version != before;
        out
    }

    pub fn find_next(&mut self, fwd: bool) {
        let Some(pat) = self.search.clone() else {
            return;
        };
        if pat.is_empty() || self.t.len() < pat.len() {
            return;
        }
        let hits: Vec<usize> = (0..=self.t.len() - pat.len())
            .filter(|&i| self.t[i..].starts_with(&pat))
            .collect();
        let q = if fwd {
            hits.iter().find(|&&h| h > self.p).or_else(|| hits.first())
        } else {
            hits.iter()
                .rev()
                .find(|&&h| h < self.p)
                .or_else(|| hits.last())
        };
        match q {
            Some(&q) => self.p = q,
            None => self.hint = "pattern not found".into(),
        }
    }

    pub fn set_search(&mut self, pat: &str, fwd: bool) {
        let pat: Vec<char> = pat.chars().collect();
        if fwd {
            // a match under the cursor counts
            self.p = self.p.saturating_sub(1);
            if self.p == 0 && self.t.starts_with(&pat) {
                self.search = Some(pat);
                return;
            }
        }
        self.search = Some(pat);
        self.find_next(fwd);
    }

    // ------------------------------------------------------------ drawing

    fn rows(&self, w: usize) -> Vec<Row> {
        let w = w.max(2);
        let mut rows = vec![];
        let mut s = 0;
        let mut line = 0;
        loop {
            let e = self.le(s);
            let (mut a, mut acc) = (s, 0);
            for (i, &ch) in self.t.iter().enumerate().take(e).skip(s) {
                let c = cw(ch);
                if acc + c > w && i > a {
                    rows.push(Row { line, a, b: i });
                    a = i;
                    acc = 0;
                }
                acc += c;
            }
            rows.push(Row { line, a, b: e });
            if e >= self.t.len() {
                break;
            }
            s = e + 1;
            line += 1;
        }
        rows
    }

    pub fn tokens(&mut self) -> &[Tok] {
        if self.hl.as_ref().is_none_or(|(v, _)| *v != self.version) {
            let toks = match self.lang {
                Lang::Spec => highlight::spec(&self.t),
                Lang::Data => highlight::data(&self.t),
                Lang::Csharp => highlight::csharp(&self.t),
                Lang::Plain => vec![Tok::Text; self.t.len()],
            };
            self.hl = Some((self.version, toks));
        }
        self.hl
            .as_ref()
            .map_or(&[][..], |(_, toks)| toks.as_slice())
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, th: &Theme, vim: &Vim, focused: bool) {
        if area.width < 6 || area.height == 0 {
            return;
        }
        let digits = self.n_lines().to_string().len().max(2);
        let gutter = digits + 2;
        let tw = (area.width as usize).saturating_sub(gutter + 1).max(2);
        self.view_h = area.height as usize;
        self.p = self.p.min(self.t.len());
        let rows = self.rows(tw);
        let cur = rows
            .iter()
            .rposition(|r| r.a <= self.p && (r.line == self.line_col().0 - 1))
            .unwrap_or(0);
        let h = area.height as usize;
        let so = 2.min(h.saturating_sub(1) / 2);
        if cur < self.top + so {
            self.top = cur.saturating_sub(so);
        }
        if cur + so >= self.top + h {
            self.top = cur + so + 1 - h;
        }
        self.top = self.top.min(rows.len().saturating_sub(1));
        let sel = if focused && vim.in_visual_of(self.id) {
            vim.visual_range(&self.t, self.p)
        } else {
            None
        };
        let search = self.search.clone();
        let hits: Vec<usize> = match &search {
            Some(p) if !p.is_empty() && self.t.len() >= p.len() => (0..=self.t.len() - p.len())
                .filter(|&i| self.t[i..].starts_with(p))
                .collect(),
            _ => vec![],
        };
        let plen = search.as_ref().map_or(0, Vec::len);
        let toks: Vec<Tok> = self.tokens().to_vec();
        let (cur_line, _) = self.line_col();
        self.cursor_xy = None;
        for (ri, row) in rows.iter().enumerate().skip(self.top).take(h) {
            let y = area.y + (ri - self.top) as u16;
            let first = ri == 0 || rows[ri - 1].line != row.line;
            let err = self.errors.contains(&(row.line + 1));
            let num = if first {
                format!("{:>w$} ", row.line + 1, w = digits)
            } else {
                " ".repeat(digits + 1)
            };
            let gs = if row.line + 1 == cur_line && focused {
                Style::default().fg(th.fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.gutter)
            };
            buf.set_string(area.x, y, &num, gs);
            if err && first {
                buf.set_string(
                    area.x + digits as u16 + 1,
                    y,
                    "▌",
                    Style::default().fg(th.err),
                );
            }
            let mut x = area.x + gutter as u16;
            let line_bg = if err { Some(th.err_bg) } else { None };
            for (i, &c) in self.t.iter().enumerate().take(row.b).skip(row.a) {
                let mut st = toks.get(i).copied().unwrap_or(Tok::Text).style(th);
                if let Some(bg) = line_bg {
                    st = st.bg(bg);
                }
                if hits.iter().any(|&h| i >= h && i < h + plen) {
                    st = st.bg(th.match_bg);
                }
                if sel.is_some_and(|(s, e)| i >= s && i < e) {
                    st = st.bg(th.sel_bg);
                }
                let s: String = if c == '\t' {
                    "  ".into()
                } else {
                    ascii::expand(c).map_or_else(|| c.to_string(), str::to_string)
                };
                let w = cw(c) as u16;
                if x + w > area.x + area.width {
                    break;
                }
                buf.set_string(x, y, &s, st);
                x += w;
            }
            // a selected line break shows as one selected blank
            if let Some((s, e)) = sel
                && row.b >= s
                && row.b < e
                && x < area.x + area.width
                && row.b == self.le(row.b)
            {
                buf[(x, y)].set_bg(th.sel_bg);
            }
            if ri == cur {
                let off: usize = self.t[row.a..self.p.min(row.b).max(row.a)]
                    .iter()
                    .map(|&c| cw(c))
                    .sum();
                let cx =
                    (area.x as usize + gutter + off).min((area.x + area.width - 1) as usize) as u16;
                self.cursor_xy = Some((cx, y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn typed(e: &mut Editor, v: &mut Vim, keys: &str) {
        for c in keys.chars() {
            let ev = match c {
                '⎋' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '⏎' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                c => k(c),
            };
            e.key(v, ev);
        }
    }

    #[test]
    fn vim_commands_edit_the_text() {
        let mut e = Editor::new(Lang::Plain, 1, "one two\nthree\n");
        let mut v = Vim::default();
        typed(&mut e, &mut v, "dwjAx y⎋");
        assert_eq!(e.text(), "two\nthreex y\n");
        typed(&mut e, &mut v, "u");
        assert_eq!(e.text(), "two\nthree\n");
        typed(&mut e, &mut v, "ggyyp");
        assert_eq!(e.text(), "two\ntwo\nthree\n");
    }

    #[test]
    fn typing_converts_comparison_symbols() {
        let mut e = Editor::new(Lang::Spec, 1, "");
        let mut v = Vim::default();
        typed(&mut e, &mut v, "ia<=b>=c!=d⎋");
        assert_eq!(e.text(), "a≤b≥c≠d");
    }

    #[test]
    fn digraphs_and_enter_indent() {
        let mut e = Editor::new(Lang::Data, 1, "");
        let mut v = Vim::default();
        typed(&mut e, &mut v, "ia: [⏎1⎋");
        assert_eq!(e.text(), "a: [\n  1");
        e.key(
            &mut v,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        e.key(
            &mut v,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        );
        typed(&mut e, &mut v, "FA");
        assert!(e.text().contains('∀'));
    }

    #[test]
    fn completion_cycles_and_restores() {
        let mut e = Editor::new(Lang::Spec, 1, "Be:\nszamok∈N\nsz");
        let mut v = Vim::default();
        e.p = e.t.len();
        typed(&mut e, &mut v, "a");
        e.key(
            &mut v,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        );
        assert!(e.text().ends_with("szamok"), "{}", e.text());
        e.key(
            &mut v,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        );
        assert!(e.text().ends_with("\nsz"), "{}", e.text());
    }

    #[test]
    fn goto_and_search() {
        let mut e = Editor::new(Lang::Plain, 1, "ab\ncd\nab\n");
        e.goto(2, 2);
        assert_eq!(e.line_col(), (2, 2));
        e.set_search("ab", true);
        assert_eq!(e.line_col(), (3, 1));
        e.find_next(true);
        assert_eq!(e.line_col(), (1, 1));
    }
}
