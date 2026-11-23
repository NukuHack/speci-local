//! The Code tab: one C# editor. "From spec + stuki" writes the program from the specification
//! and the structograms; Run builds and runs it with the `dotnet` on the PATH (Linux, Windows,
//! macOS: whatever finds `dotnet`).

use super::App;
use super::theme::{card, pane_title};
use crate::csharp::{self, Inputs};
use crate::dotnet;
use crate::editor::{Lang, code_editor, cursor_of};
use crate::platform;
use crate::ui::{ACCENT, GREEN, I_CLOSE, I_OK, I_RUN, I_WAND, I_WARN, RED};
use eframe::egui::{self, Id, Margin, RichText};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    pub(super) run: Option<dotnet::Run>,
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
    /// The expected output of the selected test case, for the specification and test it was
    /// made from (the hash).
    #[serde(skip)]
    expected: Option<(u64, Result<String, String>)>,
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
        let test = self
            .doc
            .tests
            .get(self.current)
            .cloned()
            .unwrap_or_default();
        match csharp::convert(
            &self.doc.spec,
            &self.doc.title,
            &self.stuki.program,
            &test,
            self.code.source.into(),
        ) {
            Ok(c) => {
                self.code.text = c.code.clone();
                self.code.generated = c.code;
                self.code.notes = c.notes;
                self.code.errors.clear();
                if let Some(stdin) = c.stdin {
                    self.code.stdin = stdin;
                }
                let from = if c.own_structograms {
                    "the specification (the Stuki tab is empty, so From spec's structograms were used)"
                } else {
                    "the specification and the structograms"
                };
                self.code.status = format!("Program written from {from}");
            }
            Err(csharp::ConvertError::Spec(at)) => {
                self.code.status =
                    format!("Fix the specification on the Specification tab first{at}");
            }
            Err(csharp::ConvertError::Test(e)) => {
                self.code.status = format!("Can't convert (test case {}): {e}", self.current + 1);
            }
        }
    }

    pub(super) fn code_stop(&mut self) {
        if let Some(r) = &self.code.run {
            r.stop();
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
        let console = self.code.source == Source::Console;
        let input = if console {
            self.code.stdin.clone()
        } else {
            String::new()
        };
        let wake = ctx.clone();
        self.code.ran = self.code.text.clone();
        self.code.output.clear();
        self.code.errors.clear();
        self.code.status = "Building…".into();
        self.code.run = Some(dotnet::Run::start(
            self.code.text.clone(),
            input,
            console,
            move || wake.request_repaint(),
        ));
    }

    /// Takes what the background run has produced.
    pub(super) fn poll_code(&mut self, ctx: &egui::Context) {
        let Some(run) = &self.code.run else {
            return;
        };
        let mut done = None;
        while let Some(m) = run.try_recv() {
            match m {
                dotnet::Msg::Out(s) => self.code.output.push_str(&s),
                dotnet::Msg::Build(s) => {
                    self.code.errors = dotnet::error_lines(&s);
                    self.code.output.push_str(&s);
                }
                dotnet::Msg::Done(s) => done = Some(s),
            }
        }
        match done {
            Some(s) => {
                self.code.status = s;
                self.code.run = None;
            }
            None => {
                let secs = run.elapsed().as_secs_f32();
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
        // the input and the expected output share the height
        let h = ((ui.available_height() - 34.0) * 0.5).max(40.0);
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
        // what the selected test case expects, to compare the run with
        ui.add_space(2.0);
        let verdict = self.code_verdict();
        let expected = self.expected_output();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Expected output (test {})", self.current + 1))
                    .small()
                    .weak(),
            );
            match verdict {
                Some(true) => {
                    ui.label(
                        RichText::new(format!("{I_OK} matches"))
                            .small()
                            .color(GREEN),
                    );
                }
                Some(false) => {
                    ui.label(
                        RichText::new(format!("{I_WARN} differs"))
                            .small()
                            .color(RED),
                    );
                }
                None => {}
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("code-expected")
            .auto_shrink([false, true])
            .show(ui, |ui| match expected {
                Ok(text) => {
                    ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
                }
                Err(e) => {
                    ui.label(RichText::new(e).small().weak());
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

    /// What the console program prints for the selected test case.
    pub(super) fn expected_output(&mut self) -> Result<String, String> {
        use std::hash::{Hash, Hasher};
        let test = self
            .doc
            .tests
            .get(self.current)
            .cloned()
            .unwrap_or_default();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.doc.spec, &test).hash(&mut h);
        let key = h.finish();
        if let Some((k, r)) = &self.code.expected
            && *k == key
        {
            return r.clone();
        }
        let r = crate::parser::parse(&self.doc.spec)
            .map_err(|d| format!("the specification has errors ({})", d[0].title))
            .and_then(|s| csharp::console_output(&s, &test));
        self.code.expected = Some((key, r.clone()));
        r
    }

    /// Did the last run print the expected output? `None` while there is nothing to compare.
    pub(super) fn code_verdict(&mut self) -> Option<bool> {
        if self.code.source != Source::Console
            || self.code.run.is_some()
            || self.code.output.trim().is_empty()
        {
            return None;
        }
        let expected = self.expected_output().ok()?;
        Some(csharp::same_output(&expected, &self.code.output))
    }

    /// One more line for the running program.
    fn code_send(&mut self) {
        let line = std::mem::take(&mut self.code.send);
        if let Some(run) = &self.code.run
            && run.send_line(&line)
        {
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
