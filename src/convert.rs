//! Conversions between the forms a project travels in: the `SPECI1:` / `STUKI1:` clipboard
//! lines, the progalap.elte.hu links (`?data=…`), the plain content they hold (specification
//! text, structogram JSON) and C# programs. This is what the settings menu of the window app
//! and the `Space u` menu / `:url` commands of the terminal UI call.

use crate::compact;
use crate::file::{self, Project};
use crate::stuki::format as stformat;
use crate::stuki::from_csharp::from_csharp;

/// What to convert.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// Guess from the input: a line becomes a link, a link becomes a line.
    Auto,
    /// A `SPECI1:` / `STUKI1:` line (or specification text, or `.st` JSON) → web editor link.
    ToUrl,
    /// A web editor link → `SPECI1:` / `STUKI1:` line.
    ToLine,
    /// A web editor link → its content: the specification text, or the structogram JSON.
    Decode,
    /// A C# program → Stuki web editor link.
    CsToUrl,
    /// A C# program → `STUKI1:` line.
    CsToLine,
}

impl Op {
    pub const ALL: [Op; 6] = [
        Op::Auto,
        Op::ToUrl,
        Op::ToLine,
        Op::Decode,
        Op::CsToUrl,
        Op::CsToLine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Op::Auto => "Automatic (line <-> link)",
            Op::ToUrl => "Line / text → web editor link",
            Op::ToLine => "Web editor link → line",
            Op::Decode => "Web editor link → text / JSON",
            Op::CsToUrl => "C# program → Stuki link",
            Op::CsToLine => "C# program → STUKI1 line",
        }
    }

    /// Does the input hold C# source?
    pub fn wants_csharp(self) -> bool {
        matches!(self, Op::CsToUrl | Op::CsToLine)
    }
}

/// The result of a conversion.
#[derive(Debug)]
pub struct Converted {
    /// The link, line or text made.
    pub text: String,
    /// One line about what was read (`parsed 2 method(s): Main, Fibonacci`).
    pub note: String,
}

fn squash(t: &str) -> String {
    t.chars().filter(|c| !c.is_whitespace()).collect()
}

fn names(p: &crate::stuki::model::Program) -> String {
    let n: Vec<_> = p.diagrams.iter().map(|d| d.name.text.clone()).collect();
    format!("{} structogram(s): {}", n.len(), n.join(", "))
}

/// Does the text look like a progalap.elte.hu link with data?
pub fn is_link(t: &str) -> bool {
    let t = squash(t);
    t.contains("data=") && (t.contains("/stuki") || t.contains("/specifikacio"))
        || t.starts_with("http") && t.contains("data=")
}

/// Does the text look like a `SPECI1:` / `STUKI1:` line?
pub fn is_line(t: &str) -> bool {
    let t = t.trim_start();
    t.starts_with(file::COMPACT_PREFIX) || t.starts_with(stformat::COMPACT_PREFIX)
}

/// Runs the conversion.
pub fn run(op: Op, input: &str) -> Result<Converted, String> {
    if input.trim().is_empty() {
        return Err("Nothing to convert: the input is empty.".into());
    }
    match op {
        Op::Auto => {
            if is_link(input) {
                to_line(input)
            } else {
                to_url(input)
            }
        }
        Op::ToUrl => to_url(input),
        Op::ToLine => to_line(input),
        Op::Decode => decode(input),
        Op::CsToUrl | Op::CsToLine => {
            let c = from_csharp(input)?;
            let note = format!(
                "parsed {} method(s): {}",
                c.methods.len(),
                c.methods.join(", ")
            );
            let text = if op == Op::CsToUrl {
                stformat::to_web_link(&c.program)
            } else {
                stformat::to_compact(&c.program)
            };
            Ok(Converted { text, note })
        }
    }
}

fn to_url(input: &str) -> Result<Converted, String> {
    let t = input.trim();
    if is_link(t) {
        return Err("This is already a link (use “link → line” to shorten it).".into());
    }
    // a line without its prefix is taken for a structogram, as the old script did
    let bare = !is_line(t)
        && !t.starts_with('{')
        && squash(t).len() > 40
        && squash(t)
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_+/=".contains(c));
    let t = if bare {
        format!("{}{}", stformat::COMPACT_PREFIX, squash(t))
    } else {
        t.to_string()
    };
    if stformat::looks_like_stuki(&t) {
        let p = stformat::from_clipboard(&t)?;
        return Ok(Converted {
            note: format!("read {}", names(&p)),
            text: stformat::to_web_link(&p),
        });
    }
    let Project { doc, .. } = file::from_clipboard(&t)?;
    Ok(Converted {
        note: format!(
            "read a specification ({} characters, {} test(s))",
            doc.spec.chars().count(),
            doc.tests.len()
        ),
        text: file::to_web_link(&doc),
    })
}

