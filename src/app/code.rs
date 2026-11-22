//! The Code tab: one C# editor. "From spec + stuki" writes the program from the specification
//! and the structograms; Run builds and runs it with the `dotnet` on the PATH (Linux, Windows,
//! macOS: whatever finds `dotnet`).

use super::App;
use super::theme::{card, pane_title};
use crate::csharp::{self, Inputs};
use crate::editor::{Lang, code_editor, cursor_of};
use crate::platform;
use crate::ui::{ACCENT, I_CLOSE, I_RUN, I_WAND, I_WARN};
use eframe::egui::{self, Id, Margin, RichText};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Where the program gets its input.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum Source {
    /// The values of the selected test case, written into the program.
    #[default]
    Test,
    /// Read from the console.
    Console,
}

impl From<Source> for Inputs {
    fn from(s: Source) -> Inputs {
        match s {
            Source::Test => Inputs::Test,
            Source::Console => Inputs::Console,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Code {
    pub text: String,
    pub source: Source,
    /// What the last conversion wrote: the text is "unedited" while it still equals this.
    pub(super) generated: String,
    /// The text the program reads in the console mode.
    pub stdin: String,
    /// The program as it is in the saved file.
    saved: String,
    #[serde(skip)]
    pub output: String,
    #[serde(skip)]
    pub status: String,
    /// Things the conversion could not translate or guessed.
    #[serde(skip)]
    pub(super) notes: Vec<String>,
    /// Lines of the text with a compiler error (1-based).
    #[serde(skip)]
    pub(super) errors: Vec<usize>,
    /// The text `errors` belong to.
    #[serde(skip)]
    ran: String,
    #[serde(skip)]
    pub(super) run: Option<Run>,
    /// Ask before a conversion replaces text that was edited.
    #[serde(skip)]
    pub(super) confirm: bool,
    /// The first visit of the tab converts once.
    #[serde(skip)]
    tried: bool,
    /// A line to send to the running program.
    #[serde(skip)]
    send: String,
    /// (source, test case) the text was last made for.
    #[serde(skip)]
    key: Option<(Source, usize)>,
}

/// A build-and-run in the background.
pub(super) struct Run {
    rx: mpsc::Receiver<Msg>,
    stop: Arc<AtomicBool>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    started: Instant,
}

enum Msg {
    /// A piece of the program's output.
    Out(String),
    /// The build failed: its messages.
    Build(String),
    /// The end, with a summary.
    Done(String),
}

fn code_editor_id() -> Id {
    Id::unique("code-editor")
}

impl Code {
    /// Changed since the file was saved or opened?
    pub(super) fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub(super) fn mark_saved(&mut self) {
        self.saved = self.text.clone();
    }
}

impl App {
    /// Puts the program of a project that was opened (or a new one) in the editor. A program
    /// that came from a file counts as edited: it is never replaced without asking.
    pub(super) fn reset_code(&mut self, text: Option<String>) {
        self.code_stop();
        let text = text.unwrap_or_default();
        self.code.saved = text.clone();
        self.code.text = text;
        self.code.generated.clear();
        self.code.output.clear();
        self.code.errors.clear();
        self.code.notes.clear();
        self.code.status.clear();
        self.code.confirm = false;
        // an empty editor is filled from the specification on the first visit of the tab
        self.code.tried = false;
    }

    /// The action bar of the Code tab.
    pub(super) fn code_menu(&mut self, ui: &mut egui::Ui) {
        let mut convert = false;
        ui.horizontal_wrapped(|ui| {
            self.vim_toggle(ui);
            ui.separator();
            if ui
                .button(format!("{I_WAND}  From spec + stuki"))
                .on_hover_text(
                    "Write the C# program from the specification and the structograms\n(the Stuki tab; if it is empty, the structograms From spec would build)",
                )
                .clicked()
            {
                convert = true;
            }
            ui.separator();
            ui.selectable_value(&mut self.code.source, Source::Test, "Test values")
                .on_hover_text(
                    "The values of the selected test case are written into the program, which prints its results",
                );
            ui.selectable_value(&mut self.code.source, Source::Console, "Console")
                .on_hover_text(
                    "The program reads its input from the console and prints its results",
                );
            if !self.doc.tests.is_empty() {
                egui::ComboBox::from_id_salt("code-test")
                    .selected_text(format!("Test {}", self.current + 1))
                    .show_ui(ui, |ui| {
                        for k in 0..self.doc.tests.len() {
                            ui.selectable_value(&mut self.current, k, format!("Test {}", k + 1));
                        }
                    })
                    .response
                    .on_hover_text("The test case the program is made for");
            }
            ui.separator();
            if self.code.run.is_some() {
                if ui
                    .button(format!("{I_CLOSE}  Stop"))
                    .on_hover_text("Stop the program")
                    .clicked()
                {
                    self.code_stop();
                }
            } else if ui
                .add_enabled(
                    !platform::MOBILE,
                    egui::Button::new(format!("{I_RUN}  Run")),
                )
                .on_hover_text(platform::shortcut(if platform::MOBILE {
                    "Running needs the .NET SDK on the computer"
                } else {
                    "Build and run the program with dotnet (Ctrl+Enter)"
                }))
                .clicked()
            {
                self.code_run(ui.ctx());
            }
        });
        // a changed source or test case rewrites the text, as long as it was not edited
        let key = (self.code.source, self.current);
        if self.code.key != Some(key) {
            let first = self.code.key.is_none();
            self.code.key = Some(key);
            if !first && !self.code.text.is_empty() && self.code.text == self.code.generated {
                self.code_convert();
            }
        }
        if convert {
            if !self.code.text.is_empty() && self.code.text != self.code.generated {
                self.code.confirm = true;
            } else {
                self.code_convert();
            }
        }
    }

    /// Writes the program from the specification and the structograms.
    pub(super) fn code_convert(&mut self) {
        let spec = match crate::parser::parse(&self.doc.spec) {
            Ok(s) => s,
            Err(d) => {
                let at = match d.first() {
                    Some(d) if d.line > 0 => format!(" (line {}: {})", d.line, d.title),
                    Some(d) => format!(" ({})", d.title),
                    None => String::new(),
                };
                self.code.status =
                    format!("Fix the specification on the Specification tab first{at}");
                return;
            }
        };
        let own = is_blank(&self.stuki.program);
        let program = if own {
            crate::stuki::from_spec::from_spec(&spec, &self.doc.title).program
        } else {
            self.stuki.program.clone()
        };
        let test = self
            .doc
            .tests
            .get(self.current)
            .cloned()
            .unwrap_or_default();
        match csharp::generate(&spec, &program, self.code.source.into(), &test) {
            Ok(out) => {
                self.code.text = out.code.clone();
                self.code.generated = out.code;
                self.code.notes = out.notes;
                self.code.errors.clear();
                if self.code.source == Source::Console {
                    match csharp::console_input(&spec, &test) {
                        Ok(text) => self.code.stdin = text,
                        Err(e) => self.code.status = format!("No input from the test case: {e}"),
                    }
                }
                let from = if own {
                    "the specification (the Stuki tab is empty, so From spec's structograms were used)"
                } else {
                    "the specification and the structograms"
                };
                self.code.status = format!("Program written from {from}");
            }
            Err(e) => {
                self.code.status = format!("Can't convert (test case {}): {e}", self.current + 1);
            }
        }
    }

    pub(super) fn code_stop(&mut self) {
        if let Some(r) = &self.code.run {
            r.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Builds and runs the program in the background.
    pub(super) fn code_run(&mut self, ctx: &egui::Context) {
        if platform::MOBILE {
            self.code.status = "Running needs the .NET SDK on a computer".into();
            return;
        }
        if self.code.run.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stdin = Arc::new(Mutex::new(None));
        let console = self.code.source == Source::Console;
        let job = Job {
            text: self.code.text.clone(),
            input: if console {
                self.code.stdin.clone()
            } else {
                String::new()
            },
            keep_stdin: console,
            tx,
            stop: stop.clone(),
            stdin: stdin.clone(),
            ctx: ctx.clone(),
        };
        self.code.ran = self.code.text.clone();
        self.code.output.clear();
        self.code.errors.clear();
        self.code.status = "Building…".into();
        std::thread::spawn(move || job.go());
        self.code.run = Some(Run {
            rx,
            stop,
            stdin,
            started: Instant::now(),
        });
    }

    /// Takes what the background run has produced.
    pub(super) fn poll_code(&mut self, ctx: &egui::Context) {
        let Some(run) = &self.code.run else {
            return;
        };
        let mut done = None;
        while let Ok(m) = run.rx.try_recv() {
            match m {
                Msg::Out(s) => self.code.output.push_str(&s),
                Msg::Build(s) => {
                    self.code.errors = error_lines(&s);
                    self.code.output.push_str(&s);
                }
                Msg::Done(s) => done = Some(s),
            }
        }
        match done {
            Some(s) => {
                self.code.status = s;
                self.code.run = None;
            }
            None => {
                let secs = run.started.elapsed().as_secs_f32();
                self.code.status = format!("Running… {secs:.0} s");
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
    }

    /// The editor and, below it, the output.
    pub(super) fn code_layout(&mut self, ui: &mut egui::Ui) {
        if !self.code.tried {
            self.code.tried = true;
            if self.code.text.is_empty() {
                self.code_convert();
                // only looking at the tab is not a change of the project
                self.code.mark_saved();
            }
        }
        let pane = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(Margin::same(10));
        egui::Panel::bottom("code-io")
            .frame(pane)
            .resizable(true)
            .default_size(if self.narrow { 220.0 } else { 260.0 })
            .show(ui, |ui| self.code_io(ui));
        egui::CentralPanel::default().frame(pane).show(ui, |ui| {
            ui.horizontal(|ui| pane_title(ui, "C#", "Code"));
            // compiler errors belong to the text they were found in
            let errors = if self.code.ran == self.code.text {
                self.code.errors.clone()
            } else {
                vec![]
            };
            card(ui).show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("code-scroll")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        code_editor(
                            ui,
                            code_editor_id(),
                            &mut self.code.text,
                            Lang::Csharp,
                            &[],
                            &errors,
                            &mut self.comp,
                            "Press “From spec + stuki” to write the program",
                            self.vim.then_some(&mut self.vim_state),
                        );
                    });
            });
        });
        self.code_confirm(ui.ctx());
    }

    fn code_confirm(&mut self, ctx: &egui::Context) {
        if !self.code.confirm {
            return;
        }
        egui::Window::new("Replace the code?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("The program was edited. Writing it again from the specification replaces your changes.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Replace").clicked() {
                        self.code.confirm = false;
                        self.code_convert();
                    }
                    if ui.button("Keep my code").clicked() {
                        self.code.confirm = false;
                    }
                });
            });
    }

    /// Output of the program, and (in the console mode) its input.
    fn code_io(&mut self, ui: &mut egui::Ui) {
        let console = self.code.source == Source::Console;
        let running = self.code.run.is_some();
        ui.horizontal(|ui| {
            pane_title(ui, I_RUN, "Output");
            if !self.code.notes.is_empty() {
                ui.label(
                    RichText::new(format!("{I_WARN} {}", self.code.notes.len())).color(ACCENT),
                )
                .on_hover_text(self.code.notes.join("\n"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.code.output.clear();
                }
            });
        });
        let width = ui.available_width();
        let two = console && width > 520.0;
        let show_output = |ui: &mut egui::Ui, code: &mut Code| {
            card(ui).show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("code-output")
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let text = if code.output.is_empty() && !running {
                            RichText::new("Nothing printed yet. Press Run (Ctrl+Enter).").weak()
                        } else {
                            RichText::new(&code.output).monospace()
                        };
                        ui.add(egui::Label::new(text).selectable(true));
                    });
            });
        };
        if two {
            ui.columns(2, |cols| {
                show_output(&mut cols[0], &mut self.code);
                self.code_input(&mut cols[1]);
            });
        } else {
            show_output(ui, &mut self.code);
        }
        if console && !two {
            ui.add_space(4.0);
            ui.collapsing("Input", |ui| self.code_input(ui));
        }
    }

    fn code_input(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Input (what the program reads)")
                .small()
                .weak(),
        );
        let h = (ui.available_height() - 34.0).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt("code-stdin")
            .max_height(h)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.code.stdin)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3),
                );
            });
        ui.horizontal(|ui| {
            if ui
                .button("Fill from test case")
                .on_hover_text(
                    "The input of the selected test case, as the generated program reads it",
                )
                .clicked()
            {
                self.code_fill_input();
            }
            if self.code.run.is_some() {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.code.send)
                        .hint_text("send a line to the program, Enter")
                        .desired_width(f32::INFINITY),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.code_send();
                    r.request_focus();
                }
            }
        });
    }

    fn code_fill_input(&mut self) {
        let test = self
            .doc
            .tests
            .get(self.current)
            .cloned()
            .unwrap_or_default();
        match crate::parser::parse(&self.doc.spec)
            .map_err(|d| d[0].title.clone())
            .and_then(|s| csharp::console_input(&s, &test))
        {
            Ok(text) => self.code.stdin = text,
            Err(e) => self.code.status = format!("No input from the test case: {e}"),
        }
    }

    /// One more line for the running program.
    fn code_send(&mut self) {
        let line = std::mem::take(&mut self.code.send);
        if let Some(run) = &self.code.run
            && let Ok(mut slot) = run.stdin.lock()
            && let Some(w) = slot.as_mut()
        {
            let _ = writeln!(w, "{line}");
            let _ = w.flush();
            self.code.output.push_str(&format!("{line}\n"));
        }
    }

    /// Status bar of the Code tab.
    pub(super) fn code_status(&mut self, ui: &mut egui::Ui) {
        let cursor = ui
            .memory(|m| m.has_focus(code_editor_id()))
            .then(|| cursor_of(ui.ctx(), code_editor_id()))
            .flatten()
            .map(|c| super::line_col(&self.code.text, c.1));
        let mode = self.vim.then(|| self.vim_state.status());
        crate::ui::status_bar(
            ui,
            self.dirty(),
            &self.code.status,
            self.path.as_deref(),
            "project",
            cursor,
            mode.as_deref(),
        );
    }

    /// Ctrl+Shift+C / the Copy button on the Code tab: the program text.
    pub(super) fn code_copy(&mut self, ctx: &egui::Context) {
        ctx.copy_text(self.code.text.clone());
        self.code.status = "Copied the C# program".into();
    }
}

