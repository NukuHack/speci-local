//! The application: documents, background evaluation, keys and commands.

use crate::ascii;
mod code_tool;

use crate::clip;
use crate::code::{Code, CodePane, Source};
use crate::editor::{Editor, Lang, Req as EdReq};
use crate::files::{Browser, Chosen, Kind};
use crate::input::{In, Input};
use crate::nslayout;
use crate::stuki::{Req as StReq, Stuki};
use crate::theme::Theme;
use crate::vim::Vim;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use speci_local::diag::{Diagnostic, Pane as DiagPane};
use speci_local::eval::Trace;
use speci_local::file::{
    Doc, code_of, from_clipboard, from_project, from_sp, to_compact, to_project,
};
use speci_local::parser::Spec;
use speci_local::stuki::format as stformat;
use speci_local::stuki::from_spec;
use speci_local::stuki::model::Program;
use speci_local::{Outcome, parse, run_and_trace, run_test_with, scaffold, trace_test};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const AUTO_DELAY: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Spec,
    Stuki,
    Code,
}

impl Tool {
    pub const ALL: [Self; 3] = [Self::Spec, Self::Stuki, Self::Code];

    /// The next (or previous) tool, round and round.
    pub fn step(self, fwd: bool) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[if fwd { (i + 1) % 3 } else { (i + 2) % 3 }]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Spec,
    Data,
    Eval,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Results,
    Stepper,
    StepList,
}

pub enum Pending {
    Quit,
    New,
    Open,
    Load(PathBuf),
    Import(Doc, Option<Program>),
    ImportStuki(Program),
    FromSpec(Program, String),
    /// Write the program again, replacing the edited one.
    Convert,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Command,
    Search,
}

pub enum Overlay {
    None,
    Help(usize),
    Files(Box<Browser>),
    Confirm(Pending),
    Line(LineKind, Input),
    Paste,
}

struct EvalResult {
    outcomes: Result<Vec<Outcome>, Vec<Diagnostic>>,
    trace: Option<Trace>,
}

struct Job {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<EvalResult>,
    doc: Doc,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn run_all(
    spec: &Spec,
    tests: &[String],
    traced: usize,
    known: &[(String, Outcome)],
    cancel: &AtomicBool,
) -> (Vec<Outcome>, Option<Trace>) {
    let mut trace = None;
    let outcomes = tests
        .iter()
        .enumerate()
        .map(|(i, t)| match known.iter().find(|(text, _)| text == t) {
            Some((_, o)) => {
                if i == traced {
                    trace = trace_test(spec, t, cancel);
                }
                o.clone()
            }
            None if i == traced => {
                let (o, steps) = run_and_trace(spec, t, cancel);
                trace = steps;
                o
            }
            None => run_test_with(spec, t, cancel),
        })
        .collect();
    (outcomes, trace)
}

pub fn doc_hash(d: &Doc) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    (&d.spec, &d.tests).hash(&mut h);
    h.finish()
}

pub fn sample() -> Doc {
    Doc {
        title: "Páratlan számok".into(),
        spec: "Be:\nn∈N,\nszamok∈N[1..n]\n\nKi:\ndb∈N\n\nEf:\n-\n\nUf:\ndb=DARAB(i=1..n, szamok[i] mod 2 = 1)\n".into(),
        tests: vec!["n: 5\nszamok: [3, 72, 81, 4, 5]\ndb: 3\n".into()],
    }
}

fn blank() -> Doc {
    Doc {
        title: String::new(),
        spec: "Be:\n\nKi:\n\nEf:\n-\n\nUf:\n".into(),
        tests: vec![String::new()],
    }
}

pub fn file_stem(name: &str, fallback: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() { fallback } else { name };
    name.chars()
        .map(|c| if "/\\:*?\"<>|\n".contains(c) { '_' } else { c })
        .collect()
}

pub struct App {
    pub doc: Doc,
    pub saved: Doc,
    pub path: Option<PathBuf>,
    pub current: usize,
    pub spec_ed: Editor,
    pub data_eds: Vec<Editor>,
    pub vim: Vim,
    pub tool: Tool,
    pub focus: Pane,
    pub view: View,
    pub zoom: bool,
    pub results: Vec<Option<Outcome>>,
    pub spec_diags: Vec<Diagnostic>,
    pub trace: Option<Trace>,
    pub step: usize,
    pub traced_test: usize,
    pub evaluated: Option<Doc>,
    job: Option<Job>,
    submitted: u64,
    last_change: (u64, Instant),
    pub auto: bool,
    pub status: String,
    pub status_at: Instant,
    pub stuki: Stuki,
    pub code: Code,
    pub overlay: Overlay,
    pub theme: Theme,
    pub eval_scroll: usize,
    pub eval_len: usize,
    pub eval_h: usize,
    pub scroll_to_card: bool,
    pub leader: bool,
    pub ctrl_w: bool,
    bracket: Option<char>,
    pub g_pending: bool,
    hist: Vec<String>,
    hist_i: usize,
    next_id: u64,
    pub quit: bool,
    overwrite_ok: Option<PathBuf>,
    /// Set when the next frame must repaint everything.
    pub clear_screen: bool,
}

impl App {
    pub fn new(dark: bool) -> Self {
        let doc = sample();
        let mut a = Self {
            saved: doc.clone(),
            spec_ed: Editor::new(Lang::Spec, 1, &doc.spec),
            data_eds: vec![],
            doc,
            path: None,
            current: 0,
            vim: Vim::default(),
            tool: Tool::Spec,
            focus: Pane::Spec,
            view: View::Results,
            zoom: false,
            results: vec![],
            spec_diags: vec![],
            trace: None,
            step: 0,
            traced_test: 0,
            evaluated: None,
            job: None,
            submitted: 0,
            last_change: (0, Instant::now()),
            auto: true,
            status: String::new(),
            status_at: Instant::now(),
            stuki: Stuki::default(),
            code: Code::new(),
            overlay: Overlay::None,
            theme: Theme::new(dark),
            eval_scroll: 0,
            eval_len: 0,
            eval_h: 10,
            scroll_to_card: false,
            leader: false,
            ctrl_w: false,
            bracket: None,
            g_pending: false,
            hist: vec![],
            hist_i: 0,
            next_id: 100,
            quit: false,
            overwrite_ok: None,
            clear_screen: false,
        };
        a.rebuild_editors();
        a
    }

    pub fn say(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_at = Instant::now();
    }

