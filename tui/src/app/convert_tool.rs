//! The link and conversion tools: `SPECI1:` / `STUKI1:` lines <-> progalap.elte.hu links <-> their
//! content, the web link of the project, and C# → structograms. Reached with `Space u` (a menu)
//! or the `:url`, `:toline`, `:tourl`, `:decode`, `:csurl`, `:csline`, `:weblink`, `:openweb`
//! and `:fromcode` commands.

use super::{App, Overlay, Pending, Tool};
use crate::clip;
use crate::code;
use crossterm::event::{KeyCode, KeyEvent};
use speci_local::convert::{self, Op};
use speci_local::file::to_web_link;
use speci_local::stuki::format as stformat;
use speci_local::stuki::from_csharp::from_csharp;

/// The keys of the `Space u` menu, with what they do.
pub const MENU: [(char, Op, &str); 6] = [
    ('a', Op::Auto, "line <-> link: guess from the clipboard"),
    (
        'l',
        Op::ToUrl,
        "line / specification text / .st JSON → web editor link",
    ),
    ('u', Op::ToLine, "web editor link → SPECI1: / STUKI1: line"),
    (
        'd',
        Op::Decode,
        "web editor link → specification text / structogram JSON",
    ),
    ('c', Op::CsToUrl, "C# program → Stuki web editor link"),
    ('C', Op::CsToLine, "C# program → STUKI1: line"),
];

/// Is the word the source of a conversion (a link or a compact line)?
fn is_source(w: &str) -> bool {
    convert::is_line(w) || w.contains("data=")
}

impl App {
    /// Runs a conversion. The input is `arg` when given, the C# program for the C# conversions,
    /// else the clipboard (without a clipboard tool the terminal's paste is waited for).
    /// The result goes to the clipboard, and for `:decode … file` also to the file.
    pub fn convert_run(&mut self, op: Op, arg: &str) {
        let mut file = None;
        let mut input = arg.trim().to_string();
        if op == Op::Decode && !input.is_empty() {
            // `:decode URL out.txt`, or `:decode out.txt` for a link on the clipboard
            let words: Vec<&str> = input.split_whitespace().collect();
            if words.len() > 1 || !is_source(words[0]) {
                file = words
                    .last()
                    .filter(|w| !is_source(w))
                    .map(|w| (*w).to_string());
                input = words
                    .iter()
                    .copied()
                    .filter(|w| is_source(w))
                    .collect::<Vec<_>>()
                    .join("");
            }
        }
        if input.is_empty() {
            if op.wants_csharp() {
                self.sync();
                input = self.code.text();
            } else if let Some(t) = clip::paste() {
                input = t;
            } else {
                self.overlay = Overlay::Convert(Some(op));
                return;
            }
        }
        self.convert_text(op, &input, file.as_deref());
    }

    /// The conversion itself, with the input at hand.
    pub fn convert_text(&mut self, op: Op, input: &str, file: Option<&str>) {
        match convert::run(op, input) {
            Ok(c) => {
                let how = clip::copy(&c.text);
                let mut msg = format!(
                    "{} → copied {} characters via {how}",
                    c.note,
                    c.text.chars().count()
                );
                if let Some(f) = file {
                    match std::fs::write(f, &c.text) {
                        Ok(()) => msg += &format!("; wrote {f}"),
                        Err(e) => msg += &format!("; could not write {f}: {e}"),
                    }
                }
                self.say(msg);
            }
            Err(e) => self.say(e),
        }
    }

    /// The `Space u` menu.
    pub(super) fn convert_menu_key(&mut self, k: KeyEvent) {
        let KeyCode::Char(c) = k.code else { return };
        match c {
            'w' => self.web_link(false),
            'o' => self.web_link(true),
            c => {
                if let Some((_, op, _)) = MENU.iter().find(|m| m.0 == c) {
                    self.convert_run(*op, "");
                } else {
                    self.overlay = Overlay::Convert(None);
                }
            }
        }
    }

    /// The link that opens the current tool's project in the ELTE web editor: the specification
    /// editor for the Specification tool, Stuki for the Structogram tool, and for the Code tool
    /// the program as structograms.
    pub fn web_link(&mut self, open: bool) {
        self.sync();
        let url = match self.tool {
            Tool::Spec => to_web_link(&self.doc),
            Tool::Stuki => stformat::to_web_link(&self.stuki.program),
            Tool::Code => match from_csharp(&self.code.text()) {
                Ok(c) => stformat::to_web_link(&c.program),
                Err(e) => {
                    return self.say(format!("Can't make a Stuki link from the program: {e}"));
                }
            },
        };
        if open && code::open_browser(&url) {
            self.say("Opened the project in the web editor");
            return;
        }
        let how = clip::copy(&url);
        self.say(if open {
            format!("No browser found: copied the web editor link via {how}")
        } else {
            format!(
                "Copied the web editor link via {how} ({} characters)",
                url.len()
            )
        });
    }

    /// Code tool: the C# program becomes the structograms (the Structogram tool opens).
    pub fn code_to_stuki(&mut self) {
        match from_csharp(&self.code.text()) {
            Ok(c) => {
                let msg = format!(
                    "Structograms built from the C# program ({})",
                    c.methods.join(", ")
                );
                self.tool = Tool::Stuki;
                self.request(Pending::FromSpec(c.program, msg));
            }
            Err(e) => {
                self.code.status = format!("Can't make structograms from the program: {e}");
                self.say(self.code.status.clone());
            }
        }
    }
}
