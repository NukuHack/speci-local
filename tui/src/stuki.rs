//! The structogram editor: state, keys, undo, files and the background test run.

use crate::input::{In, Input};
use crate::nslayout::{Focus, Layout};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use speci_local::stuki::format;
use speci_local::stuki::model::{Diagram, Kind, Node, Place, Program, Tag, TextKey, fresh_id};
use speci_local::stuki::rich::Rich;
use speci_local::stuki::run::{Verdict, run_tests};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

const UNDO_LIMIT: usize = 200;

pub type Checks = Result<Vec<Verdict>, String>;

struct Job {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Checks>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct Edit {
    pub target: Focus,
    pub input: Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Insert { after: bool },
}

/// What the app should do with a key the structogram editor doesn't handle.
#[derive(PartialEq, Eq, Debug)]
pub enum Req {
    None,
    Command,
}

pub struct Stuki {
    pub program: Program,
    saved: String,
    pub path: Option<PathBuf>,
    undo: Vec<Program>,
    redo: Vec<Program>,
    pub focus: Focus,
    pub top: usize,
    pub left: usize,
    pub edit: Option<Edit>,
    pub menu: Option<Menu>,
    pub keys: Vec<char>,
    yank: Option<Node>,
    pub status: String,
    pub checks: Option<Checks>,
    checks_of: u64,
    job: Option<Job>,
    pub shown_test: usize,
    /// What the last draw showed, for scrolling and navigation.
    pub order: Vec<Focus>,
    pub view_h: usize,
    pub last_rows: Vec<(Focus, usize)>,
}

fn fresh(n: &mut Node) {
    n.id = fresh_id();
    for s in n.seqs_mut() {
        for c in s {
            fresh(c);
        }
    }
    if let Kind::Switch { cases } = &mut n.kind {
        for c in cases {
            c.id = fresh_id();
        }
    }
}

struct Item {
    focus: Focus,
    parent: Option<Focus>,
}

fn walk_seq(nodes: &[Node], parent: Option<Focus>, out: &mut Vec<Item>) {
    for n in nodes {
        let f = Focus::Node(n.id);
        if let Kind::Repeat { body, .. } = &n.kind {
            walk_seq(body, Some(f), out);
            out.push(Item { focus: f, parent });
        } else {
            out.push(Item { focus: f, parent });
            match &n.kind {
                Kind::If { yes, no, .. } => {
                    walk_seq(yes, Some(f), out);
                    walk_seq(no, Some(f), out);
                }
                Kind::While { body, .. } => walk_seq(body, Some(f), out),
                Kind::Switch { cases } => {
                    for c in cases {
                        let cf = Focus::Case(c.id);
                        out.push(Item {
                            focus: cf,
                            parent: Some(f),
                        });
                        walk_seq(&c.body, Some(cf), out);
                    }
                }
                _ => {}
            }
        }
    }
}

fn walk(p: &Program) -> Vec<Item> {
    let mut out = vec![];
    for d in &p.diagrams {
        out.push(Item {
            focus: Focus::Name(d.id),
            parent: None,
        });
        if d.show_variables {
            out.push(Item {
                focus: Focus::Vars(d.id),
                parent: Some(Focus::Name(d.id)),
            });
        }
        walk_seq(&d.body, Some(Focus::Name(d.id)), &mut out);
        if d.show_return {
            out.push(Item {
                focus: Focus::Ret(d.id),
                parent: Some(Focus::Name(d.id)),
            });
        }
    }
    out
}

fn seq_ids(nodes: &[Node], id: u64) -> Option<Vec<u64>> {
    if nodes.iter().any(|n| n.id == id) {
        return Some(nodes.iter().map(|n| n.id).collect());
    }
    nodes
        .iter()
        .find_map(|n| n.seqs().into_iter().find_map(|s| seq_ids(s, id)))
}

impl Default for Stuki {
    fn default() -> Self {
        Self::with(Program::default(), None, true)
    }
}

impl Stuki {
    pub fn with(program: Program, path: Option<PathBuf>, saved: bool) -> Self {
        let focus = program
            .diagrams
            .first()
            .and_then(|d| d.body.first())
            .map_or(Focus::Name(0), |n| Focus::Node(n.id));
        Self {
            saved: if saved {
                program.content()
            } else {
                String::new()
            },
            program,
            path,
            undo: vec![],
            redo: vec![],
            focus,
            top: 0,
            left: 0,
            edit: None,
            menu: None,
            keys: vec![],
            yank: None,
            status: String::new(),
            checks: None,
            checks_of: 0,
            job: None,
            shown_test: 0,
            order: vec![],
            view_h: 20,
            last_rows: vec![],
        }
    }

