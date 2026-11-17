//! Drawing structograms (Nassi–Shneiderman diagrams) as text.
//!
//! Lines are drawn by setting direction bits on cells and turned into box-drawing characters
//! at the end, so every junction (`├ ┬ ┼ …`) comes out right however the blocks nest.

use crate::ascii;
use speci_local::stuki::model::{Case, Kind, Node, Program};
use unicode_width::UnicodeWidthChar;

/// Something the cursor can be on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Name(u64),
    Vars(u64),
    Ret(u64),
    Node(u64),
    Case(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tag {
    #[default]
    Border,
    Text,
    Cond,
    Label,
    Dim,
    Title,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ch: char,
    pub tag: Tag,
    mask: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            tag: Tag::Border,
            mask: 0,
        }
    }
}

const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

const fn glyph(m: u8) -> char {
    match m {
        0 => ' ',
        m if m == U || m == D || m == U | D => '│',
        m if m == L || m == R || m == L | R => '─',
        m if m == D | R => '┌',
        m if m == D | L => '┐',
        m if m == U | R => '└',
        m if m == U | L => '┘',
        m if m == U | D | R => '├',
        m if m == U | D | L => '┤',
        m if m == D | L | R => '┬',
        m if m == U | L | R => '┴',
        _ => '┼',
    }
}

/// The same lines in plain ASCII.
const fn glyph_ascii(m: u8) -> char {
    match glyph(m) {
        '│' => '|',
        '─' => '-',
        ' ' => ' ',
        _ => '+',
    }
}

impl Cell {
    pub fn shown(&self) -> char {
        if self.ch != ' ' || self.tag != Tag::Border {
            self.ch
        } else if ascii::on() {
            glyph_ascii(self.mask)
        } else {
            glyph(self.mask)
        }
    }
}

/// Words for the branch labels (they follow the language of the specification).
#[derive(Clone, Copy)]
pub struct Words {
    pub yes: &'static str,
    pub no: &'static str,
    pub other: &'static str,
}

pub const HU: Words = Words {
    yes: "igen",
    no: "nem",
    other: "egyébként",
};
pub const EN: Words = Words {
    yes: "yes",
    no: "no",
    other: "otherwise",
};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct Layout {
    pub w: usize,
    pub cells: Vec<Vec<Cell>>,
    /// The highlighted part of each focusable thing, in document order.
    pub rects: Vec<(Focus, Rect)>,
    pub failing: Option<Rect>,
}

impl Layout {
    pub const fn height(&self) -> usize {
        self.cells.len()
    }
    pub fn rect(&self, f: Focus) -> Option<Rect> {
        self.rects.iter().find(|(g, _)| *g == f).map(|(_, r)| *r)
    }
    pub fn order(&self) -> Vec<Focus> {
        self.rects.iter().map(|(f, _)| *f).collect()
    }
}

fn cw(c: char) -> usize {
    c.width().unwrap_or(1).max(1)
}

pub fn width_of(s: &str) -> usize {
    ascii::text(s).chars().map(cw).sum()
}

/// Word wrap to `w` columns; always at least one line.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let text = ascii::text(text);
    let w = w.max(1);
    let mut lines = vec![];
    for para in text.split('\n') {
        let mut cur = String::new();
        let mut cur_w = 0;
        for word in para.split(' ').filter(|x| !x.is_empty()) {
            let ww = width_of(word);
            if cur_w > 0 && cur_w + 1 + ww <= w {
                cur.push(' ');
                cur.push_str(word);
                cur_w += 1 + ww;
                continue;
            }
            if cur_w > 0 {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            if ww <= w {
                cur.push_str(word);
                cur_w = ww;
            } else {
                for c in word.chars() {
                    if cur_w + cw(c) > w {
                        lines.push(std::mem::take(&mut cur));
                        cur_w = 0;
                    }
                    cur.push(c);
                    cur_w += cw(c);
                }
            }
        }
        lines.push(cur);
    }
    lines
}

