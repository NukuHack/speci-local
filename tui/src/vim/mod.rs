#![allow(dead_code)]
//! Vim keybindings for the code editor: Normal, Insert, Visual and Visual-line modes.
//!
//! Forked from the window app's engine (`src/editor/vim`): the egui glue is replaced by [`Vim::feed`].
//! Insert mode typing is left to the caller; the other modes run the keys as commands.

use std::collections::HashMap;

mod keys;
mod text;

use text::*;

/// Which editor the undo history belongs to.
pub type Id = u64;

pub const ESC: char = '\x1b';
pub const ENTER: char = '\r';
pub const BACK: char = '\x08';
pub const CTRL_R: char = '\x12';

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "V-LINE",
        }
    }

    pub const fn visual(self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// The range ends before the target.
    Excl,
    /// The range includes the target character.
    Incl,
    /// Whole lines.
    Line,
}

enum Mo {
    /// More keys are needed.
    More,
    /// Not a motion (or it can't move): the command is dropped.
    Bad,
    To(usize, Kind),
}

#[derive(Default)]
struct History {
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

/// The last change, for `.`: the command's keys and the text typed in Insert mode after it.
#[derive(Clone, Default)]
struct Change {
    keys: Vec<char>,
    text: String,
}

/// The text being edited, as chars, with the cursor.
struct Ed {
    t: Vec<char>,
    p: usize,
    changed: bool,
}

impl Ed {
    fn new(text: &str, p: usize) -> Self {
        Self {
            t: text.chars().collect(),
            p,
            changed: false,
        }
    }

    fn splice(&mut self, s: usize, e: usize, with: &[char]) {
        self.t.splice(s..e, with.iter().copied());
        self.changed = true;
    }
}

#[derive(Default)]
pub struct Vim {
    pub mode: Mode,
    /// Keys of the command typed so far.
    keys: Vec<char>,
    reg: String,
    reg_line: bool,
    /// Last `f`/`t`/`F`/`T`: (command, target).
    find: Option<(char, char)>,
    /// Last `*`/`#` search: (word, forward).
    search: Option<(Vec<char>, bool)>,
    /// Column kept by `j`/`k` across shorter lines.
    want: Option<usize>,
    keep_want: bool,
    /// Visual mode: where the selection started, and the cursor.
    anchor: usize,
    cur: usize,
    hist: HashMap<Id, History>,
    id: Option<Id>,
    /// An undo step was saved for the command being run.
    saved: bool,
    last: Option<Change>,
    /// Recording the text typed in Insert mode for `.`: (start, text length at start).
    typing: Option<(usize, usize)>,
    replaying: bool,
}

impl Vim {
    /// Text for the status bar: the mode and a half-typed command.
    pub fn status(&self) -> String {
        let keys: String = self.keys.iter().filter(|c| !c.is_control()).collect();
        if keys.is_empty() {
            self.mode.label().into()
        } else {
            format!("{}  {keys}", self.mode.label())
        }
    }

    pub fn reset(&mut self) {
        self.mode = Mode::Normal;
        self.keys.clear();
        self.typing = None;
    }

    pub fn inserting(&self) -> bool {
        self.mode == Mode::Insert
    }

    /// Where to paint the block cursor (Visual modes; Normal mode uses the text cursor).
    pub fn visual_cursor(&self) -> Option<usize> {
        self.mode.visual().then_some(self.cur)
    }

