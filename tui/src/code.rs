//! The Code tool: one C# editor, written from the specification and the structograms, that is
//! built and run with the `dotnet` on the PATH. In the console mode the program reads the
//! input box and the test case's expected output is shown to compare the run with.

use crate::editor::{Editor, Lang};
use speci_local::csharp::{self, Converted, Inputs};
use speci_local::dotnet::{self, Msg, Run};
use speci_local::file::CodeCfg;
use speci_local::ioplan::IoLayout;
use std::hash::{Hash, Hasher};

/// Where the program gets its input.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Source {
    /// The values of the selected test case, written into the program.
    #[default]
    Test,
    /// Read from the console.
    Console,
}

impl Source {
    pub const fn inputs(self) -> Inputs {
        match self {
            Self::Test => Inputs::Test,
            Self::Console => Inputs::Console,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Test => "test values",
            Self::Console => "console",
        }
    }

    pub const fn from_inputs(i: Inputs) -> Self {
        match i {
            Inputs::Test => Self::Test,
            Inputs::Console => Self::Console,
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Test => Self::Console,
            Self::Console => Self::Test,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodePane {
    Editor,
    /// The console input (and below it the expected output); only in the console mode.
    Input,
    Output,
}

/// Editor ids: 1 is the specification, the test cases count up from 100.
const CODE_ID: u64 = 2;
const INPUT_ID: u64 = 3;

pub struct Code {
    pub ed: Editor,
    pub input: Editor,
    pub source: Source,
    /// Input, Process and Output are functions of their own.
    pub functions: bool,
    /// Where the console program breaks its input and output into lines (read from the project
    /// file and kept there; this app has no panel to change it).
    pub layout: IoLayout,
    /// The settings as they are in the saved file.
    saved_cfg: CodeCfg,
    pub focus: CodePane,
    /// What the last conversion wrote: the program is "unedited" while it still equals this.
    generated: String,
    /// The program as it is in the saved file.
    saved: String,
    pub output: String,
    pub out_scroll: usize,
    pub out_len: usize,
    pub out_h: usize,
    /// Anything that happened: shown in the status bar.
    pub status: String,
    /// The run only (`Building…`, `Finished in 0.3 s`): shown over the output.
    pub run_status: String,
    /// Things the conversion could not translate or guessed.
    pub notes: Vec<String>,
    /// The first visit of the tool converts once.
    pub tried: bool,
    run: Option<Run>,
    /// The expected output of the selected test case, for the specification and test it was
    /// made from (the hash).
    expected: Option<(u64, Result<String, String>)>,
    /// (source, test case) the program was last made for.
    pub key: Option<(Source, usize)>,
}

impl Default for Code {
    fn default() -> Self {
        Self::new()
    }
}

impl Code {
    pub fn new() -> Self {
        Self {
            ed: Editor::new(Lang::Csharp, CODE_ID, ""),
            input: Editor::new(Lang::Plain, INPUT_ID, ""),
            source: Source::Test,
            functions: false,
            layout: IoLayout::default(),
            saved_cfg: CodeCfg::default(),
            focus: CodePane::Editor,
            generated: String::new(),
            saved: String::new(),
            output: String::new(),
            out_scroll: 0,
            out_len: 0,
            out_h: 10,
            status: String::new(),
            run_status: String::new(),
            notes: vec![],
            tried: false,
            run: None,
            expected: None,
            key: None,
        }
    }

    pub fn text(&self) -> String {
        self.ed.text()
    }

    pub const fn is_empty(&self) -> bool {
        self.ed.t.is_empty()
    }

    /// Changed since the file was saved or opened?
    pub fn dirty(&self) -> bool {
        self.ed.text() != self.saved || self.cfg() != self.saved_cfg
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.ed.text();
        self.saved_cfg = self.cfg();
    }

    /// The settings the project file keeps.
    pub fn cfg(&self) -> CodeCfg {
        CodeCfg {
            source: self.source.inputs(),
            functions: self.functions,
            layout: self.layout.clone(),
        }
    }

    /// Edited since the last conversion (or came from a file)? Then it is never replaced
    /// without asking.
    pub fn edited(&self) -> bool {
        !self.is_empty() && self.ed.text() != self.generated
    }

    /// Still exactly what the conversion wrote?
    pub fn unedited(&self) -> bool {
        !self.generated.is_empty() && self.ed.text() == self.generated
    }

    /// The program of a project that was opened (or a new one). A program that came from a
    /// file counts as edited.
    pub fn reset(&mut self, text: Option<String>, settings: Option<&str>) {
        self.stop();
        self.run = None;
        // a project without settings has the default ones
        let cfg = CodeCfg::from_line(settings);
        self.source = Source::from_inputs(cfg.source);
        self.functions = cfg.functions;
        self.layout = cfg.layout.clone();
        self.saved_cfg = cfg;
        self.key = None;
        if self.source == Source::Test && self.focus == CodePane::Input {
            self.focus = CodePane::Editor;
        }
        let text = text.unwrap_or_default();
        self.ed.set_text(&text);
        self.ed.p = 0;
        self.ed.errors.clear();
        self.saved = text;
        self.generated.clear();
        self.output.clear();
        self.out_scroll = 0;
        self.notes.clear();
        self.status.clear();
        self.run_status.clear();
        // an empty editor is filled from the specification on the first visit
        self.tried = false;
    }

    /// Puts a converted program in the editor.
    pub fn apply(&mut self, c: Converted) {
        self.ed.set_text(&c.code);
        self.ed.p = 0;
        self.ed.errors.clear();
        self.generated = c.code;
        self.notes = c.notes;
        // what was printed belongs to the program that is gone
        self.output.clear();
        self.out_scroll = 0;
        self.run_status.clear();
        if let Some(stdin) = c.stdin {
            self.input.set_text(&stdin);
            self.input.p = 0;
        }
    }

    // ------------------------------------------------------------ running

    pub const fn running(&self) -> bool {
        self.run.is_some()
    }

    pub fn stop(&self) {
        if let Some(r) = &self.run {
            r.stop();
        }
    }

    /// Builds and runs the program in the background.
    pub fn start(&mut self) {
        if self.run.is_some() {
            return;
        }
        let console = self.source == Source::Console;
        let input = if console {
            self.input.text()
        } else {
            String::new()
        };
        self.output.clear();
        self.out_scroll = 0;
        self.ed.errors.clear();
        self.status = "Building…".into();
        self.run_status = self.status.clone();
        // the terminal loop polls while a run is going: nothing to wake
        self.run = Some(Run::start(self.ed.text(), input, console, || {}));
    }

    /// Takes what the background run has produced.
    pub fn poll(&mut self) {
        let Some(run) = &self.run else { return };
        let mut done = None;
        while let Some(m) = run.try_recv() {
            match m {
                Msg::Out(s) => self.output.push_str(&s),
                Msg::Build(s) => {
                    self.ed.errors = dotnet::error_lines(&s);
                    self.output.push_str(&s);
                }
                Msg::Done(s) => done = Some(s),
            }
        }
        if let Some(s) = done {
            self.status.clone_from(&s);
            self.run_status = s;
            self.run = None;
        } else {
            self.status = format!("Running… {:.0} s", run.elapsed().as_secs_f32());
            self.run_status.clone_from(&self.status);
        }
    }

    /// One more line for the running program (echoed into the output). False when it is not
    /// taking input.
    pub fn send_line(&mut self, line: &str) -> bool {
        let sent = self.run.as_ref().is_some_and(|r| r.send_line(line));
        if sent {
            self.output.push_str(line);
            self.output.push('\n');
        }
        sent
    }

    // ------------------------------------------------------------ expected output

    /// What the console program prints for the test case `test`.
    pub fn expected(&mut self, spec: &str, test: &str) -> Result<String, String> {
        let mut h = std::hash::DefaultHasher::new();
        (spec, test).hash(&mut h);
        let key = h.finish();
        if let Some((k, r)) = &self.expected
            && *k == key
        {
            return r.clone();
        }
        let layout = self.layout.clone();
        let r = speci_local::parse(spec)
            .map_err(|d| format!("the specification has errors ({})", d[0].title))
            .and_then(|s| csharp::console_output_with(&s, test, &layout));
        self.expected = Some((key, r.clone()));
        r
    }

    /// Did the last run print the expected output? `None` while there is nothing to compare.
    pub fn verdict(&mut self, spec: &str, test: &str) -> Option<bool> {
        if self.source != Source::Console || self.run.is_some() || self.output.trim().is_empty() {
            return None;
        }
        let expected = self.expected(spec, test).ok()?;
        Some(csharp::same_output(&expected, &self.output))
    }
}

/// Open a link in the browser, as well as the system allows. False when nothing could be
/// started.
pub fn open_browser(url: &str) -> bool {
    use std::process::{Command, Stdio};
    let candidates: &[&[&str]] = if cfg!(target_os = "windows") {
        &[&["cmd", "/c", "start", ""]]
    } else if cfg!(target_os = "macos") {
        &[&["open"]]
    } else {
        &[&["xdg-open"], &["wslview"], &["sensible-browser"]]
    };
    candidates.iter().any(|c| {
        Command::new(c[0])
            .args(&c[1..])
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    })
}
