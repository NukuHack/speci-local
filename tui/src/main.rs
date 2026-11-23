//! speci-tui: the terminal edition of speci-local. Keyboard only, vim-style.

#![warn(
    clippy::pedantic,
    clippy::nursery,
    clippy::let_unit_value,
    clippy::print_stdout,
    clippy::print_stderr,
    unsafe_code,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names, // x/y/w/h geometry and the vim engine's text/pos
    clippy::too_long_first_doc_paragraph,
    clippy::wildcard_imports,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::struct_excessive_bools,
    clippy::significant_drop_tightening,
    clippy::implicit_hasher,
    clippy::similar_names,
    clippy::too_many_lines
)]
#![warn(
    missing_debug_implementations,
    unused_qualifications,
    rust_2018_idioms,
    rust_2021_compatibility,
    rust_2024_compatibility
)]
// the tests drive the app with scripted keys and may unwrap / panic freely
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::print_stdout
    )
)]

mod app;
mod ascii;
mod clip;
mod code;
mod editor;
mod files;
mod highlight;
mod input;
mod nslayout;
mod stuki;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod vim;

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "speci-tui: specifications, structograms and C# in the terminal

Usage: speci-tui [OPTIONS] [FILE.sp | FILE.st]

Options:
  --light        light colors (default: dark; SPECI_THEME=light works too)
  --dark         dark colors
  --ascii        draw with 7-bit ASCII only (SPECI_ASCII=1 works too; also used
                 automatically when the locale says the terminal is not UTF-8)
  --utf8         force Unicode drawing
  -h, --help     this text
  -V, --version  version

Press ? (or F1) inside for the keys. Everything works from the keyboard.";

fn restore() {
    let mut out = io::stdout();
    let _ = execute!(
        out,
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
    let _ = out.flush();
}

fn main() -> io::Result<()> {
    let mut dark = std::env::var("SPECI_THEME").map_or(true, |v| v != "light");
    let mut file: Option<PathBuf> = None;
    let mut plain = std::env::var("SPECI_ASCII").is_ok_and(|v| !v.is_empty() && v != "0")
        || ascii::locale_is_not_utf8();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                writeln!(io::stdout(), "{USAGE}")?;
                return Ok(());
            }
            "-V" | "--version" => {
                writeln!(io::stdout(), "speci-tui {}", env!("CARGO_PKG_VERSION"))?;
                return Ok(());
            }
            "--light" => dark = false,
            "--dark" => dark = true,
            "--ascii" => plain = true,
            "--utf8" => plain = false,
            f if f.starts_with('-') => {
                writeln!(io::stderr(), "unknown option {f}\n\n{USAGE}")?;
                std::process::exit(2);
            }
            f => file = Some(PathBuf::from(f)),
        }
    }
    ascii::set(plain);
    let mut app = app::App::new(dark);
    if let Some(f) = file {
        app.load(&f);
    }

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |i| {
        restore();
        hook(i);
    }));
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut bar = false;
    let res = (|| -> io::Result<()> {
        loop {
            app.tick();
            if std::mem::take(&mut app.clear_screen) {
                // a fresh terminal has an empty "previous screen", so everything is sent
                // again; `Terminal::clear` would ask the terminal for the cursor position
                // and wait for the answer, which some terminals never give
                execute!(io::stdout(), Clear(ClearType::All))?;
                term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            }
            term.draw(|f| ui::draw(f, &mut app))?;
            let want_bar = matches!(app.tool, app::Tool::Spec | app::Tool::Code) && app.typing()
                || matches!(app.tool, app::Tool::Stuki) && app.stuki.edit.is_some();
            if want_bar != bar {
                bar = want_bar;
                execute!(
                    io::stdout(),
                    if bar {
                        SetCursorStyle::SteadyBar
                    } else {
                        SetCursorStyle::SteadyBlock
                    }
                )?;
            }
            if app.quit {
                return Ok(());
            }
            let wait = if app.wants_tick() {
                Duration::from_millis(30)
            } else {
                Duration::from_millis(250)
            };
            if event::poll(wait)? {
                loop {
                    match event::read()? {
                        Event::Key(k) => app.on_key(k),
                        Event::Paste(s) => app.on_paste(&s),
                        _ => {}
                    }
                    if app.quit || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
        }
    })();
    restore();
    res
}