// ---------------------------------------------------------------- sizes

const LEAF_MIN: usize = 8;

fn text_of(n: &Node) -> &str {
    match &n.kind {
        Kind::Noop => "-",
        Kind::Statement(t) => &t.text,
        Kind::If { cond, .. } | Kind::While { cond, .. } | Kind::Repeat { cond, .. } => &cond.text,
        Kind::Switch { .. } => "",
    }
}

fn longest(s: &str) -> usize {
    s.split('\n').map(width_of).max().unwrap_or(0)
}

fn seq_nat(nodes: &[Node]) -> usize {
    nodes.iter().map(nat).max().unwrap_or(LEAF_MIN)
}

fn seq_min(nodes: &[Node]) -> usize {
    nodes.iter().map(min_w).max().unwrap_or(LEAF_MIN)
}

fn case_label<'a>(c: &'a Case, words: &'a Words) -> &'a str {
    c.cond.as_ref().map_or(words.other, |r| r.text.as_str())
}

/// The width a node would like.
fn nat(n: &Node) -> usize {
    let own = longest(text_of(n)) + 4;
    match &n.kind {
        Kind::Noop | Kind::Statement(_) => own.max(LEAF_MIN),
        Kind::If { yes, no, .. } => own.max(seq_nat(yes) + seq_nat(no) - 1),
        Kind::While { body, .. } | Kind::Repeat { body, .. } => own.max(seq_nat(body) + 2),
        Kind::Switch { cases } => {
            let s: usize = cases
                .iter()
                .map(|c| {
                    seq_nat(&c.body)
                        .max(longest(c.cond.as_ref().map_or("egyébként", |r| &r.text)) + 4)
                })
                .sum();
            s + 1 - cases.len().min(s + 1)
        }
    }
}

/// The least width a node can be drawn in.
fn min_w(n: &Node) -> usize {
    match &n.kind {
        Kind::Noop | Kind::Statement(_) => LEAF_MIN,
        Kind::If { yes, no, .. } => seq_min(yes) + seq_min(no) - 1,
        Kind::While { body, .. } | Kind::Repeat { body, .. } => (seq_min(body) + 2).max(LEAF_MIN),
        Kind::Switch { cases } => {
            cases.iter().map(|c| seq_min(&c.body)).sum::<usize>() + 1 - cases.len().min(1)
        }
    }
}

/// Column widths (neighbouring columns share a border) adding up to `total`.
fn split(total: usize, nats: &[usize], mins: &[usize]) -> Vec<usize> {
    let n = nats.len();
    let budget = total + n - 1;
    let mut w = mins.to_vec();
    let used: usize = w.iter().sum();
    if used >= budget {
        return w;
    }
    let extra = budget - used;
    let sum: usize = nats.iter().sum::<usize>().max(1);
    let mut given = 0;
    for i in 0..n {
        let add = extra * nats[i] / sum;
        w[i] += add;
        given += add;
    }
    let mut i = 0;
    while given < extra {
        w[i % n] += 1;
        given += 1;
        i += 1;
    }
    w
}

// ---------------------------------------------------------------- painting

struct P<'a> {
    cells: Vec<Vec<Cell>>,
    rects: Vec<(Focus, Rect)>,
    w: usize,
    words: &'a Words,
    failing_node: Option<u64>,
    failing: Option<Rect>,
}