    fn rebuild_editors(&mut self) {
        self.spec_ed = Editor::new(Lang::Spec, 1, &self.doc.spec);
        self.data_eds = self
            .doc
            .tests
            .iter()
            .map(|t| {
                self.next_id += 1;
                Editor::new(Lang::Data, self.next_id, t)
            })
            .collect();
        self.vim.forget();
        self.current = self.current.min(self.doc.tests.len().saturating_sub(1));
    }

    pub fn dirty(&self) -> bool {
        self.doc != self.saved
    }

    /// Anything the `.sp` file holds that changed: the specification, tests or the program.
    pub fn project_dirty(&self) -> bool {
        self.dirty() || self.code.dirty()
    }

    /// The tools whose files are `.sp` projects.
    pub const fn spec_file(&self) -> bool {
        matches!(self.tool, Tool::Spec | Tool::Code)
    }

    pub fn data(&mut self) -> &mut Editor {
        let i = self.current.min(self.data_eds.len() - 1);
        &mut self.data_eds[i]
    }

    pub fn typing(&self) -> bool {
        match self.tool {
            Tool::Stuki => self.stuki.edit.is_some() || self.stuki.menu.is_some(),
            Tool::Code => {
                self.code.focus != CodePane::Output
                    && self.vim.in_insert()
                    && (self.vim.id_is(self.code.ed.id) || self.vim.id_is(self.code.input.id))
            }
            Tool::Spec => {
                self.focus != Pane::Eval
                    && self.vim.in_insert()
                    && (self.vim.id_is(self.spec_ed.id)
                        || self.data_eds.iter().any(|e| self.vim.id_is(e.id)))
            }
        }
    }

    fn leave_insert(&mut self) {
        if !self.vim.in_insert() {
            return;
        }
        let v = &mut self.vim;
        let code = [&mut self.code.ed, &mut self.code.input];
        for e in std::iter::once(&mut self.spec_ed)
            .chain(self.data_eds.iter_mut())
            .chain(code)
        {
            if v.id_is(e.id) {
                v.escape(e.id, &mut e.t, &mut e.p);
            }
        }
    }

    pub fn set_focus(&mut self, p: Pane) {
        self.leave_insert();
        self.focus = p;
    }

    pub fn set_current(&mut self, i: usize) {
        self.leave_insert();
        self.current = i.min(self.doc.tests.len() - 1);
        self.scroll_to_card = true;
        self.step = 0;
    }

    /// Copy the editors' texts into the document.
    fn sync(&mut self) {
        let s = self.spec_ed.text();
        if s != self.doc.spec {
            self.doc.spec = s;
        }
        for (i, e) in self.data_eds.iter().enumerate() {
            if i < self.doc.tests.len() {
                let t = e.text();
                if t != self.doc.tests[i] {
                    self.doc.tests[i] = t;
                }
            }
        }
    }

    // ------------------------------------------------------------ evaluation

    fn known(&self, spec: &str) -> Vec<(String, Outcome)> {
        match &self.evaluated {
            Some(ev) if ev.spec == spec && self.spec_diags.is_empty() => ev
                .tests
                .iter()
                .zip(&self.results)
                .filter_map(|(t, o)| Some((t.clone(), o.clone()?)))
                .collect(),
            _ => vec![],
        }
    }

    pub fn evaluate(&mut self) {
        self.sync();
        let doc = self.doc.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (d, c) = (doc.clone(), cancel.clone());
        let known = self.known(&doc.spec);
        let test = self.current;
        self.traced_test = test;
        let spawned = std::thread::Builder::new()
            .stack_size(speci_local::eval::EVAL_STACK)
            .spawn(move || {
                let (outcomes, trace) = match parse(&d.spec) {
                    Err(diags) => (Err(diags), None),
                    Ok(spec) => {
                        let (o, t) = run_all(&spec, &d.tests, test, &known, &c);
                        (Ok(o), t)
                    }
                };
                if !c.load(Ordering::Relaxed) {
                    let _ = tx.send(EvalResult { outcomes, trace });
                }
            });
        self.submitted = doc_hash(&doc);
        match spawned {
            Ok(_) => self.job = Some(Job { cancel, rx, doc }),
            Err(e) => self.say(format!("Could not start the evaluation: {e}")),
        }
    }

    pub const fn running(&self) -> bool {
        self.job.is_some() || self.stuki.running() || self.code.running()
    }

    pub fn tick(&mut self) {
        self.sync();
        let h = doc_hash(&self.doc);
        if h != self.last_change.0 {
            self.last_change = (h, Instant::now());
        }
        if self.current != self.traced_test && self.job.is_none() {
            self.submitted = 0;
        }
        if h != self.submitted
            && (self.auto || self.submitted == 0)
            && self.last_change.1.elapsed() >= AUTO_DELAY
        {
            self.evaluate();
        }
        self.poll_job();
        self.results.resize(self.doc.tests.len(), None);
        self.stuki.poll();
        self.stuki
            .refresh_checks(&self.doc.spec, &self.doc.tests, false);
        self.code.poll();
        if self.tool == Tool::Code {
            self.code_housekeeping();
        }
        self.refresh_errors();
        if !self.status.is_empty() && self.status_at.elapsed() > Duration::from_secs(6) {
            self.status.clear();
        }
    }