/// A structogram program with nothing drawn in it (a fresh Stuki tab has one empty diagram).
fn is_blank(program: &crate::stuki::model::Program) -> bool {
    use crate::stuki::model::Kind;
    program.diagrams.iter().all(|d| {
        d.body.iter().all(|n| match &n.kind {
            Kind::Noop => true,
            Kind::Statement(t) => matches!(t.text.trim(), "" | "-"),
            _ => false,
        })
    })
}

/// The 1-based lines `Program.cs(12,5): error CS…` messages point to.
fn error_lines(build: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = build
        .lines()
        .filter(|l| l.contains(": error "))
        .filter_map(|l| {
            let rest = l.strip_prefix("Program.cs(")?;
            rest.split([',', ')']).next()?.parse().ok()
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

// ------------------------------------------------------------------------ the background job

struct Job {
    text: String,
    input: String,
    keep_stdin: bool,
    tx: mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    ctx: egui::Context,
}

const NO_DOTNET: &str = "`dotnet` was not found on the PATH. Install the .NET SDK (dotnet.microsoft.com/download), then start the app again.";

fn dotnet() -> Command {
    let mut c = Command::new("dotnet");
    c.env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    c
}

impl Job {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }

    fn go(self) {
        let end = |j: &Job, s: String| j.send(Msg::Done(s));
        // the newest SDK decides the target framework
        let major = match dotnet().arg("--version").output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .trim()
                .split('.')
                .next()
                .and_then(|m| m.parse::<u32>().ok())
                .unwrap_or(8),
            Ok(o) => {
                let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
                return end(&self, format!("`dotnet --version` failed: {e}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return end(&self, NO_DOTNET.into());
            }
            Err(e) => return end(&self, format!("Can't start dotnet: {e}")),
        };
        if major < 6 {
            return end(
                &self,
                format!("The .NET SDK is too old (version {major}): .NET 6 or newer is needed"),
            );
        }
        let dir = std::env::temp_dir().join("speci-local-csharp");
        if let Err(e) = write_project(&dir, &self.text, major) {
            return end(
                &self,
                format!("Can't write the project in {}: {e}", dir.display()),
            );
        }

        // build, so that what runs (and what Stop kills) is the program itself
        let out = dir.join("out");
        let built = dotnet()
            .arg("build")
            .arg(dir.join("speci.csproj"))
            .args(["-c", "Debug", "-v", "q", "--nologo", "-o"])
            .arg(&out)
            .stdin(Stdio::null())
            .output();
        let built = match built {
            Ok(b) => b,
            Err(e) => return end(&self, format!("Can't start dotnet: {e}")),
        };
        if !built.status.success() {
            let text = String::from_utf8_lossy(&built.stdout);
            self.send(Msg::Build(clean_build(&text, &dir)));
            return end(&self, "The program has errors".into());
        }
        if self.stop.load(Ordering::Relaxed) {
            return end(&self, "Stopped".into());
        }

        let mut child = match dotnet()
            .arg(out.join("speci.dll"))
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return end(&self, format!("Can't start the program: {e}")),
        };
        if let Some(mut w) = child.stdin.take() {
            let mut text = self.input.clone();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            let _ = w.write_all(text.as_bytes());
            let _ = w.flush();
            if self.keep_stdin {
                if let Ok(mut slot) = self.stdin.lock() {
                    *slot = Some(w);
                }
            }
        }
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut r| {
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let _ = tx.send(Msg::Out(String::from_utf8_lossy(&buf[..n]).into_owned()));
                    ctx.request_repaint();
                }
            })
        })
        .collect();

        let started = Instant::now();
        let status = wait(&mut child, &self.stop);
        if let Ok(mut slot) = self.stdin.lock() {
            *slot = None;
        }
        for r in readers {
            let _ = r.join();
        }
        let secs = started.elapsed().as_secs_f32();
        end(
            &self,
            match status {
                None => "Stopped".into(),
                Some(s) if s.success() => format!("Finished in {secs:.1} s"),
                Some(s) => format!(
                    "The program ended with an error (exit code {}) after {secs:.1} s",
                    s.code().map_or("?".into(), |c| c.to_string())
                ),
            },
        );
    }
}