impl P<'_> {
    fn at(&mut self, x: usize, y: usize) -> &mut Cell {
        while self.cells.len() <= y {
            self.cells.push(vec![Cell::default(); self.w]);
        }
        let w = self.w;
        &mut self.cells[y][x.min(w - 1)]
    }

    fn hline(&mut self, x0: usize, x1: usize, y: usize) {
        for x in x0..=x1 {
            self.at(x, y).mask |= if x > x0 { L } else { 0 } | if x < x1 { R } else { 0 };
        }
    }

    fn vline(&mut self, x: usize, y0: usize, y1: usize) {
        for y in y0..=y1 {
            self.at(x, y).mask |= if y > y0 { U } else { 0 } | if y < y1 { D } else { 0 };
        }
    }

    fn text(&mut self, x: usize, y: usize, s: &str, tag: Tag) {
        let mut x = x;
        for c in ascii::text(s).chars() {
            if x >= self.w {
                break;
            }
            let c = if c.is_control() { ' ' } else { c };
            let cell = self.at(x, y);
            cell.ch = c;
            cell.tag = tag;
            x += 1;
            if cw(c) == 2 && x < self.w {
                let cell = self.at(x, y);
                cell.ch = '\0';
                cell.tag = tag;
                x += 1;
            }
        }
    }

    fn lines(&mut self, x: usize, y: usize, ls: &[String], tag: Tag) {
        for (i, l) in ls.iter().enumerate() {
            self.text(x, y + i, l, tag);
        }
    }

    fn mark(&mut self, f: Focus, r: Rect, node: Option<u64>) {
        if node.is_some() && node == self.failing_node {
            self.failing = Some(r);
        }
        self.rects.push((f, r));
    }

    fn seq(&mut self, nodes: &[Node], x: usize, y: usize, w: usize) -> usize {
        let mut h = 0;
        for n in nodes {
            h += self.node(n, x, y + h, w);
        }
        h
    }

    /// Paint one node whose top edge is row `y`, spanning columns `x..x+w`. Returns its height
    /// (the row below belongs to what follows).
    fn node(&mut self, n: &Node, x: usize, y: usize, w: usize) -> usize {
        let xr = x + w - 1;
        let id = n.id;
        match &n.kind {
            Kind::Noop => {
                self.hline(x, xr, y);
                self.text(x + 2, y + 1, "-", Tag::Dim);
                self.vline(x, y, y + 2);
                self.vline(xr, y, y + 2);
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 1,
                        y: y + 1,
                        w: w - 2,
                        h: 1,
                    },
                    Some(id),
                );
                2
            }
            Kind::Statement(t) => {
                let ls = wrap(&t.text, w - 4);
                self.hline(x, xr, y);
                self.lines(x + 2, y + 1, &ls, Tag::Text);
                self.vline(x, y, y + 1 + ls.len());
                self.vline(xr, y, y + 1 + ls.len());
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 1,
                        y: y + 1,
                        w: w - 2,
                        h: ls.len(),
                    },
                    Some(id),
                );
                1 + ls.len()
            }
            Kind::If { cond, yes, no } => {
                let ls = wrap(&cond.text, w - 4);
                let c = ls.len();
                self.hline(x, xr, y);
                self.lines(x + 2, y + 1, &ls, Tag::Cond);
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 1,
                        y: y + 1,
                        w: w - 2,
                        h: c,
                    },
                    Some(id),
                );
                let ys = y + 1 + c;
                let cols = split(
                    w,
                    &[seq_nat(yes), seq_nat(no)],
                    &[seq_min(yes), seq_min(no)],
                );
                let xm = x + cols[0] - 1;
                let hl = self.seq(yes, x, ys, cols[0]);
                let hr = self.seq(no, xm, ys, cols[1]);
                let hb = hl.max(hr);
                self.vline(x, y, ys + hb);
                self.vline(xr, y, ys + hb);
                self.vline(xm, ys, ys + hb);
                let (yl, nl) = (self.words.yes, self.words.no);
                if cols[0] >= width_of(yl) + 4 {
                    self.text(x + 2, ys, yl, Tag::Label);
                }
                if cols[1] >= width_of(nl) + 4 {
                    self.text(xm + 2, ys, nl, Tag::Label);
                }
                1 + c + hb
            }
            Kind::While { cond, body } => {
                let ls = wrap(&cond.text, w - 4);
                let c = ls.len();
                self.hline(x, xr, y);
                self.lines(x + 2, y + 1, &ls, Tag::Cond);
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 1,
                        y: y + 1,
                        w: w - 2,
                        h: c,
                    },
                    Some(id),
                );
                let ys = y + 1 + c;
                self.hline(x, xr, ys);
                let hb = self.seq(body, x + 2, ys, w - 2);
                self.vline(x, y, ys + hb);
                self.vline(xr, y, ys + hb);
                1 + c + hb
            }
            Kind::Repeat { cond, body } => {
                self.hline(x, xr, y);
                let hb = self.seq(body, x + 2, y, w - 2);
                let ls = wrap(&cond.text, w - 4);
                let c = ls.len();
                self.hline(x, xr, y + hb);
                self.lines(x + 2, y + hb + 1, &ls, Tag::Cond);
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 1,
                        y: y + hb + 1,
                        w: w - 2,
                        h: c,
                    },
                    Some(id),
                );
                self.vline(x, y, y + hb + 1 + c);
                self.vline(xr, y, y + hb + 1 + c);
                hb + 1 + c
            }
            Kind::Switch { cases } => {
                let n = cases.len();
                let nats: Vec<usize> = cases
                    .iter()
                    .map(|c| seq_nat(&c.body).max(longest(case_label(c, self.words)) + 4))
                    .collect();
                let mins: Vec<usize> = cases.iter().map(|c| seq_min(&c.body)).collect();
                let cws = split(w, &nats, &mins);
                let mut bx = vec![x];
                let mut at = x;
                for width in &cws {
                    at += width - 1;
                    bx.push(at);
                }
                let texts: Vec<Vec<String>> = cases
                    .iter()
                    .enumerate()
                    .map(|(i, c)| wrap(case_label(c, self.words), cws[i].saturating_sub(4)))
                    .collect();
                let r = texts.iter().map(Vec::len).max().unwrap_or(1);
                self.hline(x, xr, y);
                self.text(x + 2, y, "case", Tag::Label);
                self.mark(
                    Focus::Node(id),
                    Rect {
                        x: x + 2,
                        y,
                        w: 4,
                        h: 1,
                    },
                    Some(id),
                );
                let ys = y + 1 + r;
                self.hline(x, xr, ys);
                let mut hb = 0;
                for (i, c) in cases.iter().enumerate() {
                    let tag = if c.cond.is_some() {
                        Tag::Cond
                    } else {
                        Tag::Dim
                    };
                    self.lines(bx[i] + 2, y + 1, &texts[i], tag);
                    self.mark(
                        Focus::Case(c.id),
                        Rect {
                            x: bx[i] + 1,
                            y: y + 1,
                            w: cws[i].saturating_sub(2),
                            h: r,
                        },
                        None,
                    );
                    hb = hb.max(self.seq(&c.body, bx[i], ys, cws[i]));
                }
                for b in &bx[..=n] {
                    self.vline(*b, y, ys + hb);
                }
                1 + r + hb
            }
        }
    }
}

