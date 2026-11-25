//! The Code tool's part of the app: converting the project to C#, running it with dotnet, and
//! the keys of its three panes (program, console input, output).

use super::{App, EdReq, LineKind, Overlay, Pane, Pending, Tool};
use crate::clip;
use crate::code::{self, CodePane, Source};
use crate::editor::Editor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use speci_local::csharp::{self, ConvertError};
use std::fmt::Write;

impl App {
    /// The selected test case's text.
    pub(super) fn selected_test(&self) -> String {
        self.doc
            .tests
            .get(self.current)
            .cloned()
            .unwrap_or_default()
    }

    /// The panes `Tab` goes through: the console input only exists in the console mode.
    pub fn code_panes(&self) -> Vec<CodePane> {
        let mut v = vec![CodePane::Editor];
        if self.code.source == Source::Console {
            v.push(CodePane::Input);
        }
        v.push(CodePane::Output);
        v
    }

    pub(super) fn code_focus(&mut self, p: CodePane) {
        if !self.code_panes().contains(&p) {
            return;
        }
        self.leave_insert();
        self.code.focus = p;
    }

    pub(super) fn code_cycle(&mut self, fwd: bool) {
        let panes = self.code_panes();
        let i = panes
            .iter()
            .position(|p| *p == self.code.focus)
            .unwrap_or(0);
        let n = panes.len();
        self.code_focus(panes[if fwd { (i + 1) % n } else { (i + n - 1) % n }]);
    }