    /// Run one key in Normal / Visual mode on `text` (a char buffer) with the cursor `*p`.
    /// Returns true when the text changed. In Insert mode the caller types; it calls
    /// [`Vim::escape`] to leave.
    pub fn feed(&mut self, id: Id, text: &mut Vec<char>, p: &mut usize, k: char) -> bool {
        if self.id != Some(id) {
            if self.mode.visual() {
                self.mode = Mode::Normal;
            }
            self.keys.clear();
            self.id = Some(id);
        }
        let start = if self.mode.visual() { self.cur } else { *p };
        let mut ed = Ed {
            t: std::mem::take(text),
            p: start,
            changed: false,
        };
        self.press(&mut ed, k);
        ed.p = ed.p.min(ed.t.len());
        match self.mode {
            Mode::Insert => *p = ed.p,
            Mode::Normal => *p = clamp_normal(&ed.t, ed.p),
            Mode::Visual | Mode::VisualLine => {
                self.anchor = self.anchor.min(ed.t.len());
                self.cur = ed.p;
                *p = ed.p;
            }
        }
        *text = ed.t;
        ed.changed
    }

    /// Leave Insert mode (Escape), recording the typed text for `.`.
    pub fn escape(&mut self, id: Id, text: &mut Vec<char>, p: &mut usize) {
        self.id = Some(id);
        let mut ed = Ed {
            t: std::mem::take(text),
            p: *p,
            changed: false,
        };
        self.leave_insert(&mut ed);
        *p = clamp_normal(&ed.t, ed.p);
        *text = ed.t;
    }