fn to_line(input: &str) -> Result<Converted, String> {
    let t = input.trim();
    if is_line(t) {
        return Err("This is already a line (use “line → link” for a link).".into());
    }
    if !is_link(t) {
        return Err("This is not a progalap.elte.hu link with data=… in it.".into());
    }
    if stformat::looks_like_stuki(t) {
        let p = stformat::from_clipboard(t)?;
        return Ok(Converted {
            note: format!("read {}", names(&p)),
            text: stformat::to_compact(&p),
        });
    }
    let project = file::from_clipboard(t)?;
    let note = format!(
        "read a specification ({} characters, {} test(s))",
        project.doc.spec.chars().count(),
        project.doc.tests.len()
    );
    let program = project.stuki.unwrap_or_default();
    Ok(Converted {
        note,
        text: file::to_compact(&project.doc, &program),
    })
}

fn decode(input: &str) -> Result<Converted, String> {
    let t = input.trim();
    let json = compact::web_link_json(t)
        .ok_or("This is not a link with data=… in it.")?
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("The link's data is not JSON: {e}"))?;
    if let Some(spec) = v.get("specification").and_then(|s| s.as_str()) {
        let tests = v["tests"].as_array().map_or(0, Vec::len);
        let note = if tests > 0 {
            format!(
                "{tests} test(s) are in the link but not in this text (the line form keeps them)"
            )
        } else {
            "specification text".into()
        };
        return Ok(Converted {
            text: spec.to_string(),
            note,
        });
    }
    if v.get("structograms").is_some() {
        let p = stformat::from_json(&v)?;
        return Ok(Converted {
            note: format!("{} (the C# source is not stored in the link)", names(&p)),
            text: serde_json::to_string_pretty(&v).unwrap_or(json),
        });
    }
    Err("The link holds neither a specification nor a structogram.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stuki::from_csharp::DEMO_SOURCE;

    #[test]
    fn csharp_to_link_and_back() {
        let url = run(Op::CsToUrl, DEMO_SOURCE).unwrap();
        assert!(url.text.starts_with(compact::STUKI_EDITOR));
        assert!(url.note.contains("Main, Fibonacci"));
        let line = run(Op::Auto, &url.text).unwrap();
        assert!(line.text.starts_with("STUKI1:"));
        let again = run(Op::Auto, &line.text).unwrap();
        assert!(again.text.starts_with(compact::STUKI_EDITOR));
        let json = run(Op::Decode, &url.text).unwrap();
        assert!(json.text.contains("\"Fibonacci\""));
        let direct = run(Op::CsToLine, DEMO_SOURCE).unwrap();
        assert!(direct.text.starts_with("STUKI1:"));
    }

    #[test]
    fn specification_links() {
        let spec = "Be: n∈N\nKi: x∈N\nUf: x=n\n";
        let url = run(Op::ToUrl, spec).unwrap();
        assert!(url.text.starts_with(compact::SPEC_EDITOR));
        let text = run(Op::Decode, &url.text).unwrap();
        assert!(text.text.contains("Be: n∈N"));
        let line = run(Op::ToLine, &url.text).unwrap();
        assert!(line.text.starts_with("SPECI1:"));
        // wrapped lines are fine
        let wrapped = format!("{}\n{}", &line.text[..20], &line.text[20..]);
        let back = run(Op::ToUrl, &wrapped).unwrap();
        let again = run(Op::Decode, &back.text).unwrap();
        assert_eq!(again.text, text.text);
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(run(Op::Auto, "  ").is_err());
        assert!(
            run(Op::ToLine, "hello")
                .unwrap_err()
                .contains("not a progalap")
        );
        assert!(run(Op::Decode, "hello").is_err());
        assert!(run(Op::CsToUrl, "class A {}").is_err());
        let url = run(Op::CsToUrl, DEMO_SOURCE).unwrap().text;
        assert!(run(Op::ToUrl, &url).unwrap_err().contains("already a link"));
    }
}