    pub(super) fn code_window_key(&mut self, c: char) {
        match c {
            'h' | 'k' | 'W' | 'p' => self.code_cycle(false),
            'l' | 'j' | 'w' => self.code_cycle(true),
            '1' => self.code_focus(CodePane::Editor),
            '2' => self.code_focus(CodePane::Input),
            '3' => self.code_focus(CodePane::Output),
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

    /// The editor a search or `:N` goes to.
    pub(super) fn search_editor(&mut self) -> &mut Editor {
        if self.tool == Tool::Code {
            if self.code.focus == CodePane::Input {
                &mut self.code.input
            } else {
                &mut self.code.ed
            }
        } else if self.focus == Pane::Data {
            self.data()
        } else {
            &mut self.spec_ed
        }
    }

    // ------------------------------------------------------------ the program

    /// Visiting the tool: the first time it writes the program; a changed mode or test case
    /// writes it again as long as it was not edited.
    pub(super) fn code_housekeeping(&mut self) {
        let key = (self.code.source, self.current);
        if !self.code.tried {
            self.code.tried = true;
            if self.code.is_empty() {
                self.code_convert_now();
                // only looking at the tool is not a change of the project
                self.code.mark_saved();
            }
            self.code.key = Some(key);
            return;
        }
        if self.code.key != Some(key) {
            self.code.key = Some(key);
            if self.code.unedited() {
                self.code_convert_now();
            }
        }
    }

    /// Writes the program from the specification and the structograms (asking first when the
    /// program was edited).
    pub fn code_convert(&mut self) {
        if self.code.edited() {
            self.overlay = Overlay::Confirm(Pending::Convert);
        } else {
            self.code_convert_now();
        }
    }

    pub(super) fn code_convert_now(&mut self) {
        self.sync();
        let test = self.selected_test();
        match csharp::convert_opts(
            &self.doc.spec,
            &self.doc.title,
            &self.stuki.program,
            &test,
            self.code.source.inputs(),
            &self.code.layout,
            self.code.functions,
        ) {
            Ok(c) => {
                let from = if c.own_structograms {
                    "the specification (and From spec's structograms)"
                } else {
                    "the specification and the structograms"
                };
                let notes = c.notes.len();
                self.code.apply(c);
                self.code.status = format!(
                    "Program written from {from}{}",
                    if notes > 0 {
                        format!("; {notes} thing(s) to check (// TODO lines)")
                    } else {
                        String::new()
                    }
                );
            }
            Err(ConvertError::Spec(at)) => {
                self.code.status = format!("Fix the specification first{at}");
            }
            Err(ConvertError::Test(e)) => {
                self.code.status = format!("Can't convert (test case {}): {e}", self.current + 1);
            }
        }
    }

    pub fn code_set_source(&mut self, s: Source) {
        self.code.source = s;
        if s == Source::Test && self.code.focus == CodePane::Input {
            self.code.focus = CodePane::Editor;
        }
        self.code.key = Some((s, self.current));
        if self.code.unedited() {
            self.code_convert_now();
        } else if !self.code.is_empty() {
            self.code.status = format!(
                "Reading {}: your program was kept, Space f writes it again",
                s.label()
            );
        }
    }

    /// Input, Process and Output as functions of their own, or not.
    pub fn code_set_functions(&mut self, on: bool) {
        self.code.functions = on;
        if self.code.unedited() {
            self.code_convert_now();
        } else if !self.code.is_empty() {
            self.code.status = format!(
                "Functions {}: your program was kept, Space f writes it again",
                if on { "on" } else { "off" }
            );
        } else {
            self.code.status = format!("Functions {}", if on { "on" } else { "off" });
        }
    }

    /// The input of the selected test case, as the generated program reads it.
    pub(super) fn code_fill(&mut self) {
        let test = self.selected_test();
        match speci_local::parse(&self.doc.spec)
            .map_err(|d| d[0].title.clone())
            .and_then(|s| csharp::console_input_with(&s, &test, &self.code.layout))
        {
            Ok(text) => {
                self.code.input.set_text(&text);
                self.code.input.p = 0;
                self.code.status = format!("Input of test {} filled in", self.current + 1);
            }
            Err(e) => self.code.status = format!("No input from the test case: {e}"),
        }
    }

    /// What the conversion could not translate or guessed, listed in the output pane.
    pub(super) fn code_notes(&mut self) {
        if self.code.notes.is_empty() {
            self.say("The conversion has nothing to point out");
            return;
        }
        let mut text = String::from("Things to check in the program:\n");
        for n in &self.code.notes {
            let _ = writeln!(text, "- {n}");
        }
        self.code.output = text;
        self.code.out_scroll = 0;
        self.code.run_status = "notes".into();
    }

    /// A link that opens the program in carbon.now.sh: copied, or opened in the browser.
    pub(super) fn code_carbon(&mut self, open: bool) {
        let url = csharp::carbon_link(&self.code.text());
        if open && code::open_browser(&url) {
            self.code.status = "Opened the program in Carbon".into();
            return;
        }
        let how = clip::copy(&url);
        self.code.status = if open {
            format!("No browser found: copied the Carbon link via {how}")
        } else {
            format!("Copied a Carbon link via {how}")
        };
    }

    /// Builds and runs the program in the background.
    pub fn code_run(&mut self) {
        if self.code.running() {
            self.say("Already running: Space x stops it");
        } else if self.code.is_empty() {
            self.say("Nothing to run: Space f writes the program");
        } else {
            self.sync();
            self.code.start();
        }
    }

    // ------------------------------------------------------------ keys

    pub(super) fn code_key(&mut self, k: KeyEvent) {
        let p = self.code.focus;
        if p == CodePane::Output {
            return self.code_output_key(k);
        }
        // a running program takes the line under the cursor of the input box as its input
        if p == CodePane::Input
            && k.code == KeyCode::Enter
            && self.code.running()
            && !self.vim.in_insert()
        {
            let line = self.code.input.current_line();
            if !self.code.send_line(line.trim_end()) {
                self.say("The program is not taking input");
            }
            let down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
            self.code.input.key(&mut self.vim, down);
            return;
        }
        let e = if p == CodePane::Input {
            &mut self.code.input
        } else {
            &mut self.code.ed
        };
        let out = e.key(&mut self.vim, k);
        let hint = std::mem::take(&mut e.hint);
        match out.req {
            Some(EdReq::Command) => self.open_line(LineKind::Command),
            Some(EdReq::Search) => self.open_line(LineKind::Search),
            None => {}
        }
        if !hint.is_empty() {
            self.say(hint);
        }
    }

    /// Scrolling the output.
    fn code_output_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let half = (self.code.out_h / 2).max(1);
        let last = self.code.out_len.saturating_sub(1);
        let c = &mut self.code;
        match (k.code, ctrl) {
            (KeyCode::Char('j') | KeyCode::Down, false) => c.out_scroll += 1,
            (KeyCode::Char('k') | KeyCode::Up, false) => {
                c.out_scroll = c.out_scroll.saturating_sub(1);
            }
            (KeyCode::Char('d'), true) | (KeyCode::PageDown | KeyCode::Char(' '), _) => {
                c.out_scroll += half;
            }
            (KeyCode::Char('u'), true) | (KeyCode::PageUp, _) => {
                c.out_scroll = c.out_scroll.saturating_sub(half);
            }
            (KeyCode::Char('g'), false) | (KeyCode::Home, _) => c.out_scroll = 0,
            (KeyCode::Char('G'), false) | (KeyCode::End, _) => c.out_scroll = last,
            (KeyCode::Char(':'), false) => return self.open_line(LineKind::Command),
            (KeyCode::Enter, _) => {
                // jump to the first line the compiler complained about
                if let Some(&l) = self.code.ed.errors.first() {
                    self.code_focus(CodePane::Editor);
                    self.code.ed.goto(l, 1);
                }
            }
            _ => {}
        }
        self.code.out_scroll = self.code.out_scroll.min(last);
    }
}
