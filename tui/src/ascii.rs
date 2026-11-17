//! ASCII mode: the whole screen drawn with 7-bit characters only.
//!
//! The conversion happens inside the renderer, before ratatui works out what changed, so
//! partial redraws and scrolling stay correct (a filter on the byte stream can't know about
//! them). There are two layers:
//!
//! * [`expand`] / [`text`]: text the user reads and edits. Symbols become the spelling the
//!   language itself accepts (`∈` → ` in `, `≤` → `<=`), so what is on the screen is also valid
//!   input. These can be wider than one column, so the editor and the diagram painter measure
//!   with them.
//! * [`flatten`]: a last pass over the finished frame that maps every remaining non-ASCII cell
//!   (borders, check marks, arrows, scroll bars) to one ASCII column.

use ratatui::buffer::Buffer;
use std::borrow::Cow;
use std::cell::Cell;
use unicode_width::UnicodeWidthChar;

thread_local! {
    // the screen is drawn from one thread only
    static ON: Cell<bool> = const { Cell::new(false) };
}

pub fn on() -> bool {
    ON.with(Cell::get)
}

pub fn set(on: bool) {
    ON.with(|c| c.set(on));
}

/// Whether the locale says the terminal can't do UTF-8 (only when it says something).
pub fn locale_is_not_utf8() -> bool {
    let vars: Vec<String> = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .filter(|v| !v.is_empty())
        .collect();
    !vars.is_empty()
        && !vars.iter().any(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("utf-8") || v.contains("utf8")
        })
        && vars.iter().all(|v| v != "C.UTF-8")
}

/// What the language accepts in place of a symbol (padded so it reads next to names).
const SYMBOLS: &[(char, &str)] = &[
    ('∈', " in "),
    ('∉', " not in "),
    ('∀', "forall "),
    ('∃', "exists "),
    ('≠', "!="),
    ('≤', "<="),
    ('≥', ">="),
    ('¬', "not "),
    ('∤', "!|"),
    ('∧', " and "),
    ('∨', " or "),
    ('→', "->"),
    ('←', "<-"),
    ('↔', "<->"),
    ('⇒', "=>"),
    ('×', "*"),
    ('÷', "/"),
    ('−', "-"),
    ('–', "-"),
    ('—', "--"),
    ('„', "\""),
    ('“', "\""),
    ('”', "\""),
    ('‘', "'"),
    ('’', "'"),
    ('…', "..."),
    ('·', "."),
    ('•', "*"),
    ('°', "o"),
    ('²', "^2"),
    ('³', "^3"),
    ('∞', "inf"),
    ('ß', "ss"),
    ('\u{a0}', " "),
];

/// Letters with accents, by the plain letter they fold to.
const LETTERS: &[(&str, &str)] = &[
    ("áàâäãåāăą", "a"),
    ("ÁÀÂÄÃÅĀĂĄ", "A"),
    ("éèêëēěę", "e"),
    ("ÉÈÊËĒĚĘ", "E"),
    ("íìîïīı", "i"),
    ("ÍÌÎÏĪİ", "I"),
    ("óòôöõőō", "o"),
    ("ÓÒÔÖÕŐŌ", "O"),
    ("úùûüűūů", "u"),
    ("ÚÙÛÜŰŪŮ", "U"),
    ("ýÿ", "y"),
    ("Ý", "Y"),
    ("çćč", "c"),
    ("ÇĆČ", "C"),
    ("ñńň", "n"),
    ("ÑŃŇ", "N"),
    ("šś", "s"),
    ("ŠŚ", "S"),
    ("žźż", "z"),
    ("ŽŹŻ", "Z"),
    ("łľ", "l"),
    ("ŁĽ", "L"),
    ("řŕ", "r"),
    ("ŘŔ", "R"),
    ("ďđ", "d"),
    ("ĎĐ", "D"),
    ("ť", "t"),
    ("Ť", "T"),
];