    pub fn replace(&mut self, program: Program, path: Option<PathBuf>, saved: bool) {
        let status = std::mem::take(&mut self.status);
        *self = Self::with(program, path, saved);
        self.status = status;
    }

    pub fn dirty(&self) -> bool {
        !self.program.content_is(&self.saved)
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.program.content();
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "untitled.st".into(), |f| f.to_string_lossy().to_string())
    }

    pub fn to_file(&self) -> String {
        format::to_st(&self.program)
    }

    fn push_undo(&mut self) {
        self.undo.push(self.program.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn history(&mut self, redo: bool) {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        match from.pop() {
            Some(p) => {
                to.push(std::mem::replace(&mut self.program, p));
                self.fix_focus();
                self.status = if redo { "redo".into() } else { "undo".into() };
            }
            None => self.status = "nothing to ".to_string() + if redo { "redo" } else { "undo" },
        }
    }

    fn items(&self) -> Vec<Item> {
        walk(&self.program)
    }

    fn fix_focus(&mut self) {
        let items = self.items();
        if !items.iter().any(|i| i.focus == self.focus) {
            let pos = self
                .order
                .iter()
                .position(|f| *f == self.focus)
                .unwrap_or(0);
            self.focus = items
                .get(pos.min(items.len().saturating_sub(1)))
                .map_or(self.focus, |i| i.focus);
        }
    }

    fn diagram_of(&self, f: Focus) -> Option<usize> {
        match f {
            Focus::Name(d) | Focus::Vars(d) | Focus::Ret(d) => {
                self.program.diagrams.iter().position(|x| x.id == d)
            }
            Focus::Node(id) => self.program.diagram_of(id),
            Focus::Case(c) => self
                .program
                .switch_of_case(c)
                .and_then(|s| self.program.diagram_of(s)),
        }
    }

    pub fn current_diagram(&self) -> usize {
        self.diagram_of(self.focus).unwrap_or(0)
    }

    pub fn text_of(&self, f: Focus) -> Option<String> {
        let key = match f {
            Focus::Name(d) => TextKey::Name(d),
            Focus::Vars(d) => TextKey::Vars(d),
            Focus::Ret(d) => TextKey::Ret(d),
            Focus::Node(n) => TextKey::Node(n),
            Focus::Case(c) => TextKey::Case(c),
        };
        self.program.text(key).map(|r| r.text.clone())
    }

    /// The node a verdict points at for the shown test.
    pub fn failing_node(&self) -> Option<u64> {
        match &self.checks {
            Some(Ok(v)) => v.get(self.shown_test).and_then(|v| v.node),
            _ => None,
        }
    }

    // ------------------------------------------------------------ test runs

    /// Start (or restart) the test run when the program, the specification or the tests changed.
    pub fn refresh_checks(&mut self, spec: &str, tests: &[String], force: bool) {
        let mut h = std::hash::DefaultHasher::new();
        (self.program.content(), spec, tests).hash(&mut h);
        let of = h.finish();
        if of == self.checks_of && !force {
            return;
        }
        self.checks_of = of;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (program, spec, tests) = (self.program.clone(), spec.to_string(), tests.to_vec());
        let c = cancel.clone();
        let spawned = std::thread::Builder::new()
            .stack_size(speci_local::eval::EVAL_STACK)
            .spawn(move || {
                let out: Checks = match speci_local::parse(&spec) {
                    Ok(s) => Ok(run_tests(&program, &s, &tests, &c)),
                    Err(d) => Err(match d.first() {
                        Some(d) if d.line > 0 => {
                            format!("Fix the specification first (line {}: {})", d.line, d.title)
                        }
                        Some(d) => format!("Fix the specification first ({})", d.title),
                        None => "Fix the specification first".into(),
                    }),
                };
                if !c.load(Ordering::Relaxed) {
                    let _ = tx.send(out);
                }
            });
        if spawned.is_ok() {
            self.job = Some(Job { cancel, rx });
        }
    }

    /// Pick up a finished run. True when something changed.
    pub fn poll(&mut self) -> bool {
        let Some(j) = &self.job else { return false };
        match j.rx.try_recv() {
            Ok(r) => {
                self.checks = Some(r);
                self.job = None;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(_) => {
                self.job = None;
                false
            }
        }
    }

    pub const fn running(&self) -> bool {
        self.job.is_some()
    }

    // ------------------------------------------------------------ editing

    fn begin_edit(&mut self, clear: bool) {
        let f = self.focus;
        let Some(t) = self.text_of(f) else {
            self.status =
                "Nothing to edit here: edit the cases of a multi-way decision instead".into();
            return;
        };
        let t = if matches!(f, Focus::Node(id) if self.program.find(id).is_some_and(Node::is_noop))
            || clear
        {
            String::new()
        } else {
            t
        };
        self.edit = Some(Edit {
            target: f,
            input: Input::new(&t).symbols(),
        });
    }

    fn commit_edit(&mut self) {
        let Some(e) = self.edit.take() else { return };
        let new = e.input.text();
        if self.text_of(e.target).as_deref() == Some(new.as_str()) {
            return;
        }
        self.push_undo();
        let key = match e.target {
            Focus::Name(d) => TextKey::Name(d),
            Focus::Vars(d) => TextKey::Vars(d),
            Focus::Ret(d) => TextKey::Ret(d),
            Focus::Node(n) => {
                if self.program.find(n).is_some_and(Node::is_noop) {
                    if new.is_empty() {
                        self.undo.pop();
                        return;
                    }
                    if let Some(node) = self.program.find_mut(n) {
                        node.kind = Kind::Statement(Rich::plain(&new));
                    }
                    return;
                }
                TextKey::Node(n)
            }
            Focus::Case(c) => TextKey::Case(c),
        };
        if let Some(r) = self.program.text_mut(key) {
            let old = std::mem::replace(&mut r.text, new);
            r.edited(&old);
        }
    }

    fn insert(&mut self, tag: Tag, after: bool) {
        self.push_undo();
        let target = match self.focus {
            Focus::Node(id) => Some(id),
            Focus::Case(c) => {
                self.program
                    .switch_of_case(c)
                    .and_then(|s| match &self.program.find(s)?.kind {
                        Kind::Switch { cases } => {
                            cases.iter().find(|x| x.id == c)?.body.first().map(|n| n.id)
                        }
                        _ => None,
                    })
            }
            f => self
                .diagram_of(f)
                .and_then(|i| self.program.diagrams[i].body.first().map(|n| n.id)),
        };
        let after = after && matches!(self.focus, Focus::Node(_));
        match target.and_then(|t| self.program.add_next_to(t, tag, after)) {
            Some(id) => {
                self.focus = Focus::Node(id);
                self.status = format!("Inserted a {}", tag.description());
            }
            None => {
                self.undo.pop();
            }
        }
    }

    fn delete(&mut self) {
        match self.focus {
            Focus::Node(id) => {
                self.push_undo();
                self.yank = self.program.remove(id).map(|mut n| {
                    fresh(&mut n);
                    n
                });
                self.status = "Deleted (p pastes it)".into();
            }
            Focus::Case(c) => {
                if let Some(s) = self.program.switch_of_case(c) {
                    self.push_undo();
                    self.program.delete_case(s, c);
                }
            }
            Focus::Name(d) => {
                if self.program.diagrams.len() > 1 {
                    self.push_undo();
                    self.program.diagrams.retain(|x| x.id != d);
                    self.status = "Deleted the structogram".into();
                } else {
                    self.status = "The last structogram can't be deleted".into();
                }
            }
            _ => self.status = "Use `t` + h/v/r to hide this line".into(),
        }
        self.fix_focus();
    }

    fn move_node(&mut self, down: bool) {
        let Focus::Node(id) = self.focus else { return };
        let Some(ids) = self
            .program
            .diagrams
            .iter()
            .find_map(|d| seq_ids(&d.body, id))
        else {
            return;
        };
        let Some(i) = ids.iter().position(|x| *x == id) else {
            return;
        };
        let (target, place) = if down {
            match ids.get(i + 1) {
                Some(t) => (*t, Place::After),
                None => return,
            }
        } else if i > 0 {
            (ids[i - 1], Place::Before)
        } else {
            return;
        };
        let snap = self.program.clone();
        if self.program.move_node(id, target, place) {
            self.undo.push(snap);
            self.redo.clear();
        }
    }

    fn put(&mut self, after: bool) {
        let Some(mut n) = self.yank.clone() else {
            self.status = "Nothing to paste (yy or dd first)".into();
            return;
        };
        fresh(&mut n);
        let Focus::Node(t) = self.focus else { return };
        self.push_undo();
        let id = n.id;
        let place = if self.program.find(t).is_some_and(Node::is_noop) {
            Place::Replace
        } else if after {
            Place::After
        } else {
            Place::Before
        };
        if self.program.insert(t, place, n).is_ok() {
            self.focus = Focus::Node(id);
        }
    }

    fn goto_item(&mut self, pick: impl Fn(&[Item], usize) -> Option<usize>) {
        let items = self.items();
        let cur = items
            .iter()
            .position(|i| i.focus == self.focus)
            .unwrap_or(0);
        if let Some(i) = pick(&items, cur) {
            self.focus = items[i].focus;
        }
    }

    fn step(&mut self, d: isize, n: usize) {
        for _ in 0..n {
            self.goto_item(|it, c| {
                let t = c as isize + d;
                (t >= 0 && (t as usize) < it.len()).then_some(t as usize)
            });
        }
    }

    fn sibling(&mut self, fwd: bool) {
        self.goto_item(|it, c| {
            let par = it[c].parent;
            if fwd {
                (c + 1..it.len()).find(|&i| it[i].parent == par)
            } else {
                (0..c).rev().find(|&i| it[i].parent == par)
            }
        });
    }

    fn diagram_jump(&mut self, fwd: bool) {
        let cur = self.current_diagram();
        let t = if fwd { cur + 1 } else { cur.wrapping_sub(1) };
        if let Some(d) = self.program.diagrams.get(t) {
            self.focus = Focus::Name(d.id);
        }
    }

    fn toggle(&mut self, which: char) {
        let Some(i) = self.diagram_of(self.focus) else {
            return;
        };
        self.push_undo();
        let d: &mut Diagram = &mut self.program.diagrams[i];
        let flag = match which {
            'h' => &mut d.show_header,
            'v' => &mut d.show_variables,
            _ => &mut d.show_return,
        };
        *flag = !*flag;
        self.fix_focus();
    }

    /// The row the cursor is on in the last drawing, to move by screen lines.
    fn row_of(&self, f: Focus) -> Option<usize> {
        self.last_rows
            .iter()
            .find(|(g, _)| *g == f)
            .map(|(_, y)| *y)
    }

    fn page(&mut self, down: bool, rows: usize) {
        let Some(y) = self.row_of(self.focus) else {
            return;
        };
        let target = if down {
            y + rows
        } else {
            y.saturating_sub(rows)
        };
        let pick = if down {
            self.last_rows
                .iter()
                .find(|(_, r)| *r >= target)
                .or_else(|| self.last_rows.last())
        } else {
            self.last_rows
                .iter()
                .rev()
                .find(|(_, r)| *r <= target)
                .or_else(|| self.last_rows.first())
        };
        if let Some((f, _)) = pick {
            self.focus = *f;
        }
    }

    // ------------------------------------------------------------ keys

    pub fn key(&mut self, k: KeyEvent) -> Req {
        if let Some(e) = &mut self.edit {
            match e.input.key(k) {
                In::Accept => self.commit_edit(),
                In::Cancel => self.edit = None,
                _ => {}
            }
            return Req::None;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(Menu::Insert { after }) = self.menu.take() {
            let tag = match k.code {
                KeyCode::Char('s') | KeyCode::Enter => Some(Tag::Statement),
                KeyCode::Char('d' | 'i') => Some(Tag::If),
                KeyCode::Char('w' | 'l') => Some(Tag::While),
                KeyCode::Char('r') => Some(Tag::Repeat),
                KeyCode::Char('c' | 'm') => Some(Tag::Switch),
                _ => None,
            };
            if let Some(t) = tag {
                self.insert(t, after);
                if t == Tag::Statement {
                    self.begin_edit(true);
                }
            }
            return Req::None;
        }
        let c = match k.code {
            KeyCode::Char(c) if !ctrl => c,
            KeyCode::Esc => {
                self.keys.clear();
                return Req::None;
            }
            KeyCode::Down => 'j',
            KeyCode::Up => 'k',
            KeyCode::Left => 'h',
            KeyCode::Right => 'l',
            KeyCode::Enter => 'e',
            KeyCode::Delete => 'x',
            KeyCode::Home => {
                self.keys.clear();
                self.goto_item(|_, _| Some(0));
                return Req::None;
            }
            KeyCode::End => {
                self.goto_item(|it, _| Some(it.len() - 1));
                return Req::None;
            }
            KeyCode::Char('r') => {
                self.history(true);
                return Req::None;
            }
            KeyCode::Char('d') | KeyCode::PageDown => {
                self.page(true, self.view_h / 2);
                return Req::None;
            }
            KeyCode::Char('u') | KeyCode::PageUp => {
                self.page(false, self.view_h / 2);
                return Req::None;
            }
            KeyCode::Char('f') => {
                self.page(true, self.view_h.saturating_sub(2));
                return Req::None;
            }
            KeyCode::Char('b') => {
                self.page(false, self.view_h.saturating_sub(2));
                return Req::None;
            }
            KeyCode::Char('e') => {
                self.top += 1;
                return Req::None;
            }
            KeyCode::Char('y') => {
                self.top = self.top.saturating_sub(1);
                return Req::None;
            }
            _ => return Req::None,
        };
        self.keys.push(c);
        let ks: String = self.keys.iter().collect();
        let mut n = 1usize;
        let mut cmd = ks.as_str();
        let digits = cmd.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && digits < cmd.len() && !cmd.starts_with('0') {
            n = cmd[..digits].parse().unwrap_or(1);
            cmd = &cmd[digits..];
        } else if digits == cmd.len() && !cmd.starts_with('0') {
            return Req::None; // still a count
        }
        let mut done = true;
        match cmd {
            "j" => self.step(1, n),
            "k" => self.step(-1, n),
            "h" => self.goto_item(|it, c| {
                it[c]
                    .parent
                    .and_then(|p| it.iter().position(|i| i.focus == p))
            }),
            "l" => self
                .goto_item(|it, c| (c + 1..it.len()).find(|&i| it[i].parent == Some(it[c].focus))),
            "w" => self.sibling(true),
            "b" => self.sibling(false),
            "g" | "z" | "d" | "y" | "c" | "t" | "+" | "[" | "]" => done = false,
            "gg" => self.goto_item(|_, _| Some(0)),
            "G" => self.goto_item(|it, _| Some(it.len() - 1)),
            "}" | "]]" => self.diagram_jump(true),
            "{" | "[[" => self.diagram_jump(false),
            "i" | "a" | "e" => self.begin_edit(false),
            "cc" | "C" | "S" | "s" => self.begin_edit(true),
            "o" | "O" => {
                self.menu = Some(Menu::Insert { after: cmd == "o" });
                self.status = "insert: [s]tatement  [d]ecision  [w]hile loop  [r]epeat loop  [c]ase (multi-way)".into();
            }
            "dd" | "x" | "D" => self.delete(),
            "yy" | "Y" => {
                if let Focus::Node(id) = self.focus {
                    self.yank = self.program.find(id).cloned();
                    self.status = "Yanked".into();
                }
            }
            "p" => self.put(true),
            "P" => self.put(false),
            "J" => self.move_node(true),
            "K" => self.move_node(false),
            "u" => self.history(false),
            "U" => self.history(true),
            "+c" | "+d" => {
                let sw = match self.focus {
                    Focus::Case(c) => self.program.switch_of_case(c),
                    Focus::Node(id)
                        if matches!(
                            self.program.find(id).map(|n| &n.kind),
                            Some(Kind::Switch { .. })
                        ) =>
                    {
                        Some(id)
                    }
                    _ => None,
                };
                match sw {
                    Some(s) => {
                        self.push_undo();
                        self.program.add_case(s, cmd == "+d");
                    }
                    None => self.status = "Put the cursor on a multi-way decision first".into(),
                }
            }
            "th" | "tv" | "tr" => self.toggle(cmd.chars().nth(1).unwrap_or('h')),
            "zh" => self.left = self.left.saturating_sub(8),
            "zl" => self.left += 8,
            "n" => {
                self.push_undo();
                self.program.diagrams.push(Diagram::new());
                if let Some(d) = self.program.diagrams.last() {
                    self.focus = Focus::Name(d.id);
                }
                self.status = "New structogram: give it a name with e".into();
            }
            "r" => self.status = "Tests run automatically; :run forces a run".into(),
            "T" | "]t" => {
                if let Some(Ok(v)) = &self.checks
                    && !v.is_empty()
                {
                    self.shown_test = (self.shown_test + 1) % v.len();
                }
            }
            "[t" => {
                if let Some(Ok(v)) = &self.checks
                    && !v.is_empty()
                {
                    self.shown_test = (self.shown_test + v.len() - 1) % v.len();
                }
            }
            ":" => {
                self.keys.clear();
                return Req::Command;
            }
            _ => {}
        }
        if done || self.keys.len() > 3 {
            self.keys.clear();
        }
        Req::None
    }

    pub fn layout_words(spec: &str) -> crate::nslayout::Words {
        if spec.lines().any(|l| {
            ["Be:", "Ki:", "Ef:", "Uf:", "Sa:", "Fv:"]
                .iter()
                .any(|h| l.trim_start().starts_with(h))
        }) {
            crate::nslayout::HU
        } else {
            crate::nslayout::EN
        }
    }

    pub fn note_layout(&mut self, l: &Layout, h: usize) {
        self.order = l.order();
        self.view_h = h;
        self.last_rows = l.rects.iter().map(|(f, r)| (*f, r.y)).collect();
    }
}
