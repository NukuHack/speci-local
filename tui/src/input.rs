//! A one-line text input with emacs-ish editing keys, used by the command line, the search
//! prompt, the file name field and the structogram text editor.

use crate::editor::{digraph, expand_alias};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum In {
    None,
    Accept,
    Cancel,
    Up,
    Down,
    Tab,
}

#[derive(Clone, Default, Debug)]
pub struct Input {
    pub t: Vec<char>,
    pub p: usize,
    dg: Option<Vec<char>>,
    /// Convert `<=`, `>=`, `!=` to ≤ ≥ ≠ while typing.
    pub symbols: bool,
    /// Backspace on an empty input cancels it (like the vim command line).
    pub cancel_on_empty: bool,
}

impl Input {
    pub fn new(text: &str) -> Self {
        let t: Vec<char> = text.chars().collect();
        Self {
            p: t.len(),
            t,
            ..Self::default()
        }
    }

    pub const fn symbols(mut self) -> Self {
        self.symbols = true;
        self
    }

    pub const fn cancel_on_empty(mut self) -> Self {
        self.cancel_on_empty = true;
        self
    }

    pub fn text(&self) -> String {
        self.t.iter().collect()
    }

    pub fn set(&mut self, s: &str) {
        self.t = s.chars().collect();
        self.p = self.t.len();
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars().filter(|c| *c != '\n' && *c != '\r') {
            self.ins(c);
        }
    }

    fn ins(&mut self, c: char) {
        if self.symbols && !c.is_ascii_alphanumeric() && c != '\\' {
            expand_alias(&mut self.t, &mut self.p);
        }
        self.t.insert(self.p, c);
        self.p += 1;
        if self.symbols && c == '=' && self.p >= 2 {
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
    }

    pub fn key(&mut self, k: KeyEvent) -> In {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Esc => return In::Cancel,
            KeyCode::Char('c' | '[') if ctrl => return In::Cancel,
            KeyCode::Enter => {
                if self.symbols {
                    expand_alias(&mut self.t, &mut self.p);
                }
                return In::Accept;
            }
            KeyCode::Up => return In::Up,
            KeyCode::Down => return In::Down,
            KeyCode::Char('p') if ctrl => return In::Up,
            KeyCode::Char('n') if ctrl => return In::Down,
            KeyCode::Tab => {
                if !(self.symbols && expand_alias(&mut self.t, &mut self.p)) {
                    return In::Tab;
                }
            }
            KeyCode::Backspace | KeyCode::Char('h') if k.code == KeyCode::Backspace || ctrl => {
                if self.t.is_empty() && self.cancel_on_empty {
                    return In::Cancel;
                }
                if self.p > 0 {
                    self.p -= 1;
                    self.t.remove(self.p);
                }
            }
            KeyCode::Delete => {
                if self.p < self.t.len() {
                    self.t.remove(self.p);
                }
            }
            KeyCode::Left => self.p = self.p.saturating_sub(1),
            KeyCode::Char('b') if ctrl => self.p = self.p.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('f') if k.code == KeyCode::Right || ctrl => {
                self.p = (self.p + 1).min(self.t.len());
            }
            KeyCode::Home => self.p = 0,
            KeyCode::Char('a') if ctrl => self.p = 0,
            KeyCode::End => self.p = self.t.len(),
            KeyCode::Char('e') if ctrl => self.p = self.t.len(),
            KeyCode::Char('u') if ctrl => {
                self.t.drain(..self.p);
                self.p = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let mut a = self.p;
                while a > 0 && self.t[a - 1] == ' ' {
                    a -= 1;
                }
                while a > 0
                    && self.t[a - 1] != ' '
                    && (self.t[a - 1].is_alphanumeric() || self.t[a - 1] == '_')
                {
                    a -= 1;
                }
                if a == self.p && a > 0 {
                    a -= 1;
                }
                self.t.drain(a..self.p);
                self.p = a;
            }
            KeyCode::Char('k') if ctrl => self.dg = Some(vec![]),
            KeyCode::Char(c) if alt && ('1'..='6').contains(&c) => {
                self.ins(['∈', '∀', '∃', '≠', '≤', '≥'][c as usize - '1' as usize]);
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(mut d) = self.dg.take() {
                    d.push(c);
                    if d.len() == 2 {
                        self.ins(digraph(d[0], d[1]).unwrap_or(d[1]));
                    } else {
                        self.dg = Some(d);
                    }
                } else {
                    self.ins(c);
                }
            }
            _ => {}
        }
        In::None
    }
}
