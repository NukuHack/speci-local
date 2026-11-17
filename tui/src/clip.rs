//! The clipboard without a windowing library: the OSC 52 terminal escape (works over SSH and
//! in tmux), plus the system's command-line tools when there are any.

use std::io::Write;
use std::process::{Command, Stdio};

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

const COPY: &[&[&str]] = &[
    &["wl-copy"],
    &["xclip", "-selection", "clipboard", "-in"],
    &["xsel", "--clipboard", "--input"],
    &["pbcopy"],
    &["clip.exe"],
];
const PASTE: &[&[&str]] = &[
    &["wl-paste", "--no-newline"],
    &["xclip", "-selection", "clipboard", "-out"],
    &["xsel", "--clipboard", "--output"],
    &["pbpaste"],
    &[
        "powershell.exe",
        "-NoProfile",
        "-Command",
        "Get-Clipboard -Raw",
    ],
];

/// Copy `text`. Returns a short description of what was used.
pub fn copy(text: &str) -> String {
    for cmd in COPY {
        let Ok(mut child) = Command::new(cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return cmd[0].to_string();
        }
    }
    osc52(text);
    "the terminal (OSC 52)".into()
}

/// Always send the escape too: it reaches the local clipboard through SSH.
pub fn osc52(text: &str) {
    if text.len() > 100_000 {
        return;
    }
    let seq = format!("\x1b]52;c;{}\x07", b64(text.as_bytes()));
    let seq = if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

pub fn paste() -> Option<String> {
    for cmd in PASTE {
        if let Ok(o) = Command::new(cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && o.status.success()
        {
            return String::from_utf8(o.stdout).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_is_standard() {
        assert_eq!(super::b64(b"Man"), "TWFu");
        assert_eq!(super::b64(b"Ma"), "TWE=");
        assert_eq!(super::b64(b"M"), "TQ==");
    }
}
