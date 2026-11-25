//! A keyboard-driven file browser (Open and Save as), vim-style.

use crate::input::{In, Input};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Spec,
    Stuki,
}

impl Kind {
    pub const fn ext(self) -> &'static str {
        match self {
            Self::Spec => "sp",
            Self::Stuki => "st",
        }
    }
    pub fn of(path: &Path) -> Self {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("st"))
        {
            Self::Stuki
        } else {
            Self::Spec
        }
    }
    pub fn with_ext(self, mut p: PathBuf) -> PathBuf {
        let has = p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(self.ext()));
        if !has && let Some(n) = p.file_name() {
            let mut n = n.to_os_string();
            n.push(".");
            n.push(self.ext());
            p.set_file_name(n);
        }
        p
    }
}

pub struct Entry {
    pub name: String,
    pub dir: bool,
}

pub enum Chosen {
    Open(PathBuf),
    Save(PathBuf),
}

pub struct Browser {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub sel: usize,
    pub scroll: usize,
    pub save: bool,
    pub kind: Kind,
    pub name: Input,
    pub naming: bool,
    pub hidden: bool,
    pub all: bool,
    pub filter: Option<Input>,
    pub msg: String,
    pub height: usize,
    pending_g: bool,
}

impl Browser {
    pub fn new(save: bool, kind: Kind, start: Option<&Path>, name: &str) -> Self {
        let dir = start
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut b = Self {
            dir,
            entries: vec![],
            sel: 0,
            scroll: 0,
            save,
            kind,
            name: Input::new(name),
            naming: save,
            hidden: false,
            all: false,
            filter: None,
            msg: String::new(),
            height: 10,
            pending_g: false,
        };
        b.reload();
        b
    }

    pub fn reload(&mut self) {
        let mut v: Vec<Entry> = vec![];
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if !self.hidden && name.starts_with('.') {
                        continue;
                    }
                    let dir = e.path().is_dir();
                    let want = dir
                        || self.all
                        || e.path().extension().is_some_and(|x| {
                            x.eq_ignore_ascii_case("sp") || x.eq_ignore_ascii_case("st")
                        });
                    let f = self
                        .filter
                        .as_ref()
                        .map_or_default(|f| f.text().to_lowercase());
                    if want && name.to_lowercase().contains(&f) {
                        v.push(Entry { name, dir });
                    }
                }
                self.msg.clear();
            }
            Err(e) => self.msg = format!("{}: {e}", self.dir.display()),
        }
        v.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        if self.dir.parent().is_some() {
            v.insert(
                0,
                Entry {
                    name: "..".into(),
                    dir: true,
                },
            );
        }
        self.entries = v;
        self.sel = self.sel.min(self.entries.len().saturating_sub(1));
    }

    fn enter(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.sel = 0;
        self.scroll = 0;
        self.reload();
    }

    fn up(&mut self) {
        if let Some(p) = self.dir.parent().map(Path::to_path_buf) {
            let from = self
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            self.enter(p);
            if let Some(f) = from
                && let Some(i) = self.entries.iter().position(|e| e.name == f)
            {
                self.sel = i;
            }
        }
    }

    fn mv(&mut self, d: isize) {
        let n = self.entries.len() as isize;
        if n > 0 {
            self.sel = (self.sel as isize + d).clamp(0, n - 1) as usize;
        }
    }

    fn target(&self) -> PathBuf {
        self.kind.with_ext(self.dir.join(self.name.text().trim()))
    }

    pub fn key(&mut self, k: KeyEvent) -> Option<Result<Chosen, ()>> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.naming {
            match self.name.key(k) {
                In::Cancel => {
                    self.naming = false;
                    if !self.save {
                        return None;
                    }
                }
                In::Accept => {
                    if self.name.text().trim().is_empty() {
                        self.msg = "Type a file name".into();
                    } else {
                        return Some(Ok(Chosen::Save(self.target())));
                    }
                }
                _ => {}
            }
            return None;
        }
        if let Some(f) = &mut self.filter {
            match f.key(k) {
                In::Cancel => self.filter = None,
                In::Accept => {
                    if f.t.is_empty() {
                        self.filter = None;
                    }
                    self.sel = 0;
                    self.reload();
                    return None;
                }
                _ => {}
            }
            self.sel = 0;
            self.reload();
            return None;
        }
        let g = std::mem::take(&mut self.pending_g);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => return Some(Err(())),
            KeyCode::Char('c') if ctrl => return Some(Err(())),
            KeyCode::Char('j') | KeyCode::Down => self.mv(1),
            KeyCode::Char('k') | KeyCode::Up => self.mv(-1),
            KeyCode::Char('n') if ctrl => self.mv(1),
            KeyCode::Char('p') if ctrl => self.mv(-1),
            KeyCode::Char('d') if ctrl => self.mv((self.height / 2).max(1) as isize),
            KeyCode::Char('u') if ctrl => self.mv(-((self.height / 2).max(1) as isize)),
            KeyCode::PageDown => self.mv(self.height as isize),
            KeyCode::PageUp => self.mv(-(self.height as isize)),
            KeyCode::Char('g') if !g => self.pending_g = true,
            KeyCode::Char('g') | KeyCode::Home => self.sel = 0,
            KeyCode::Char('G') | KeyCode::End => self.sel = self.entries.len().saturating_sub(1),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.up(),
            KeyCode::Char('~') => {
                if let Some(h) =
                    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
                {
                    self.enter(PathBuf::from(h));
                }
            }
            KeyCode::Char('.') => {
                self.hidden = !self.hidden;
                self.reload();
            }
            KeyCode::Char('a') => {
                self.all = !self.all;
                self.reload();
            }
            KeyCode::Char('/') => self.filter = Some(Input::default().cancel_on_empty()),
            KeyCode::Char('i' | 's' | 'n') if self.save => self.naming = true,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                let e = self.entries.get(self.sel)?;
                if e.dir {
                    if e.name == ".." {
                        self.up();
                    } else {
                        let d = self.dir.join(&e.name);
                        self.enter(d);
                    }
                } else if self.save {
                    self.name.set(&e.name);
                    self.naming = true;
                } else {
                    return Some(Ok(Chosen::Open(self.dir.join(&e.name))));
                }
            }
            _ => {}
        }
        None
    }
}