fn fold_letter(c: char) -> Option<&'static str> {
    LETTERS
        .iter()
        .find(|(set, _)| set.contains(c))
        .map(|(_, plain)| *plain)
}

/// How a character shows in ASCII mode: `None` when it needs no change (ASCII, or the mode is
/// off). The result may be several columns wide.
pub fn expand(c: char) -> Option<&'static str> {
    if !on() || c.is_ascii() {
        return None;
    }
    Some(
        SYMBOLS
            .iter()
            .find(|(s, _)| *s == c)
            .map(|(_, a)| *a)
            .or_else(|| fold_letter(c))
            .unwrap_or("?"),
    )
}

/// A string as it shows in ASCII mode.
pub fn text(s: &str) -> Cow<'_, str> {
    if !on() || s.is_ascii() {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().fold(String::new(), |mut out, c| {
        match expand(c) {
            Some(a) => out.push_str(a),
            None => out.push(c),
        }
        out
    }))
}

/// One ASCII column for a whole-frame cell.
fn cell(c: char) -> char {
    match c {
        '─' | '━' | '┄' | '┈' | '╌' | '═' | '‒' | '‐' | '·' | '–' | '—' | '−' => {
            '-'
        }
        '│' | '┃' | '║' | '▏' | '▍' | '▌' | '╎' | '┆' => '|',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '┏' | '┓' | '┗' | '┛' | '┣' | '┫'
        | '┳' | '┻' | '╋' | '╭' | '╮' | '╰' | '╯' | '╔' | '╗' | '╚' | '╝' | '✔' | '✓' => {
            '+'
        }
        '█' | '▓' | '▒' | '░' | '■' => '#',
        '▶' | '▸' | '►' | '›' | '→' | '❯' => '>',
        '◀' | '◂' | '◄' | '‹' | '←' | '❮' => '<',
        '↑' | '▲' => '^',
        '↓' | '▼' => 'v',
        '✘' | '✗' | '✕' => 'x',
        '●' | '◆' | '•' => '*',
        '⟳' | '↻' => '~',
        '…' => '.',
        '“' | '”' | '„' => '"',
        '‘' | '’' => '\'',
        _ if c.is_whitespace() => ' ',
        _ => fold_letter(c)
            .filter(|a| a.len() == 1)
            .and_then(|a| a.chars().next())
            .unwrap_or('?'),
    }
}

/// Turn every non-ASCII cell of a finished frame into ASCII (one column each).
pub fn flatten(buf: &mut Buffer) {
    let area = buf.area;
    for y in area.top()..area.bottom() {
        let mut x = area.left();
        while x < area.right() {
            let sym = buf[(x, y)].symbol().to_string();
            let wide = sym
                .chars()
                .next()
                .map_or(1, |c| c.width().unwrap_or(1).max(1));
            if !sym.is_ascii() {
                let c = sym.chars().next().map_or(' ', cell);
                buf[(x, y)].set_symbol(c.encode_utf8(&mut [0; 4]));
                // the second column of a wide character is part of that cell
                for k in 1..wide {
                    if x + (k as u16) < area.right() {
                        buf[(x + k as u16, y)].set_symbol(" ");
                    }
                }
            }
            x += wide.max(1) as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_show_as_the_words_the_language_accepts() {
        set(true);
        assert_eq!(text("n∈N, i≤n"), "n in N, i<=n");
        assert_eq!(text("∀i∈[1..n]"), "forall i in [1..n]");
        assert_eq!(text("Páratlan számok"), "Paratlan szamok");
        assert_eq!(text("plain"), "plain");
        assert_eq!(text("日本"), "??");
        set(false);
        assert_eq!(text("n∈N"), "n∈N");
    }

    #[test]
    fn every_replacement_is_ascii() {
        set(true);
        for (c, a) in SYMBOLS {
            assert!(a.is_ascii(), "{c}");
        }
        for (_, plain) in LETTERS {
            assert!(plain.is_ascii());
        }
        set(false);
    }
}
