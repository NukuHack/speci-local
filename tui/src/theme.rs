//! Colors. Truecolor terminals get exact colors, others the nearest of the 256-color palette.

use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

fn truecolor() -> bool {
    static T: OnceLock<bool> = OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("COLORTERM").is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"))
            || std::env::var("TERM").is_ok_and(|v| v.contains("truecolor") || v.contains("direct"))
            || std::env::var_os("WT_SESSION").is_some()
    })
}

/// An RGB color as a terminal color.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    if truecolor() {
        Color::Rgb(r, g, b)
    } else {
        let q = |v: u8| (u32::from(v) * 5 + 127) / 255;
        Color::Indexed((16 + 36 * q(r) + 6 * q(g) + q(b)) as u8)
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub ok: Color,
    pub err: Color,
    pub warn: Color,
    pub sel_bg: Color,
    pub err_bg: Color,
    pub match_bg: Color,
    pub bar_bg: Color,
    pub border: Color,
    pub gutter: Color,
    pub header: Color,
    pub macro_: Color,
    pub keyword: Color,
    pub ty: Color,
    pub number: Color,
    pub string: Color,
    pub comment: Color,
    pub op: Color,
    pub punct: Color,
    pub func: Color,
    pub key: Color,
}

impl Theme {
    pub fn new(dark: bool) -> Self {
        let c = rgb;
        if dark {
            Self {
                dark,
                fg: c(0xd4, 0xd8, 0xe0),
                dim: c(0x7d, 0x85, 0x94),
                accent: c(0x5f, 0xb3, 0xf9),
                ok: c(0x4c, 0xc3, 0x8a),
                err: c(0xff, 0x6b, 0x6b),
                warn: c(0xe5, 0xc0, 0x7b),
                sel_bg: c(0x3a, 0x4a, 0x6b),
                err_bg: c(0x4a, 0x25, 0x30),
                match_bg: c(0x6b, 0x5a, 0x1e),
                bar_bg: c(0x2b, 0x30, 0x3b),
                border: c(0x4a, 0x51, 0x5e),
                gutter: c(0x55, 0x5c, 0x68),
                header: c(0xf0, 0x88, 0x3e),
                macro_: c(0xc6, 0x8a, 0xf5),
                keyword: c(0x5f, 0xb3, 0xf9),
                ty: c(0x4e, 0xc9, 0xb0),
                number: c(0xe5, 0xc0, 0x7b),
                string: c(0x98, 0xc3, 0x79),
                comment: c(0x6b, 0x73, 0x80),
                op: c(0xff, 0x7a, 0x93),
                punct: c(0x8b, 0x93, 0xa1),
                func: c(0x61, 0xd0, 0xe8),
                key: c(0x5f, 0xb3, 0xf9),
            }
        } else {
            Self {
                dark,
                fg: c(0x24, 0x29, 0x2f),
                dim: c(0x6e, 0x77, 0x81),
                accent: c(0x09, 0x69, 0xda),
                ok: c(0x1a, 0x7f, 0x37),
                err: c(0xcf, 0x22, 0x2e),
                warn: c(0x9a, 0x67, 0x00),
                sel_bg: c(0xb6, 0xd4, 0xfe),
                err_bg: c(0xff, 0xd8, 0xd8),
                match_bg: c(0xff, 0xf0, 0xa6),
                bar_bg: c(0xe3, 0xe7, 0xec),
                border: c(0xa0, 0xa7, 0xb0),
                gutter: c(0xa0, 0xa7, 0xb0),
                header: c(0xc2, 0x4e, 0x00),
                macro_: c(0x82, 0x50, 0xdf),
                keyword: c(0x09, 0x69, 0xda),
                ty: c(0x0a, 0x7f, 0x6f),
                number: c(0x95, 0x5f, 0x00),
                string: c(0x1a, 0x7f, 0x37),
                comment: c(0x8c, 0x95, 0x9f),
                op: c(0xcf, 0x22, 0x2e),
                punct: c(0x57, 0x60, 0x6a),
                func: c(0x05, 0x7f, 0xa5),
                key: c(0x09, 0x69, 0xda),
            }
        }
    }

    pub fn text(&self) -> Style {
        Style::default().fg(self.fg)
    }
    pub fn dim(&self) -> Style {
        Style::default().fg(self.dim)
    }
    pub fn bold(&self) -> Style {
        Style::default().fg(self.fg).add_modifier(Modifier::BOLD)
    }
    pub fn accent(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
    /// Reverse-video "selected row" style.
    pub fn selected(&self) -> Style {
        Style::default().bg(self.sel_bg).fg(self.fg)
    }
}