/// Paint every diagram of the program under each other, `avail` columns wide if possible.
pub fn layout(p: &Program, avail: usize, words: &Words, failing_node: Option<u64>) -> Layout {
    let mut natural = 0;
    let mut least = 0;
    for d in &p.diagrams {
        natural = natural.max(seq_nat(&d.body));
        least = least.max(seq_min(&d.body));
    }
    let w = natural
        .clamp(30, 400)
        .min(avail.max(10))
        .max(least)
        .max(LEAF_MIN);
    let mut pt = P {
        cells: vec![],
        rects: vec![],
        w,
        words,
        failing_node,
        failing: None,
    };
    let mut y = 0;
    for (k, d) in p.diagrams.iter().enumerate() {
        if k > 0 {
            y += 1;
        }
        let title = if d.name.text.is_empty() {
            "(unnamed)"
        } else {
            &d.name.text
        };
        let shown = match (ascii::on(), d.show_header) {
            (true, true) => "#",
            (true, false) => "-",
            (false, true) => "▍",
            (false, false) => "▏",
        };
        pt.text(0, y, &format!("{shown}{title}"), Tag::Title);
        pt.mark(
            Focus::Name(d.id),
            Rect {
                x: 0,
                y,
                w: width_of(title) + 1,
                h: 1,
            },
            None,
        );
        y += 1;
        if d.show_variables {
            let t = format!("vars: {}", d.variables.text);
            pt.text(0, y, &t, Tag::Dim);
            pt.mark(
                Focus::Vars(d.id),
                Rect {
                    x: 0,
                    y,
                    w: width_of(&t),
                    h: 1,
                },
                None,
            );
            y += 1;
        }
        let h = pt.seq(&d.body, 0, y, w);
        y += h;
        pt.hline(0, w - 1, y);
        pt.vline(0, y - 1, y);
        pt.vline(w - 1, y - 1, y);
        y += 1;
        if d.show_return {
            let t = format!("return: {}", d.ret.text);
            pt.text(0, y, &t, Tag::Dim);
            pt.mark(
                Focus::Ret(d.id),
                Rect {
                    x: 0,
                    y,
                    w: width_of(&t),
                    h: 1,
                },
                None,
            );
            y += 1;
        }
    }
    pt.at(0, y.saturating_sub(1));
    Layout {
        w,
        cells: pt.cells,
        rects: pt.rects,
        failing: pt.failing,
    }
}