/// Waits for the program; `None` when it was stopped (and killed).
fn wait(child: &mut Child, stop: &AtomicBool) -> Option<ExitStatus> {
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

/// `Program.cs(3,9): error CS…` lines without the paths around them.
fn clean_build(text: &str, dir: &Path) -> String {
    let dir = dir.display().to_string();
    let mut out = String::new();
    let mut seen = std::collections::HashSet::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let l = l
            .replace(&format!("{dir}/"), "")
            .replace(&format!("{dir}\\"), "");
        let l = match l.rfind(" [") {
            Some(i) if l.ends_with(".csproj]") => l[..i].to_string(),
            _ => l,
        };
        if seen.insert(l.clone()) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    out
}

fn write_project(dir: &Path, text: &str, major: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let proj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    <TargetFramework>net{major}.0</TargetFramework>\n    <AssemblyName>speci</AssemblyName>\n    <Nullable>disable</Nullable>\n    <ImplicitUsings>disable</ImplicitUsings>\n    <InvariantGlobalization>true</InvariantGlobalization>\n    <UseAppHost>false</UseAppHost>\n    <NuGetAudit>false</NuGetAudit>\n  </PropertyGroup>\n</Project>\n"
    );
    let write_if_changed = |name: &str, content: &str| -> std::io::Result<()> {
        let path: PathBuf = dir.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|old| old == content) {
            return Ok(());
        }
        std::fs::write(path, content)
    };
    write_if_changed("speci.csproj", &proj)?;
    write_if_changed("Program.cs", text)
}