    pub fn wants_tick(&self) -> bool {
        let h = doc_hash(&self.doc);
        self.running() || (h != self.submitted && (self.auto || self.submitted == 0))
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let r = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.say("The evaluation crashed (this is a bug in speci-local)");
                self.job = None;
                return;
            }
        };
        let Some(job) = self.job.take() else { return };
        self.trace = r.trace;
        self.step = self.step.min(
            self.trace
                .as_ref()
                .map_or(0, |t| t.steps.len().saturating_sub(1)),
        );
        match r.outcomes {
            Err(d) => {
                self.spec_diags = d;
                self.results = vec![None; job.doc.tests.len()];
                self.say("The specification has errors");
            }
            Ok(o) => {
                self.spec_diags.clear();
                let ok = o.iter().filter(|o| o.pass).count();
                self.say(format!("{ok} of {} test case(s) passed", o.len()));
                self.results = o.into_iter().map(Some).collect();
            }
        }
        self.evaluated = Some(job.doc.clone());
    }

    /// Tint the lines the last evaluation complained about (only while the text is unchanged).
    fn refresh_errors(&mut self) {
        let ev = self.evaluated.as_ref();
        self.spec_ed.errors = match ev {
            Some(e) if e.spec == self.doc.spec => {
                let mut v: Vec<usize> = self
                    .spec_diags
                    .iter()
                    .filter(|d| d.pane == DiagPane::Spec && d.line > 0)
                    .map(|d| d.line)
                    .collect();
                for o in self.results.iter().flatten() {
                    v.extend(
                        o.diags
                            .iter()
                            .filter(|d| d.pane == DiagPane::Spec && d.line > 0)
                            .map(|d| d.line),
                    );
                }
                v
            }
            _ => vec![],
        };
        for (i, e) in self.data_eds.iter_mut().enumerate() {
            e.errors = match (ev, self.results.get(i)) {
                (Some(ev), Some(Some(o))) if ev.tests.get(i) == Some(&e.text()) => o
                    .diags
                    .iter()
                    .filter(|d| d.pane == DiagPane::Data && d.line > 0)
                    .map(|d| d.line)
                    .collect(),
                _ => vec![],
            };
        }
    }

    // ------------------------------------------------------------ files

    fn reset_doc(&mut self, doc: Doc, path: Option<PathBuf>) {
        self.saved = doc.clone();
        self.doc = doc;
        self.path = path;
        self.code.reset(None);
        self.current = 0;
        self.results.clear();
        self.spec_diags.clear();
        self.evaluated = None;
        self.trace = None;
        self.step = 0;
        self.job = None;
        self.submitted = 0;
        self.rebuild_editors();
        self.status.clear();
    }

    pub fn request(&mut self, p: Pending) {
        let dirty = match &p {
            Pending::Quit => self.project_dirty() || self.stuki.dirty(),
            Pending::Import(..) | Pending::New | Pending::Open | Pending::Load(_)
                if self.spec_file()
                    && !matches!(&p, Pending::Load(f) if Kind::of(f) == Kind::Stuki) =>
            {
                self.project_dirty()
            }
            _ => self.stuki.dirty(),
        };
        if dirty {
            self.overlay = Overlay::Confirm(p);
        } else {
            self.run(p);
        }
    }

    fn run(&mut self, p: Pending) {
        match p {
            Pending::Quit => self.quit = true,
            Pending::New => match self.tool {
                Tool::Spec | Tool::Code => {
                    self.reset_doc(blank(), None);
                    self.set_focus(Pane::Spec);
                }
                Tool::Stuki => {
                    self.stuki.replace(Program::default(), None, true);
                    self.stuki.status = "New structogram".into();
                }
            },
            Pending::Open => {
                let kind = if self.spec_file() {
                    Kind::Spec
                } else {
                    Kind::Stuki
                };
                let start = if self.spec_file() {
                    self.path.clone()
                } else {
                    self.stuki.path.clone()
                };
                self.overlay =
                    Overlay::Files(Box::new(Browser::new(false, kind, start.as_deref(), "")));
            }
            Pending::Load(p) => self.load(&p),
            Pending::Import(d, program) => {
                self.reset_doc(d, None);
                self.saved = Doc::default();
                // a project line carries its structograms too
                if let Some(p) = program {
                    self.stuki.replace(p, None, false);
                }
                if self.tool != Tool::Code {
                    self.tool = Tool::Spec;
                }
                self.say("Project read from the clipboard (not saved to a file yet)");
            }
            Pending::ImportStuki(p) => {
                self.stuki.replace(p, None, false);
                self.tool = Tool::Stuki;
                self.say("Structogram read from the clipboard (not saved to a file yet)");
            }
            Pending::FromSpec(p, msg) => {
                self.stuki.replace(p, None, false);
                self.say(msg);
            }
            Pending::Convert => self.code_convert_now(),
        }
    }

    pub fn load(&mut self, path: &Path) {
        if Kind::of(path) == Kind::Stuki {
            self.tool = Tool::Stuki;
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|t| stformat::from_st(&t))
            {
                Ok(p) => {
                    self.stuki.replace(p, Some(path.to_path_buf()), true);
                    self.say(format!("Opened {}", path.display()));
                }
                Err(e) => self.say(format!("Could not open {}: {e}", path.display())),
            }
            return;
        }
        if self.tool != Tool::Code {
            self.tool = Tool::Spec;
        }
        match std::fs::read_to_string(path) {
            Ok(text) => {
                // the whole project: specification, tests, structograms and the C# program
                let (project, problem) = match from_project(&text) {
                    Ok(p) => (p, None),
                    Err(e) => {
                        let (doc, code) = (from_sp(&text), code_of(&text));
                        (
                            speci_local::file::Project {
                                doc,
                                stuki: None,
                                code,
                            },
                            Some(e),
                        )
                    }
                };
                let mut doc = project.doc;
                if doc.title.is_empty() {
                    doc.title = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                }
                self.reset_doc(doc, Some(path.to_path_buf()));
                self.code.reset(project.code);
                let mut msg = format!("Opened {}", path.display());
                match (project.stuki, problem) {
                    (_, Some(e)) => msg += &format!(" ({e})"),
                    // unsaved structograms are not thrown away by opening a file
                    (Some(_), None) if self.stuki.dirty() => {
                        msg += " (its structograms were not loaded: the Structogram tool has unsaved changes)";
                    }
                    (Some(p), None) => self.stuki.replace(p, None, true),
                    (None, None) => {}
                }
                self.say(msg);
            }
            Err(e) => self.say(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// Open from the command line / `:e`, asking about unsaved changes first.
    pub fn open_path(&mut self, path: PathBuf) {
        if Kind::of(&path) == Kind::Stuki {
            self.tool = Tool::Stuki;
        } else if self.tool != Tool::Code {
            self.tool = Tool::Spec;
        }
        self.request(Pending::Load(path));
    }

    fn renamed(&self, path: &Path) -> Option<PathBuf> {
        let named = |t: &str| file_stem(t, "specification");
        let (was, now) = (named(&self.saved.title), named(&self.doc.title));
        if was == now || path.file_stem()? != was.as_str() {
            return None;
        }
        let ext = path.extension()?.to_string_lossy();
        let new = path.with_file_name(format!("{now}.{ext}"));
        (!new.exists()).then_some(new)
    }

    pub fn save(&mut self, as_new: bool) {
        match self.tool {
            Tool::Spec | Tool::Code => {
                if let (Some(p), false) = (self.path.clone(), as_new) {
                    self.sync();
                    match self.renamed(&p) {
                        Some(new) => {
                            self.save_spec_to(new.clone());
                            if self.path.as_ref() == Some(&new) {
                                let _ = std::fs::remove_file(&p);
                            }
                        }
                        None => self.save_spec_to(p),
                    }
                    return;
                }
                let name = file_stem(&self.doc.title, "specification");
                self.overlay = Overlay::Files(Box::new(Browser::new(
                    true,
                    Kind::Spec,
                    self.path.as_deref(),
                    &name,
                )));
            }
            Tool::Stuki => {
                if let (Some(p), false) = (self.stuki.path.clone(), as_new) {
                    self.save_stuki_to(p);
                    return;
                }
                let name = self.stuki.program.diagrams.first().map_or_else(
                    || "structogram".into(),
                    |d| file_stem(&d.name.text, "structogram"),
                );
                self.overlay = Overlay::Files(Box::new(Browser::new(
                    true,
                    Kind::Stuki,
                    self.stuki.path.as_deref(),
                    &name,
                )));
            }
        }
    }

    pub fn save_spec_to(&mut self, path: PathBuf) {
        self.sync();
        let path = Kind::Spec.with_ext(path);
        // the whole project; a file without structograms or program stays a plain `.sp`
        let text = to_project(&self.doc, &self.stuki.program, &self.code.text());
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.say(format!("Saved {}", path.display()));
                self.saved = self.doc.clone();
                self.code.mark_saved();
                if self.stuki.path.is_none() {
                    self.stuki.mark_saved();
                }
                self.path = Some(path);
            }
            Err(e) => self.say(format!("Could not save {}: {e}", path.display())),
        }
    }

    pub fn save_stuki_to(&mut self, path: PathBuf) {
        let path = Kind::Stuki.with_ext(path);
        match std::fs::write(&path, self.stuki.to_file()) {
            Ok(()) => {
                self.say(format!("Saved {}", path.display()));
                self.stuki.mark_saved();
                self.stuki.path = Some(path);
            }
            Err(e) => self.say(format!("Could not save {}: {e}", path.display())),
        }
    }

    pub fn copy_project(&mut self) {
        let text = match self.tool {
            Tool::Spec => {
                self.sync();
                to_compact(&self.doc, &self.stuki.program)
            }
            Tool::Stuki => stformat::to_compact(&self.stuki.program),
            Tool::Code => {
                let text = self.code.text();
                let how = clip::copy(&text);
                self.code.status = format!("Copied the C# program via {how}");
                return;
            }
        };
        let how = clip::copy(&text);
        self.say(format!(
            "Copied the whole project via {how} ({} characters)",
            text.len()
        ));
    }

    pub fn paste_project(&mut self) {
        match clip::paste() {
            Some(t) => self.import_text(&t),
            None => {
                self.overlay = Overlay::Paste;
            }
        }
    }

    pub fn import_text(&mut self, text: &str) {
        if stformat::looks_like_stuki(text) {
            match stformat::from_clipboard(text) {
                Ok(p) => {
                    self.tool = Tool::Stuki;
                    self.request(Pending::ImportStuki(p));
                }
                Err(e) => self.say(e),
            }
            return;
        }
        match from_clipboard(text) {
            Ok(project) => {
                if self.tool != Tool::Code {
                    self.tool = Tool::Spec;
                }
                self.request(Pending::Import(project.doc, project.stuki));
            }
            Err(e) => self.say(e),
        }
    }

    fn build_from_spec(&mut self) {
        self.sync();
        let spec = match parse(&self.doc.spec) {
            Ok(s) => s,
            Err(d) => {
                let at = match d.first() {
                    Some(d) if d.line > 0 => format!(" (line {}: {})", d.line, d.title),
                    Some(d) => format!(" ({})", d.title),
                    None => String::new(),
                };
                self.say(format!("Fix the specification first{at}"));
                return;
            }
        };
        let g = from_spec::from_spec(&spec, &self.doc.title);
        let mut msg = "Structograms built from the specification".to_string();
        if !g.unset.is_empty() {
            msg += &format!(
                "; the postcondition doesn't say how to compute {}",
                g.unset.join(", ")
            );
        }
        self.tool = Tool::Stuki;
        self.request(Pending::FromSpec(g.program, msg));
    }

    fn export_text(&mut self, file: Option<&str>) {
        let words = Stuki::layout_words(&self.doc.spec);
        let l = nslayout::layout(&self.stuki.program, 100, &words, None);
        let text = nslayout::to_text(&l);
        if let Some(f) = file {
            match std::fs::write(f, &text) {
                Ok(()) => self.say(format!("Wrote the picture to {f}")),
                Err(e) => self.say(format!("Could not write {f}: {e}")),
            }
        } else {
            let how = clip::copy(&text);
            self.say(format!("Copied the picture as text via {how}"));
        }
    }

    // ------------------------------------------------------------ tests

    fn new_test(&mut self) {
        self.leave_insert();
        self.next_id += 1;
        self.doc.tests.push(String::new());
        self.data_eds
            .push(Editor::new(Lang::Data, self.next_id, ""));
        self.vim.forget();
        self.set_current(self.doc.tests.len() - 1);
        self.set_focus(Pane::Data);
        self.say("New test case: `<Space>s` fills in the variable names");
    }

    fn delete_test(&mut self) {
        if self.doc.tests.len() <= 1 {
            self.say("The last test case can't be deleted (clear it instead)");
            return;
        }
        self.leave_insert();
        let i = self.current;
        self.doc.tests.remove(i);
        self.data_eds.remove(i);
        self.results.clear();
        self.evaluated = None;
        self.vim.forget();
        self.set_current(i.min(self.doc.tests.len() - 1));
        self.submitted = 0;
        self.say(format!("Deleted test case {}", i + 1));
    }

    fn scaffold_test(&mut self) {
        self.sync();
        match parse(&self.doc.spec) {
            Ok(spec) => {
                let cur = self.doc.tests[self.current].clone();
                let add = scaffold(&spec, &cur);
                if add.is_empty() {
                    self.say("Every variable is already in this test case");
                    return;
                }
                let sep = if cur.is_empty() || cur.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                let new = format!("{cur}{sep}{add}");
                self.data().set_text(&new);
                self.sync();
                self.set_focus(Pane::Data);
                self.say("Variable names inserted: fill in the values");
            }
            Err(_) => self.say("Fix the specification first"),
        }
    }

    pub fn jump_to_diag(&mut self) {
        let Some(Some(o)) = self.results.get(self.current) else {
            return;
        };
        let Some(d) = self.spec_diags.first().or_else(|| o.diags.first()).cloned() else {
            return;
        };
        if d.line == 0 {
            return;
        }
        if d.pane == DiagPane::Data {
            self.set_focus(Pane::Data);
            self.data().goto(d.line, d.col);
        } else {
            self.set_focus(Pane::Spec);
            self.spec_ed.goto(d.line, d.col);
        }
    }

    fn next_error(&mut self, fwd: bool) {
        let (ed, name) = match (self.tool, self.focus) {
            (Tool::Code, _) => (Some(&self.code.ed), "program"),
            (_, Pane::Data) => (self.data_eds.get(self.current), "test data"),
            _ => (Some(&self.spec_ed), "specification"),
        };
        let Some(ed) = ed else { return };
        let (cur, _) = ed.line_col();
        let mut lines = ed.errors.clone();
        lines.sort_unstable();
        lines.dedup();
        let t = if fwd {
            lines.iter().find(|&&l| l > cur)
        } else {
            lines.iter().rev().find(|&&l| l < cur)
        };
        match t.or_else(|| if fwd { lines.first() } else { lines.last() }) {
            Some(&l) => {
                let e = if self.tool == Tool::Code {
                    &mut self.code.ed
                } else {
                    self.search_editor()
                };
                e.goto(l, 1);
            }
            None => self.say(format!("No errors in the {name}")),
        }
    }

    // ------------------------------------------------------------ commands

    pub fn command(&mut self, line: &str) {
        let line = line.trim().trim_start_matches(':');
        let (cmd, arg) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(c, a)| (c, a.trim()));
        if let Ok(n) = cmd.parse::<usize>() {
            match self.tool {
                Tool::Spec => {
                    let e = match self.focus {
                        Pane::Data => self.data(),
                        _ => &mut self.spec_ed,
                    };
                    e.goto(n, 1);
                }
                Tool::Stuki => {}
                Tool::Code => {
                    let e = match self.code.focus {
                        CodePane::Input => &mut self.code.input,
                        _ => &mut self.code.ed,
                    };
                    e.goto(n, 1);
                }
            }
            return;
        }
        match cmd {
            "" => {}
            "q" | "quit" | "qa" | "close" => self.request(Pending::Quit),
            "q!" | "qa!" => self.quit = true,
            "w" | "write" | "up" | "update" => {
                if arg.is_empty() {
                    self.save(false);
                } else if self.spec_file() {
                    self.save_spec_to(PathBuf::from(arg));
                } else {
                    self.save_stuki_to(PathBuf::from(arg));
                }
            }
            "wq" | "x" | "xit" | "wqa" | "xa" => {
                if arg.is_empty() && self.path.is_none() && self.spec_file() {
                    self.save(false);
                    return;
                }
                if self.spec_file() || self.stuki.path.is_some() {
                    self.save(false);
                }
                if !self.project_dirty() && !self.stuki.dirty() {
                    self.quit = true;
                } else {
                    self.say("Not everything could be saved: use :w name, or :q! to discard");
                }
            }
            "sav" | "saveas" => {
                if arg.is_empty() {
                    self.save(true);
                } else if self.spec_file() {
                    self.save_spec_to(PathBuf::from(arg));
                } else {
                    self.save_stuki_to(PathBuf::from(arg));
                }
            }
            "e" | "edit" | "o" | "open" => {
                if arg.is_empty() {
                    self.request(Pending::Open);
                } else {
                    self.open_path(PathBuf::from(arg));
                }
            }
            "new" | "enew" | "n" => self.request(Pending::New),
            "run" | "eval" | "r" => match self.tool {
                Tool::Spec => self.evaluate(),
                Tool::Stuki => {
                    self.sync();
                    self.stuki
                        .refresh_checks(&self.doc.spec, &self.doc.tests, true);
                    self.say("Running the structograms on the test cases");
                }
                Tool::Code => self.code_run(),
            },
            "set" => self.set_option(arg),
            "auto" => self.set_option("auto"),
            "noauto" => self.set_option("noauto"),
            "light" | "dark" => self.set_option(cmd),
            "tnew" | "tn" if cmd == "tnew" => self.new_test(),
            "tdel" | "td" => self.delete_test(),
            "tnext" | "tnext!" => self.set_current((self.current + 1) % self.doc.tests.len()),
            "tprev" | "tp" => {
                self.set_current((self.current + self.doc.tests.len() - 1) % self.doc.tests.len());
            }
            "t" | "test" => match arg.parse::<usize>() {
                Ok(n) if n >= 1 && n <= self.doc.tests.len() => self.set_current(n - 1),
                _ => self.say(format!("There are {} test case(s)", self.doc.tests.len())),
            },
            "scaffold" | "sc" => self.scaffold_test(),
            "title" => {
                self.doc.title = arg.to_string();
                self.say(format!("Title: {arg}"));
            }
            "spec" | "specification" => self.tool = Tool::Spec,
            "stuki" | "st" => self.tool = Tool::Stuki,
            "code" | "cs" | "csharp" | "c#" => self.tool = Tool::Code,
            "convert" | "conv" => {
                self.tool = Tool::Code;
                self.code_convert();
            }
            "mode" => match arg.chars().next() {
                Some('t' | 'T') => self.code_set_source(Source::Test),
                Some('c' | 'C') => self.code_set_source(Source::Console),
                _ => self.code_set_source(self.code.source.other()),
            },
            "stop" => {
                self.code.stop();
                self.code.status = "Stopping…".into();
            }
            "fill" => self.code_fill(),
            "notes" => self.code_notes(),
            "carbon" => self.code_carbon(arg.starts_with('o')),
            "send" => {
                if !self.code.send_line(arg) {
                    self.say("The program is not running (or not taking input)");
                }
            }
            "copy" | "yank" => self.copy_project(),
            "paste" => self.paste_project(),
            "fromspec" => self.build_from_spec(),
            "export" | "picture" => self.export_text(Some(arg).filter(|a| !a.is_empty())),
            "help" | "h" | "?" => self.overlay = Overlay::Help(0),
            "view" => {
                self.view = match arg.chars().next() {
                    Some('s')
                        if arg.starts_with("step")
                            && !arg.starts_with("steps")
                            && !arg.starts_with("stepl") =>
                    {
                        View::Stepper
                    }
                    Some('l' | 'L') => View::StepList,
                    Some('s') => View::Stepper,
                    _ => View::Results,
                }
            }
            "zoom" => self.zoom = !self.zoom,
            "next" => self.next_error(true),
            _ => self.say(format!("Not a command: {cmd}  (try :help)")),
        }
    }

    fn set_option(&mut self, arg: &str) {
        for o in arg.split_whitespace() {
            match o {
                "auto" => {
                    self.auto = true;
                    self.say("Auto-evaluation on");
                }
                "noauto" => {
                    self.auto = false;
                    self.say("Auto-evaluation off: <Space>r or :run evaluates");
                }
                "light" => self.theme = Theme::new(false),
                "dark" => self.theme = Theme::new(true),
                "ascii" => self.set_ascii(true),
                "utf8" | "utf-8" | "unicode" | "noascii" => self.set_ascii(false),
                "zoom" => self.zoom = true,
                "nozoom" => self.zoom = false,
                _ => self.say(format!("Unknown option: {o}")),
            }
        }
    }

    /// Switch between Unicode and ASCII drawing; the screen is wiped so no half of a wide
    /// character is left behind.
    pub fn set_ascii(&mut self, on: bool) {
        ascii::set(on);
        self.clear_screen = true;
        self.say(if on {
            "ASCII mode: only 7-bit characters on the screen"
        } else {
            "Unicode drawing"
        });
    }

    fn toggle_theme(&mut self) {
        self.theme = Theme::new(!self.theme.dark);
    }

    // ------------------------------------------------------------ keys

    pub fn on_paste(&mut self, text: &str) {
        match &mut self.overlay {
            Overlay::Paste => {
                self.overlay = Overlay::None;
                self.import_text(text);
                return;
            }
            Overlay::Line(_, i) => return i.insert_str(text),
            Overlay::Files(b) => {
                if b.naming {
                    b.name.insert_str(text);
                }
                return;
            }
            Overlay::None => {}
            _ => return,
        }
        if self.tool == Tool::Stuki {
            if let Some(e) = &mut self.stuki.edit {
                e.input.insert_str(text);
            } else {
                self.import_text(text);
            }
            return;
        }
        let t = text.trim_start();
        if !self.vim.in_insert() && (t.starts_with("SPECI1:") || t.starts_with("STUKI1:")) {
            return self.import_text(text);
        }
        if self.tool == Tool::Code {
            if self.code.focus == CodePane::Output {
                return self.import_text(text);
            }
            let e = if self.code.focus == CodePane::Input {
                &mut self.code.input
            } else {
                &mut self.code.ed
            };
            e.paste(&mut self.vim, text);
            return;
        }
        if self.focus == Pane::Eval {
            return self.import_text(text);
        }
        let v = &mut self.vim;
        let e = if self.focus == Pane::Data {
            &mut self.data_eds[self.current]
        } else {
            &mut self.spec_ed
        };
        e.paste(v, text);
        self.sync();
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if !matches!(self.overlay, Overlay::None) {
            return self.overlay_key(k);
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // keys that work everywhere, even while typing
        match (k.code, ctrl) {
            (KeyCode::Char('s'), true) => return self.save(false),
            (KeyCode::F(5), _) => return self.command("run"),
            (KeyCode::F(1), _) => return self.overlay = Overlay::Help(0),
            (KeyCode::F(2), _) => return self.switch_tool(true),
            (KeyCode::Char('q'), true) => return self.request(Pending::Quit),
            _ => {}
        }
        if self.typing() {
            return self.pane_key(k);
        }
        if self.ctrl_w {
            self.ctrl_w = false;
            return self.window_key(k);
        }
        if self.leader {
            self.leader = false;
            return self.leader_key(k);
        }
        let idle = match self.tool {
            Tool::Spec => match self.focus {
                Pane::Eval => true,
                Pane::Spec | Pane::Data => !self.vim.pending() && !self.vim.is_visual(),
            },
            Tool::Stuki => self.stuki.keys.is_empty(),
            Tool::Code => {
                self.code.focus == CodePane::Output || !self.vim.pending() && !self.vim.is_visual()
            }
        };
        if ctrl && k.code == KeyCode::Char('w') && idle {
            self.ctrl_w = true;
            return;
        }
        if (k.code, ctrl) == (KeyCode::Char('o'), true) && idle {
            return self.request(Pending::Open);
        }
        if (k.code, ctrl) == (KeyCode::Char('n'), true) && idle && self.tool == Tool::Stuki {
            return self.request(Pending::New);
        }
        if idle && !ctrl {
            match k.code {
                KeyCode::Char(' ') => {
                    self.leader = true;
                    self.say(match self.tool {
                        Tool::Spec => "SPC: r run  f →structogram  t/d test  s scaffold  a auto  v view  w save  o open  n new  y copy  p paste  q quit  ? help",
                        Tool::Stuki => "SPC: f build from spec  c copy picture  w save  o open  n new  y copy  p paste  1/2/3 tool  q quit  ? help",
                        Tool::Code => "SPC: f from spec+stuki  m mode  r run  x stop  i fill input  b/B carbon  y copy  w save  1/2/3 tool  q quit  ? help",
                    });
                    return;
                }
                KeyCode::Char('?') => return self.overlay = Overlay::Help(0),
                KeyCode::Char(c @ (']' | '[')) if self.spec_file() => {
                    self.bracket = Some(c);
                    return;
                }
                KeyCode::Tab if self.tool == Tool::Spec => return self.cycle_pane(true),
                KeyCode::BackTab if self.tool == Tool::Spec => return self.cycle_pane(false),
                KeyCode::Tab if self.tool == Tool::Code => return self.code_cycle(true),
                KeyCode::BackTab if self.tool == Tool::Code => return self.code_cycle(false),
                KeyCode::Tab | KeyCode::BackTab => {
                    self.tool = Tool::Spec;
                    return;
                }
                _ => {}
            }
        }
        if let Some(b) = self.bracket.take()
            && idle
            && let KeyCode::Char(c) = k.code
        {
            match (b, c) {
                (']', 't') => {
                    return self.set_current((self.current + 1) % self.doc.tests.len());
                }
                ('[', 't') => {
                    return self.set_current(
                        (self.current + self.doc.tests.len() - 1) % self.doc.tests.len(),
                    );
                }
                (']', 'e') => return self.next_error(true),
                ('[', 'e') => return self.next_error(false),
                _ => {}
            }
        }
        // `gt` / `gT` switch tools
        let editing_here = match self.tool {
            Tool::Spec => self.focus != Pane::Eval,
            Tool::Code => self.code.focus != CodePane::Output,
            Tool::Stuki => false,
        };
        if editing_here
            && self.vim.pending_keys() == "g"
            && let KeyCode::Char(c @ ('t' | 'T')) = k.code
        {
            self.pane_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            self.switch_tool(c == 't');
            return;
        }
        if self.tool == Tool::Stuki
            && self.stuki.keys == ['g']
            && let KeyCode::Char(c @ ('t' | 'T')) = k.code
        {
            self.stuki.keys.clear();
            self.switch_tool(c == 't');
            return;
        }
        self.pane_key(k);
    }

    fn cycle_pane(&mut self, fwd: bool) {
        let order = [Pane::Spec, Pane::Data, Pane::Eval];
        let i = order.iter().position(|p| *p == self.focus).unwrap_or(0);
        let n = if fwd { (i + 1) % 3 } else { (i + 2) % 3 };
        self.set_focus(order[n]);
    }

    /// Next (or previous) tool.
    fn switch_tool(&mut self, fwd: bool) {
        self.leave_insert();
        self.tool = self.tool.step(fwd);
    }

    fn window_key(&mut self, k: KeyEvent) {
        let KeyCode::Char(c) = k.code else { return };
        if self.tool == Tool::Code {
            return self.code_window_key(c);
        }
        match c {
            'h' | 'H' => {
                let p = match self.focus {
                    Pane::Spec => Pane::Eval,
                    Pane::Data => Pane::Spec,
                    Pane::Eval => Pane::Data,
                };
                self.set_focus(p);
            }
            'l' | 'L' | 'w' => self.cycle_pane(true),
            'W' | 'p' => self.cycle_pane(false),
            'j' => self.set_focus(match self.focus {
                Pane::Data => Pane::Eval,
                p => p,
            }),
            'k' => self.set_focus(match self.focus {
                Pane::Eval => Pane::Data,
                p => p,
            }),
            '1' => self.set_focus(Pane::Spec),
            '2' => self.set_focus(Pane::Data),
            '3' => self.set_focus(Pane::Eval),
            'o' | 'z' => {
                self.zoom = !self.zoom;
                self.say(if self.zoom {
                    "Zoomed: Ctrl-w o restores"
                } else {
                    "Zoom off"
                });
            }
            't' => self.switch_tool(true),
            _ => {}
        }
    }

    fn leader_key(&mut self, k: KeyEvent) {
        self.status.clear();
        let KeyCode::Char(c) = k.code else { return };
        match c {
            'r' => self.command("run"),
            'x' if self.tool == Tool::Code => self.command("stop"),
            'm' if self.tool == Tool::Code => self.command("mode"),
            'i' if self.tool == Tool::Code => self.code_fill(),
            'b' if self.tool == Tool::Code => self.code_carbon(false),
            'B' if self.tool == Tool::Code => self.code_carbon(true),
            'w' => self.save(false),
            'W' => self.save(true),
            'o' => self.request(Pending::Open),
            'n' => self.request(Pending::New),
            'q' => self.request(Pending::Quit),
            'y' => self.copy_project(),
            'p' => self.paste_project(),
            'a' => self.set_option(if self.auto { "noauto" } else { "auto" }),
            't' if self.tool == Tool::Spec => self.new_test(),
            'd' if self.tool == Tool::Spec => self.delete_test(),
            's' if self.tool == Tool::Spec => self.scaffold_test(),
            'T' => self.toggle_theme(),
            'A' => self.set_ascii(!ascii::on()),
            'z' => self.zoom = !self.zoom,
            '1' => self.tool = Tool::Spec,
            '2' => self.tool = Tool::Stuki,
            '3' => self.tool = Tool::Code,
            'f' if self.tool == Tool::Code => self.code_convert(),
            'f' => self.build_from_spec(),
            'c' if self.tool == Tool::Code => self.copy_project(),
            'c' | 'e' => self.export_text(None),
            'v' if self.tool == Tool::Spec => {
                self.view = match self.view {
                    View::Results => View::Stepper,
                    View::Stepper => View::StepList,
                    View::StepList => View::Results,
                }
            }
            ':' => self.open_line(LineKind::Command),
            '?' => self.overlay = Overlay::Help(0),
            _ => self.say("Unknown leader key (SPC ? for help)"),
        }
    }

    fn open_line(&mut self, kind: LineKind) {
        let input = Input::default().cancel_on_empty();
        self.hist_i = self.hist.len();
        self.overlay = Overlay::Line(kind, input);
    }

    fn pane_key(&mut self, k: KeyEvent) {
        match self.tool {
            Tool::Stuki => {
                if self.stuki.key(k) == StReq::Command {
                    self.open_line(LineKind::Command);
                }
            }
            Tool::Code => self.code_key(k),
            Tool::Spec => match self.focus {
                Pane::Eval => self.eval_key(k),
                p => {
                    let v = &mut self.vim;
                    let e = if p == Pane::Data {
                        &mut self.data_eds[self.current]
                    } else {
                        &mut self.spec_ed
                    };
                    let out = e.key(v, k);
                    if out.changed {
                        self.sync();
                    }
                    match out.req {
                        Some(EdReq::Command) => self.open_line(LineKind::Command),
                        Some(EdReq::Search) => self.open_line(LineKind::Search),
                        None => {}
                    }
                    let h = std::mem::take(
                        &mut if p == Pane::Data {
                            &mut self.data_eds[self.current]
                        } else {
                            &mut self.spec_ed
                        }
                        .hint,
                    );
                    if !h.is_empty() {
                        self.say(h);
                    }
                }
            },
        }
    }

    fn eval_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let steps = self.trace.as_ref().map_or(0, |t| t.steps.len());
        let half = (self.eval_h / 2).max(1);
        let g = std::mem::take(&mut self.g_pending);
        let list = self.view == View::StepList;
        let stepping = self.view != View::Results;
        let mv = |me: &mut Self, d: isize| {
            if stepping && steps > 0 {
                me.step = (me.step as isize + d).clamp(0, steps as isize - 1) as usize;
            } else {
                me.eval_scroll = (me.eval_scroll as isize + d)
                    .clamp(0, me.eval_len.saturating_sub(1) as isize)
                    as usize;
            }
        };
        match (k.code, ctrl) {
            (KeyCode::Char('j') | KeyCode::Down, false) => mv(self, 1),
            (KeyCode::Char('k') | KeyCode::Up, false) => mv(self, -1),
            (KeyCode::Char('l') | KeyCode::Right, false) if self.view == View::Stepper => {
                mv(self, 1);
            }
            (KeyCode::Char('h') | KeyCode::Left, false) if self.view == View::Stepper => {
                mv(self, -1);
            }
            (KeyCode::Char('d'), true) | (KeyCode::PageDown, _) => mv(self, half as isize),
            (KeyCode::Char('u'), true) | (KeyCode::PageUp, _) => mv(self, -(half as isize)),
            (KeyCode::Char('f'), true) => mv(self, self.eval_h as isize),
            (KeyCode::Char('b'), true) => mv(self, -(self.eval_h as isize)),
            (KeyCode::Char('e'), true) => {
                self.eval_scroll = (self.eval_scroll + 1).min(self.eval_len.saturating_sub(1));
            }
            (KeyCode::Char('y'), true) => self.eval_scroll = self.eval_scroll.saturating_sub(1),
            (KeyCode::Char('g'), false) if !g => self.g_pending = true,
            (KeyCode::Char('g' | '0'), false) | (KeyCode::Home, _) => {
                if stepping {
                    self.step = 0;
                } else {
                    self.eval_scroll = 0;
                }
            }
            (KeyCode::Char('G' | '$'), false) | (KeyCode::End, _) => {
                if stepping {
                    self.step = steps.saturating_sub(1);
                } else {
                    self.eval_scroll = self.eval_len.saturating_sub(self.eval_h);
                }
            }
            (KeyCode::Char('v'), false) => {
                self.view = match self.view {
                    View::Results => View::Stepper,
                    View::Stepper => View::StepList,
                    View::StepList => View::Results,
                }
            }
            (KeyCode::Char('s'), false) => self.view = View::Stepper,
            (KeyCode::Char('S'), false) => self.view = View::StepList,
            (KeyCode::Char('r'), false) => self.view = View::Results,
            (KeyCode::Char(':'), false) => self.open_line(LineKind::Command),
            (KeyCode::Char('J'), false) => {
                self.set_current((self.current + 1) % self.doc.tests.len());
            }
            (KeyCode::Char('K'), false) => {
                self.set_current((self.current + self.doc.tests.len() - 1) % self.doc.tests.len());
            }
            (KeyCode::Enter, _) | (KeyCode::Char('e'), false) => match self.view {
                View::Results => self.jump_to_diag(),
                View::StepList => self.view = View::Stepper,
                View::Stepper => self.view = View::StepList,
            },
            (KeyCode::Esc, _) if list => self.view = View::Stepper,
            _ => {}
        }
    }

    fn overlay_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match std::mem::replace(&mut self.overlay, Overlay::None) {
            Overlay::None => {}
            Overlay::Help(mut s) => {
                let page = 15;
                match (k.code, ctrl) {
                    (KeyCode::Esc | KeyCode::Char('q' | '?'), false) => return,
                    (KeyCode::Char('j') | KeyCode::Down, false) => s += 1,
                    (KeyCode::Char('k') | KeyCode::Up, false) => s = s.saturating_sub(1),
                    (KeyCode::Char('d'), true) | (KeyCode::PageDown | KeyCode::Char(' '), _) => {
                        s += page;
                    }
                    (KeyCode::Char('u'), true) | (KeyCode::PageUp, _) => s = s.saturating_sub(page),
                    (KeyCode::Char('g'), false) | (KeyCode::Home, _) => s = 0,
                    (KeyCode::Char('G'), false) | (KeyCode::End, _) => s = usize::MAX / 2,
                    _ => {}
                }
                self.overlay = Overlay::Help(s);
            }
            Overlay::Paste => {
                if !matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.overlay = Overlay::Paste;
                }
            }
            Overlay::Confirm(Pending::Convert) => {
                if matches!(k.code, KeyCode::Char('y' | 'r')) {
                    self.code_convert_now();
                }
            }
            Overlay::Confirm(p) => match k.code {
                KeyCode::Char('s' | 'y') => {
                    let saved = match (&p, self.tool) {
                        (Pending::Quit, _) => {
                            if self.project_dirty() && self.path.is_some() {
                                self.save(false);
                            }
                            if self.stuki.dirty() && self.stuki.path.is_some() {
                                let t = std::mem::replace(&mut self.tool, Tool::Stuki);
                                self.save(false);
                                self.tool = t;
                            }
                            !self.project_dirty() && !self.stuki.dirty()
                        }
                        (_, Tool::Spec | Tool::Code)
                            if !matches!(&p, Pending::ImportStuki(_) | Pending::FromSpec(..)) =>
                        {
                            if self.path.is_some() {
                                self.save(false);
                            }
                            !self.project_dirty()
                        }
                        _ => {
                            if self.stuki.path.is_some() {
                                self.save(false);
                            }
                            !self.stuki.dirty()
                        }
                    };
                    if saved {
                        self.run(p);
                    } else {
                        self.say("Not saved yet: give it a name with :w name, then try again");
                    }
                }
                KeyCode::Char('d' | 'n') => self.run(p),
                _ => {}
            },
            Overlay::Files(mut b) => match b.key(k) {
                None => self.overlay = Overlay::Files(b),
                Some(Err(())) => {}
                Some(Ok(Chosen::Open(p))) => self.open_path(p),
                Some(Ok(Chosen::Save(p))) => {
                    if p.exists()
                        && !matches!(k.code, KeyCode::Enter if false)
                        && self.confirm_overwrite(&p)
                    {
                        self.overlay = Overlay::Files(b);
                        return;
                    }
                    if Kind::of(&p) == Kind::Stuki {
                        self.save_stuki_to(p);
                    } else {
                        self.save_spec_to(p);
                    }
                }
            },
            Overlay::Line(kind, mut input) => match input.key(k) {
                In::Cancel => {}
                In::Accept => {
                    let text = input.text();
                    match kind {
                        LineKind::Command => {
                            if !text.trim().is_empty() {
                                self.hist.push(text.clone());
                            }
                            self.command(&text);
                        }
                        LineKind::Search => {
                            if !text.is_empty() {
                                self.search_editor().set_search(&text, true);
                            }
                        }
                    }
                }
                In::Up if kind == LineKind::Command => {
                    self.hist_i = self.hist_i.saturating_sub(1);
                    if let Some(h) = self.hist.get(self.hist_i) {
                        input.set(h);
                    }
                    self.overlay = Overlay::Line(kind, input);
                }
                In::Down if kind == LineKind::Command => {
                    self.hist_i = (self.hist_i + 1).min(self.hist.len());
                    match self.hist.get(self.hist_i) {
                        Some(h) => input.set(h),
                        None => input.set(""),
                    }
                    self.overlay = Overlay::Line(kind, input);
                }
                _ => self.overlay = Overlay::Line(kind, input),
            },
        }
    }

    /// Existing files are only overwritten when the name was typed out twice: the first
    /// Enter just warns. Returns true when the save should wait.
    fn confirm_overwrite(&mut self, p: &Path) -> bool {
        if self.path.as_deref() == Some(p) || self.stuki.path.as_deref() == Some(p) {
            return false;
        }
        let key = p.to_path_buf();
        if self.overwrite_ok.as_ref() == Some(&key) {
            self.overwrite_ok = None;
            return false;
        }
        self.overwrite_ok = Some(key);
        self.say(format!(
            "{} exists: press Enter again to overwrite",
            p.display()
        ));
        true
    }
}