/// The picture as plain text (what "copy picture" gives in the terminal).
pub fn to_text(l: &Layout) -> String {
    let mut out = String::new();
    for row in &l.cells {
        let line: String = row
            .iter()
            .filter(|c| c.ch != '\0')
            .map(Cell::shown)
            .collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use speci_local::stuki::model::{Node, Tag as ModelTag};
    use speci_local::stuki::rich::Rich;

    fn sample() -> Program {
        let mut p = Program::default();
        p.diagrams[0].show_header = true;
        let first = p.diagrams[0].body[0].id;
        if let Some(Node {
            kind: Kind::Noop, ..
        }) = p.find(first)
        {}
        let w = p.add_next_to(first, ModelTag::While, true).unwrap();
        if let Some(n) = p.find_mut(w)
            && let Kind::While { cond, body } = &mut n.kind
        {
            *cond = Rich::plain("i ≤ n");
            body[0].kind = Kind::Statement(Rich::plain("i:=i+1"));
        }
        let i = p.add_next_to(w, ModelTag::If, true).unwrap();
        if let Some(n) = p.find_mut(i)
            && let Kind::If { cond, yes, .. } = &mut n.kind
        {
            *cond = Rich::plain("x > 0");
            yes[0].kind = Kind::Statement(Rich::plain("y:=1"));
        }
        let s = p.add_next_to(i, ModelTag::Switch, true).unwrap();
        p.add_case(s, false);
        p
    }

    #[test]
    fn diagram_is_drawn_with_joined_boxes() {
        let l = layout(&sample(), 60, &EN, None);
        let t = to_text(&l);
        println!("{t}");
        assert!(t.contains("i ≤ n"));
        assert!(t.contains("┌") && t.contains("┬") && t.contains("┼"));
        // every row is as wide as the diagram
        assert!(l.cells.iter().all(|r| r.len() == l.w));
        // no stray corner glyph outside the frame: left edge never has a lone `─`
        for r in &l.cells {
            assert_ne!(r[0].shown(), '─');
        }
        assert!(l.rects.len() >= 6);
    }

    #[test]
    fn narrow_widths_still_draw() {
        for w in [10, 20, 33, 80, 140] {
            let l = layout(&sample(), w, &HU, None);
            assert!(l.height() > 5, "width {w}");
        }
    }

    #[test]
    fn wrapping_respects_width() {
        let ls = wrap("alpha beta gamma delta", 10);
        assert!(ls.iter().all(|l| width_of(l) <= 10), "{ls:?}");
        assert_eq!(wrap("", 5), vec![String::new()]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}