    /// Entering Insert mode needs an undo point and the text typed afterwards is recorded.
    pub const fn pending(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The half-typed command (`g`, `2d`, …).
    pub fn pending_keys(&self) -> String {
        self.keys.iter().filter(|c| !c.is_control()).collect()
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    pub fn id_is(&self, id: Id) -> bool {
        self.id == Some(id)
    }

    pub const fn is_visual(&self) -> bool {
        self.mode.visual()
    }

    pub fn in_visual_of(&self, id: Id) -> bool {
        self.mode.visual() && self.id == Some(id)
    }

    /// The selection `s..e` (chars) in a Visual mode.
    pub fn visual_range(&self, t: &[char], p: usize) -> Option<(usize, usize)> {
        self.mode.visual().then(|| self.selection(t, p))
    }

    /// Forget every undo history (the editors' texts were replaced).
    pub fn forget(&mut self) {
        self.hist.clear();
        self.keys.clear();
        self.mode = Mode::Normal;
    }

    /// After typing in Insert mode the caller moved the text: keep `.` bookkeeping honest.
    pub fn in_insert(&self) -> bool {
        self.mode == Mode::Insert
    }

    /// The selected chars `s..e` in a Visual mode.
    fn selection(&self, t: &[char], p: usize) -> (usize, usize) {
        let (s, e) = (self.anchor.min(p), self.anchor.max(p));
        if self.mode == Mode::VisualLine {
            (ls(t, s), le(t, e))
        } else {
            (s, (e + 1).min(t.len()))
        }
    }

    fn press(&mut self, ed: &mut Ed, k: char) {
        if k == ESC && !self.keys.is_empty() {
            // cancels a half-typed command
            self.keys.clear();
            return;
        }
        self.keys.push(k);
        let keys = self.keys.clone();
        self.saved = false;
        self.keep_want = false;
        let was_changed = std::mem::take(&mut ed.changed);
        let before = self.mode;
        let done = if self.mode.visual() {
            self.visual(ed, &keys)
        } else {
            self.normal(ed, &keys)
        };
        let changed = ed.changed;
        ed.changed |= was_changed;
        if !done {
            return;
        }
        self.keys.clear();
        if !self.keep_want {
            self.want = None;
        }
        // remember the change for `.`
        let undoing = matches!(keys.last(), Some(&('u' | CTRL_R | '.')));
        if before == Mode::Normal && !self.replaying && !undoing {
            if self.mode == Mode::Insert {
                self.last = Some(Change {
                    keys,
                    text: String::new(),
                });
                self.typing = Some((ed.p, ed.t.len()));
            } else if changed {
                self.last = Some(Change {
                    keys,
                    text: String::new(),
                });
            }
        }
    }

    fn save(&mut self, ed: &Ed) {
        if self.saved {
            return;
        }
        self.saved = true;
        let h = self.hist.entry(self.id.unwrap_or(0)).or_default();
        h.undo.push((ed.t.iter().collect(), ed.p));
        if h.undo.len() > 500 {
            h.undo.remove(0);
        }
        h.redo.clear();
    }

    fn undo(&mut self, ed: &mut Ed, n: usize, redo: bool) {
        let h = self.hist.entry(self.id.unwrap_or(0)).or_default();
        for _ in 0..n {
            let now: String = ed.t.iter().collect();
            let (from, to) = if redo {
                (&mut h.redo, &mut h.undo)
            } else {
                (&mut h.undo, &mut h.redo)
            };
            // skip steps that didn't change anything
            while from.last().is_some_and(|(s, _)| *s == now) {
                from.pop();
            }
            let Some((s, p)) = from.pop() else { break };
            to.push((now, ed.p));
            ed.t = s.chars().collect();
            ed.p = p;
            ed.changed = true;
        }
    }

    fn start_insert(&mut self, ed: &Ed) {
        self.save(ed);
        self.mode = Mode::Insert;
    }

    fn leave_insert(&mut self, ed: &mut Ed) {
        self.mode = Mode::Normal;
        if let Some((start, len0)) = self.typing.take()
            && let Some(last) = &mut self.last
            && ed.p >= start
            && ed.t.len() >= len0
            && ed.t.len() - len0 == ed.p - start
        {
            last.text = ed.t[start..ed.p].iter().collect();
        }
        if ed.p > ls(&ed.t, ed.p) {
            ed.p -= 1;
        }
        // entering and leaving Insert mode without typing is no undo step
        let h = self.hist.entry(self.id.unwrap_or(0)).or_default();
        let now: String = ed.t.iter().collect();
        if h.undo.last().is_some_and(|(s, _)| *s == now) {
            h.undo.pop();
        }
    }

    fn yank(&mut self, t: &[char], s: usize, e: usize, line: bool) {
        self.reg = t[s..e].iter().collect();
        if line {
            self.reg.push('\n');
        }
        self.reg_line = line;
    }

    /// Run an operator (`d`, `c`, `y`, `>`, `<`, `~`, `u`, `U`) from `a` to `b`.
    fn apply(&mut self, ed: &mut Ed, op: char, a: usize, b: usize, kind: Kind) {
        let len = ed.t.len();
        let (mut s, mut e) = (a.min(b).min(len), a.max(b).min(len));
        match kind {
            Kind::Excl => {}
            Kind::Incl => e = (e + 1).min(len),
            Kind::Line => {
                s = ls(&ed.t, s);
                e = le(&ed.t, e);
            }
        }
        let line = kind == Kind::Line;
        match op {
            'y' => {
                self.yank(&ed.t, s, e, line);
                ed.p = a.min(b);
            }
            'd' => {
                self.save(ed);
                self.yank(&ed.t, s, e, line);
                if line {
                    // take one line break along
                    let (ds, de) = if e < len {
                        (s, e + 1)
                    } else {
                        (s.saturating_sub(1), e)
                    };
                    ed.splice(ds, de, &[]);
                    let at = ls(&ed.t, ds.min(ed.t.len()));
                    ed.p = fnb(&ed.t, at);
                } else {
                    ed.splice(s, e, &[]);
                    ed.p = s;
                }
            }
            'c' => {
                self.save(ed);
                self.yank(&ed.t, s, e, line);
                if line {
                    let indent = indent_of(&ed.t, s);
                    ed.splice(s, e, &indent);
                    ed.p = s + indent.len();
                } else {
                    ed.splice(s, e, &[]);
                    ed.p = s;
                }
                self.mode = Mode::Insert;
            }
            '>' | '<' => {
                self.save(ed);
                let first = line_no(&ed.t, s);
                let last = line_no(&ed.t, if line || e == s { e } else { e - 1 });
                for n in first..=last {
                    let st = line_start_n(&ed.t, n);
                    let en = le(&ed.t, st);
                    if op == '>' {
                        if en > st {
                            ed.splice(st, st, &[' ', ' ']);
                        }
                    } else {
                        let drop = if ed.t.get(st) == Some(&'\t') {
                            1
                        } else {
                            ed.t[st..en]
                                .iter()
                                .take(2)
                                .take_while(|c| **c == ' ')
                                .count()
                        };
                        ed.splice(st, st + drop, &[]);
                    }
                }
                ed.p = fnb(&ed.t, line_start_n(&ed.t, first));
            }
            '~' | 'u' | 'U' => {
                self.save(ed);
                let new: Vec<char> = ed.t[s..e]
                    .iter()
                    .flat_map(|&c| match op {
                        'u' => c.to_lowercase().collect::<Vec<_>>(),
                        'U' => c.to_uppercase().collect(),
                        _ if c.is_uppercase() => c.to_lowercase().collect(),
                        _ => c.to_uppercase().collect(),
                    })
                    .collect();
                ed.splice(s, e, &new);
                ed.p = s;
            }
            _ => {}
        }
    }

    fn put(&mut self, ed: &mut Ed, before: bool, n: usize) {
        if self.reg.is_empty() {
            return;
        }
        self.save(ed);
        let txt: Vec<char> = self.reg.repeat(n).chars().collect();
        if self.reg_line {
            if before {
                let at = ls(&ed.t, ed.p);
                ed.splice(at, at, &txt);
                ed.p = fnb(&ed.t, at);
            } else {
                let l = le(&ed.t, ed.p);
                if l < ed.t.len() {
                    ed.splice(l + 1, l + 1, &txt);
                } else {
                    // the last line has no line break: move it to the front
                    let mut v = vec!['\n'];
                    v.extend_from_slice(&txt[..txt.len() - 1]);
                    ed.splice(l, l, &v);
                }
                ed.p = fnb(&ed.t, l + 1);
            }
        } else {
            let at = if before || ed.p >= ed.t.len() || ed.t[ed.p] == '\n' {
                ed.p
            } else {
                ed.p + 1
            };
            ed.splice(at, at, &txt);
            ed.p = at + txt.len() - 1;
        }
    }

    /// Join `n` lines (at least two) from the cursor's.
    fn join(&mut self, ed: &mut Ed, n: usize) {
        self.save(ed);
        for _ in 0..n.max(2) - 1 {
            let l = le(&ed.t, ed.p);
            if l >= ed.t.len() {
                break;
            }
            let mut e = l + 1;
            while e < ed.t.len() && matches!(ed.t[e], ' ' | '\t') {
                e += 1;
            }
            let next_empty = e >= ed.t.len() || ed.t[e] == '\n' || ed.t[e] == ')';
            let ends_blank = l == 0 || matches!(ed.t[l - 1], ' ' | '\t' | '\n');
            let sep: &[char] = if next_empty || ends_blank {
                &[]
            } else {
                &[' ']
            };
            ed.splice(l, e, sep);
            ed.p = l;
        }
    }
}

#[cfg(test)]
pub fn run_keys(text: &str, p: usize, keys: &str) -> (String, usize, Mode) {
    let mut v = Vim::default();
    let mut ed = Ed::new(text, p);
    for k in keys.chars() {
        let k = if k == '⎋' { ESC } else { k };
        if v.mode == Mode::Insert {
            if k == ESC {
                v.leave_insert(&mut ed);
            } else {
                ed.splice(ed.p, ed.p, &[k]);
                ed.p += 1;
            }
        } else {
            v.press(&mut ed, k);
        }
    }
    let p = if v.mode == Mode::Normal {
        clamp_normal(&ed.t, ed.p)
    } else {
        ed.p
    };
    (ed.t.iter().collect(), p, v.mode)
}
