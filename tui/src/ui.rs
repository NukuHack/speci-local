//! Drawing.

use crate::app::{App, LineKind, Overlay, Pane, Tool, View};
use crate::ascii;
use crate::code::{CodePane, Source};
use crate::input::Input;
use crate::nslayout::{self, Focus, Tag, wrap};
use crate::stuki::Menu;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use speci_local::eval::{Step, StepKind};

type Ln = Line<'static>;

/// An input as drawn (ASCII mode spells symbols out) and the column of its cursor.
fn input_view(prefix: &str, input: &Input) -> (String, usize) {
    let before: String = input.t[..input.p].iter().collect();
    let col =
        prefix.chars().count() + unicode_width::UnicodeWidthStr::width(&*ascii::text(&before));
    (format!("{prefix}{}", ascii::text(&input.text())), col)
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// Borders in plain ASCII; the focused pane gets heavier `=` and `#` so it still stands out
/// without colors.
const fn ascii_border(focused: bool) -> symbols::border::Set<'static> {
    let (h, corner) = if focused { ("=", "#") } else { ("-", "+") };
    symbols::border::Set {
        top_left: corner,
        top_right: corner,
        bottom_left: corner,
        bottom_right: corner,
        vertical_left: "|",
        vertical_right: "|",
        horizontal_top: h,
        horizontal_bottom: h,
    }
}

fn pane_block(title: &str, right: &str, focused: bool, th: &Theme) -> Block<'static> {
    let (bs, ts) = if focused {
        (
            Style::default().fg(th.accent),
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        )
    } else {
        (Style::default().fg(th.border), Style::default().fg(th.dim))
    };
    let block = Block::default().borders(Borders::ALL).border_style(bs);
    let block = if ascii::on() {
        block.border_set(ascii_border(focused))
    } else if focused {
        block.border_type(ratatui::widgets::BorderType::Thick)
    } else {
        block.border_type(ratatui::widgets::BorderType::Plain)
    };
    block
        .title_top(Line::from(Span::styled(format!(" {title} "), ts)).left_aligned())
        .title_top(
            Line::from(Span::styled(
                format!(" {right} "),
                Style::default().fg(th.dim),
            ))
            .right_aligned(),
        )
}

pub fn draw(f: &mut Frame<'_>, app: &mut App) {
    let area = f.area();
    if area.width < 20 || area.height < 5 {
        f.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_tabs(f, app, top);
    match app.tool {
        Tool::Spec => draw_spec_tool(f, app, body),
        Tool::Stuki => draw_stuki_tool(f, app, body),
        Tool::Code => draw_code_tool(f, app, body),
    }
    draw_status(f, app, bottom);
    draw_overlay(f, app, area);
    if ascii::on() {
        ascii::flatten(f.buffer_mut());
    }
}

fn draw_tabs(f: &mut Frame<'_>, app: &App, area: Rect) {
    let th = app.theme;
    let mut spans: Vec<Span<'_>> = vec![
        Span::styled(
            " speci ",
            Style::default()
                .fg(th.bar_bg)
                .bg(th.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    for (t, label) in [
        (Tool::Spec, "1 Specification"),
        (Tool::Stuki, "2 Structogram"),
        (Tool::Code, "3 Code"),
    ] {
        let on = app.tool == t;
        let dirty = match t {
            Tool::Spec => app.dirty(),
            Tool::Stuki => app.stuki.dirty(),
            Tool::Code => app.code.dirty(),
        };
        let st = if on {
            Style::default()
                .fg(th.fg)
                .bg(th.sel_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        spans.push(Span::styled(
            format!(" {label}{} ", if dirty { " ●" } else { "" }),
            st,
        ));
        spans.push(Span::raw(" "));
    }
    let title = match app.tool {
        Tool::Spec | Tool::Code => {
            let file = app
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| "untitled.sp".into(), |n| n.to_string_lossy().to_string());
            if app.doc.title.is_empty() {
                file
            } else {
                format!("{}  ({file})", app.doc.title)
            }
        }
        Tool::Stuki => app.stuki.file_name(),
    };
    let right = {
        let mut r = vec![];
        if app.tool == Tool::Spec {
            r.push(Span::styled(
                match (app.auto, ascii::on()) {
                    (true, true) => "auto:on ",
                    (false, true) => "auto:off ",
                    (true, false) => "auto ✓ ",
                    (false, false) => "auto ✗ ",
                },
                if app.auto {
                    Style::default().fg(th.ok)
                } else {
                    th.dim()
                },
            ));
        }
        if app.running() {
            r.push(Span::styled("⟳ working ", Style::default().fg(th.warn)));
        }
        r.push(Span::styled("F1/? help ", th.dim()));
        r
    };
    let rw: usize = right.iter().map(Span::width).sum();
    let lw: usize = spans.iter().map(Span::width).sum();
    let avail = (area.width as usize).saturating_sub(lw + rw + 1);
    let mut t: String = title.chars().take(avail).collect();
    if title.chars().count() > avail && avail > 1 {
        t.pop();
        t.push('…');
    }
    let pad = avail.saturating_sub(t.chars().count());
    spans.push(Span::styled(t, th.bold()));
    spans.push(Span::raw(" ".repeat(pad + 1)));
    spans.extend(right);
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bar_bg)),
        area,
    );
}

// ------------------------------------------------------------------ spec tool

fn spec_rects(area: Rect, app: &App) -> [Option<Rect>; 3] {
    let single = app.zoom || area.height < 14 || area.width < 70;
    if single {
        let mut r = [None; 3];
        r[match app.focus {
            Pane::Spec => 0,
            Pane::Data => 1,
            Pane::Eval => 2,
        }] = Some(area);
        return r;
    }
    if area.width >= 120 {
        let [a, b, c] = Layout::horizontal([
            Constraint::Percentage(38),
            Constraint::Percentage(26),
            Constraint::Percentage(36),
        ])
        .areas(area);
        return [Some(a), Some(b), Some(c)];
    }
    let [l, r] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let [d, e] =
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(r);
    [Some(l), Some(d), Some(e)]
}

fn draw_spec_tool(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let rects = spec_rects(area, app);
    let th = app.theme;
    let single = rects.iter().filter(|r| r.is_some()).count() == 1;
    let nav = |p: Pane| -> String {
        if single {
            let names = [
                (Pane::Spec, "1 Spec"),
                (Pane::Data, "2 Data"),
                (Pane::Eval, "3 Eval"),
            ];
            names
                .iter()
                .map(|(q, n)| {
                    if *q == p {
                        format!("[{n}]")
                    } else {
                        n.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            match p {
                Pane::Spec => "1 Specification".into(),
                Pane::Data => "2 Test data".into(),
                Pane::Eval => "3 Evaluation".into(),
            }
        }
    };
    let overlay_none = matches!(app.overlay, Overlay::None | Overlay::Line(..));
    if let Some(r) = rects[0] {
        let focused = app.focus == Pane::Spec;
        let (l, c) = app.spec_ed.line_col();
        let mark = if app.dirty() { " ●" } else { "" };
        let blk = pane_block(
            &format!("{}{}", nav(Pane::Spec), mark),
            &format!("{l}:{c}"),
            focused,
            &th,
        );
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        app.spec_ed
            .draw(f.buffer_mut(), inner, &th, &app.vim, focused);
        if focused
            && overlay_none
            && let Some(p) = app.spec_ed.cursor_xy
        {
            f.set_cursor_position(p);
        }
    }
    if let Some(r) = rects[1] {
        let focused = app.focus == Pane::Data;
        let i = app.current;
        let n = app.doc.tests.len();
        let (l, c) = app.data_eds[i].line_col();
        let blk = pane_block(&nav(Pane::Data), &format!("{l}:{c}"), focused, &th);
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        if inner.height > 2 {
            let strip = Rect { height: 1, ..inner };
            let ed_area = Rect {
                y: inner.y + 1,
                height: inner.height - 1,
                ..inner
            };
            f.render_widget(Paragraph::new(test_strip(app, inner.width as usize)), strip);
            let _ = n;
            let (v, eds) = (&app.vim, &mut app.data_eds);
            eds[i].draw(f.buffer_mut(), ed_area, &th, v, focused);
            if focused
                && overlay_none
                && let Some(p) = eds[i].cursor_xy
            {
                f.set_cursor_position(p);
            }
        }
    }
    if let Some(r) = rects[2] {
        let focused = app.focus == Pane::Eval;
        let view = match app.view {
            View::Results => "Results",
            View::Stepper => "Stepper",
            View::StepList => "Step list",
        };
        let title = if single {
            nav(Pane::Eval)
        } else {
            format!("3 Evaluation · {view}")
        };
        let right = if single {
            view.to_string()
        } else {
            format!("test {}/{}", app.current + 1, app.doc.tests.len())
        };
        let blk = pane_block(&title, &right, focused, &th);
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        draw_eval(f, app, inner);
    }
}

fn test_strip(app: &App, width: usize) -> Line<'static> {
    let th = app.theme;
    let mut items: Vec<(String, Style)> = vec![];
    for i in 0..app.doc.tests.len() {
        let (mark, col) = match app.results.get(i) {
            Some(Some(o)) if o.pass => ("✔", th.ok),
            Some(Some(_)) => ("✘", th.err),
            _ => (if ascii::on() { "." } else { "·" }, th.dim),
        };
        let on = i == app.current;
        let st = if on {
            Style::default()
                .fg(col)
                .bg(th.sel_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(col)
        };
        items.push((format!(" {}{} ", i + 1, mark), st));
    }
    // a window of the tabs that contains the current one
    let (mut a, mut b) = (app.current, app.current + 1);
    let w = |r: std::ops::Range<usize>| {
        items[r]
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>()
    };
    loop {
        let more = a > 0 && w(a - 1..b) + 2 <= width;
        let more_r = b < items.len() && w(a..b + 1) + 2 <= width;
        if more {
            a -= 1;
        } else if more_r {
            b += 1;
        } else {
            break;
        }
    }
    let mut spans = vec![];
    spans.push(Span::styled(if a > 0 { "‹" } else { " " }, th.dim()));
    for (s, st) in &items[a..b] {
        spans.push(Span::styled(s.clone(), *st));
    }
    if b < items.len() {
        spans.push(Span::styled("›", th.dim()));
    }
    Line::from(spans)
}

// ------------------------------------------------------------------ evaluation pane

fn snippet(src: &str, st: &Step) -> String {
    let s = src.get(st.span.0..st.span.1).unwrap_or("");
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 60 {
        let mut t: String = s.chars().take(59).collect();
        t.push('…');
        t
    } else {
        s
    }
}

fn describe(src: &str, st: &Step) -> String {
    match &st.kind {
        StepKind::Value(v) => format!("{} = {v}", snippet(src, st)),
        StepKind::Error(e) => format!("✘ {} — {e}", snippet(src, st)),
        StepKind::Enter(c) => format!("↓ calling {c}"),
        StepKind::Leave(v) => format!("↑ returned {v}"),
    }
}

fn wrapped(text: &str, w: usize, st: Style, indent: &str) -> Vec<Ln> {
    text.split('\n')
        .flat_map(|l| {
            let lead: String = l.chars().take_while(|c| *c == ' ').collect();
            wrap(
                l.trim_start(),
                w.saturating_sub(indent.chars().count() + lead.len()).max(8),
            )
            .into_iter()
            .map(move |x| Line::from(Span::styled(format!("{indent}{lead}{x}"), st)))
            .collect::<Vec<_>>()
        })
        .collect()
}

fn results_lines(app: &App, w: usize) -> (Vec<Ln>, Vec<usize>) {
    let th = app.theme;
    let mut out: Vec<Ln> = vec![];
    let mut starts = vec![];
    let spec_src = app
        .evaluated
        .as_ref()
        .map_or(app.doc.spec.as_str(), |d| d.spec.as_str());
    if !app.spec_diags.is_empty() {
        out.push(Line::from(Span::styled(
            "✘ The specification has errors",
            Style::default().fg(th.err).add_modifier(Modifier::BOLD),
        )));
        for d in &app.spec_diags {
            out.extend(wrapped(&d.render(spec_src, ""), w, th.text(), "  "));
            out.push(Line::default());
        }
        out.push(Line::from(Span::styled(
            "  Enter jumps to the first error, ]e / [e step through them",
            th.dim(),
        )));
        out.push(Line::default());
    }
    for i in 0..app.doc.tests.len() {
        starts.push(out.len());
        let cur = i == app.current;
        let plain = ascii::on();
        let (mark, col, label) = match app.results.get(i) {
            Some(Some(o)) if o.pass => (if plain { "[ok]" } else { "✔" }, th.ok, "passed"),
            Some(Some(_)) => (if plain { "[FAIL]" } else { "✘" }, th.err, "failed"),
            _ if !app.spec_diags.is_empty() => {
                (if plain { "[--]" } else { "–" }, th.dim, "not run")
            }
            _ => (
                if plain { "[..]" } else { "…" },
                th.dim,
                "not evaluated yet",
            ),
        };
        let mut head = Style::default().fg(col).add_modifier(Modifier::BOLD);
        if cur {
            head = head.bg(th.sel_bg);
        }
        out.push(Line::from(vec![
            Span::styled(
                if cur { "▶ " } else { "  " },
                Style::default().fg(th.accent),
            ),
            Span::styled(format!("{mark} Test case {}  {label}", i + 1), head),
        ]));
        if let Some(Some(o)) = app.results.get(i) {
            let data_src = app
                .evaluated
                .as_ref()
                .and_then(|d| d.tests.get(i))
                .map_or("", |s| s.as_str());
            for d in &o.diags {
                out.extend(wrapped(&d.render(spec_src, data_src), w, th.text(), "    "));
                let url = d.doc_url();
                out.push(Line::from(Span::styled(
                    format!("    More: {url}"),
                    th.dim(),
                )));
            }
            if !o.values.is_empty() {
                out.push(Line::from(Span::styled("    Values:", th.dim())));
                for (n, v, from_test) in &o.values {
                    let note = if *from_test { "" } else { "   (computed)" };
                    out.extend(wrapped(&format!("{n} = {v}{note}"), w, th.text(), "      "));
                }
            }
        }
        out.push(Line::default());
    }
    if app.spec_diags.is_empty() {
        out.extend(wrapped(
            "Space f  builds a structogram from this specification",
            w,
            th.dim(),
            "",
        ));
    }
    (out, starts)
}

fn stepper_lines(app: &App, w: usize) -> Vec<Ln> {
    let th = app.theme;
    let Some(tr) = &app.trace else {
        return wrapped(
            "No steps recorded yet. Steps are traced for the selected test case once the specification is evaluated (<Space>r).",
            w,
            th.dim(),
            "",
        );
    };
    if tr.steps.is_empty() {
        return wrapped(
            "This test case evaluated nothing step by step.",
            w,
            th.dim(),
            "",
        );
    }
    let n = tr.steps.len();
    let i = app.step.min(n - 1);
    let st = &tr.steps[i];
    let src = app.evaluated.as_ref().map_or("", |d| d.spec.as_str());
    let mut out = vec![Line::from(vec![
        Span::styled(format!("Step {} of {n}", i + 1), th.accent()),
        Span::styled(
            format!("   {}  (test case {})", st.label, app.traced_test + 1),
            th.dim(),
        ),
    ])];
    if tr.truncated {
        out.push(Line::from(Span::styled(
            "(only the first steps were recorded)",
            Style::default().fg(th.warn),
        )));
    }
    out.push(Line::default());
    // the formula, with the part being evaluated highlighted
    let ls = src
        .get(..st.root.0)
        .and_then(|b| b.rfind('\n'))
        .map_or(0, |i| i + 1);
    let le = src
        .get(st.root.1..)
        .and_then(|a| a.find('\n'))
        .map_or(src.len(), |i| i + st.root.1);
    let mut cells: Vec<(char, Style)> = vec![];
    for (a, b, hi) in [
        (ls, st.span.0, false),
        (st.span.0, st.span.1, true),
        (st.span.1, le, false),
    ] {
        if let Some(t) = src.get(a..b) {
            let style = if hi {
                Style::default().fg(Color::Black).bg(th.warn)
            } else {
                th.text()
            };
            cells.extend(t.chars().map(|c| (if c == '\n' { ' ' } else { c }, style)));
        }
    }
    for chunk in cells.chunks(w.saturating_sub(2).max(10)) {
        out.push(Line::from(
            chunk
                .iter()
                .map(|(c, s)| Span::styled(c.to_string(), *s))
                .collect::<Vec<_>>(),
        ));
    }
    out.push(Line::default());
    let col = match &st.kind {
        StepKind::Error(_) => th.err,
        StepKind::Enter(_) | StepKind::Leave(_) => th.accent,
        StepKind::Value(_) => th.fg,
    };
    out.extend(wrapped(
        &describe(src, st),
        w,
        Style::default().fg(col).add_modifier(Modifier::BOLD),
        "",
    ));
    if !st.scope.is_empty() {
        let vars: Vec<String> = st.scope.iter().map(|(n, v)| format!("{n} = {v}")).collect();
        out.extend(wrapped(&vars.join("   "), w, th.dim(), ""));
    }
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "h/l or ←/→ step · gg/G first/last · v next view · Enter step list",
        th.dim(),
    )));
    out
}

fn steplist_lines(app: &App, w: usize) -> (Vec<Ln>, usize) {
    let th = app.theme;
    let Some(tr) = &app.trace else {
        return (
            vec![Line::from(Span::styled("No steps recorded yet.", th.dim()))],
            0,
        );
    };
    let src = app.evaluated.as_ref().map_or("", |d| d.spec.as_str());
    let sel = app.step.min(tr.steps.len().saturating_sub(1));
    let lines = tr
        .steps
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let text = ascii::text(&format!(
                "{:>5} {}{} {}",
                i + 1,
                "  ".repeat(st.depth.min(6)),
                st.label,
                describe(src, st)
            ))
            .into_owned();
            let t: String = text.chars().take(w).collect();
            let style = if i == sel {
                th.selected().add_modifier(Modifier::BOLD)
            } else {
                th.text()
            };
            Line::from(Span::styled(format!("{t:<w$}"), style))
        })
        .collect();
    (lines, sel)
}

fn draw_eval(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    let w = area.width as usize - 1;
    let h = area.height as usize;
    app.eval_h = h;
    let (lines, sel_row) = match app.view {
        View::Results => {
            let (l, starts) = results_lines(app, w);
            if app.scroll_to_card {
                app.scroll_to_card = false;
                if let Some(s) = starts.get(app.current) {
                    app.eval_scroll = s.saturating_sub(0);
                }
            }
            (l, None)
        }
        View::Stepper => (stepper_lines(app, w), None),
        View::StepList => {
            let (l, s) = steplist_lines(app, w);
            (l, Some(s))
        }
    };
    app.eval_len = lines.len();
    if let Some(s) = sel_row {
        if s < app.eval_scroll {
            app.eval_scroll = s;
        }
        if s >= app.eval_scroll + h {
            app.eval_scroll = s + 1 - h;
        }
    } else if app.view == View::Stepper {
        app.eval_scroll = 0;
    }
    app.eval_scroll = app.eval_scroll.min(lines.len().saturating_sub(1));
    let shown: Vec<Ln> = lines
        .iter()
        .skip(app.eval_scroll)
        .take(h)
        .cloned()
        .collect();
    f.render_widget(
        Paragraph::new(shown),
        Rect {
            width: area.width - 1,
            ..area
        },
    );
    // scrollbar
    if lines.len() > h {
        let th = app.theme;
        let x = area.x + area.width - 1;
        let total = lines.len();
        let thumb = (h * h / total).max(1);
        let pos = app.eval_scroll * (h - thumb) / (total - h).max(1);
        for y in 0..h {
            let on = y >= pos && y < pos + thumb;
            f.buffer_mut().set_string(
                x,
                area.y + y as u16,
                if on { "█" } else { "│" },
                Style::default().fg(if on { th.dim } else { th.border }),
            );
        }
    }
}

// ------------------------------------------------------------------ code tool

/// The program, the console input, the expected output and the output.
fn code_rects(area: Rect, app: &App) -> [Option<Rect>; 4] {
    let console = app.code.source == Source::Console;
    let single = app.zoom || area.height < 14 || area.width < 70;
    let input_column = |r: Rect| -> [Rect; 2] {
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(r)
    };
    if single {
        let mut r = [None; 4];
        match app.code.focus {
            CodePane::Editor => r[0] = Some(area),
            CodePane::Input => {
                let [inp, exp] = input_column(area);
                (r[1], r[2]) = (Some(inp), Some(exp));
            }
            CodePane::Output => r[3] = Some(area),
        }
        return r;
    }
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);
    if console {
        let [out, side] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(bottom);
        let [inp, exp] = input_column(side);
        [Some(top), Some(inp), Some(exp), Some(out)]
    } else {
        [Some(top), None, None, Some(bottom)]
    }
}

fn draw_code_tool(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let rects = code_rects(area, app);
    let th = app.theme;
    let single = rects.iter().filter(|r| r.is_some()).count() <= 2
        && (app.zoom || area.height < 14 || area.width < 70);
    let console = app.code.source == Source::Console;
    let overlay_none = matches!(app.overlay, Overlay::None | Overlay::Line(..));
    let nav = |p: CodePane| -> String {
        let all: Vec<(CodePane, &str, &str)> = [
            (CodePane::Editor, "1 Code", "1 C# program"),
            (CodePane::Input, "2 Input", "2 Console input"),
            (CodePane::Output, "3 Output", "3 Output"),
        ]
        .into_iter()
        .filter(|(q, ..)| console || *q != CodePane::Input)
        .collect();
        if single {
            all.iter()
                .map(|(q, short, _)| {
                    if *q == p {
                        format!("[{short}]")
                    } else {
                        (*short).to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            all.iter()
                .find(|(q, ..)| *q == p)
                .map(|(_, _, long)| (*long).to_string())
                .unwrap_or_default()
        }
    };
    let spec = app.doc.spec.clone();
    let test = app.doc.tests.get(app.current).cloned().unwrap_or_default();

    if let Some(r) = rects[0] {
        let focused = app.code.focus == CodePane::Editor;
        let (l, c) = app.code.ed.line_col();
        let mark = if app.code.dirty() { " ●" } else { "" };
        let right = if single {
            format!("{l}:{c}")
        } else {
            format!(
                "{} · test {}/{} · {l}:{c}",
                app.code.source.label(),
                app.current + 1,
                app.doc.tests.len().max(1)
            )
        };
        let blk = pane_block(
            &format!("{}{mark}", nav(CodePane::Editor)),
            &right,
            focused,
            &th,
        );
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        app.code
            .ed
            .draw(f.buffer_mut(), inner, &th, &app.vim, focused);
        if focused
            && overlay_none
            && let Some(p) = app.code.ed.cursor_xy
        {
            f.set_cursor_position(p);
        }
    }
    if let Some(r) = rects[1] {
        let focused = app.code.focus == CodePane::Input;
        let (l, c) = app.code.input.line_col();
        let blk = pane_block(&nav(CodePane::Input), &format!("{l}:{c}"), focused, &th);
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        app.code
            .input
            .draw(f.buffer_mut(), inner, &th, &app.vim, focused);
        if focused
            && overlay_none
            && let Some(p) = app.code.input.cursor_xy
        {
            f.set_cursor_position(p);
        }
    }
    let verdict = app.code.verdict(&spec, &test);
    if let Some(r) = rects[2] {
        let (right, col) = match verdict {
            Some(true) => ("✔ matches", th.ok),
            Some(false) => ("✘ differs", th.err),
            None => ("", th.dim),
        };
        let title = format!("Expected output (test {})", app.current + 1);
        let blk = pane_block(&title, "", false, &th);
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        // the verdict in its own color, over the right end of the top border
        let chip = format!(" {right} ");
        let w = chip.chars().count() as u16;
        if !right.is_empty() && w + 2 < r.width {
            let at = Rect::new(r.right() - w - 1, r.y, w, 1);
            f.render_widget(
                Paragraph::new(Span::styled(
                    chip,
                    Style::default().fg(col).add_modifier(Modifier::BOLD),
                )),
                at,
            );
        }
        let (text, style) = match app.code.expected(&spec, &test) {
            Ok(t) => (t, th.text()),
            Err(e) => (e, th.dim()),
        };
        let lines: Vec<Ln> = text
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), style)))
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }
    if let Some(r) = rects[3] {
        let focused = app.code.focus == CodePane::Output;
        let right = match (app.code.run_status.as_str(), verdict) {
            ("", _) => String::new(),
            (s, Some(true)) => format!("{s} · ✔ matches"),
            (s, Some(false)) => format!("{s} · ✘ differs"),
            (s, None) => s.to_string(),
        };
        let blk = pane_block(&nav(CodePane::Output), &right, focused, &th);
        let inner = blk.inner(r);
        f.render_widget(blk, r);
        let h = inner.height as usize;
        let lines: Vec<&str> = if app.code.output.is_empty() {
            vec![]
        } else {
            app.code.output.lines().collect()
        };
        app.code.out_h = h;
        app.code.out_len = lines.len();
        let last = lines.len().saturating_sub(h.max(1));
        // a running program: follow what it prints, unless you are reading the output
        if app.code.running() && !focused {
            app.code.out_scroll = last;
        }
        app.code.out_scroll = app.code.out_scroll.min(lines.len().saturating_sub(1));
        let shown: Vec<Ln> = if lines.is_empty() {
            vec![Line::from(Span::styled(
                if console {
                    "Nothing printed yet. Space r runs the program."
                } else {
                    "Nothing printed yet. Space r (or F5) runs the program."
                },
                th.dim(),
            ))]
        } else {
            lines
                .iter()
                .skip(app.code.out_scroll)
                .take(h)
                .map(|l| Line::from(Span::styled((*l).to_string(), th.text())))
                .collect()
        };
        f.render_widget(Paragraph::new(shown), inner);
    }
}

// ------------------------------------------------------------------ structogram tool

fn draw_stuki_tool(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let th = app.theme;
    let panel = area.width >= 100 && !app.zoom;
    let (canvas, side) = if panel {
        let [a, b] = Layout::horizontal([Constraint::Min(40), Constraint::Length(42)]).areas(area);
        (a, Some(b))
    } else {
        (area, None)
    };
    let nd = app.stuki.program.diagrams.len();
    let blk = pane_block(
        "Structogram",
        &format!("{} of {nd}", app.stuki.current_diagram() + 1),
        true,
        &th,
    );
    let inner = blk.inner(canvas);
    f.render_widget(blk, canvas);
    let words = crate::stuki::Stuki::layout_words(&app.doc.spec);
    let avail = (inner.width as usize).saturating_sub(1);
    let failing = app.stuki.failing_node();
    let l = nslayout::layout(&app.stuki.program, avail, &words, failing);
    app.stuki.note_layout(&l, inner.height as usize);
    let h = inner.height as usize;
    if let Some(r) = l.rect(app.stuki.focus) {
        let st = &mut app.stuki;
        // scrolling follows the cursor
        let above = usize::from(!matches!(st.focus, Focus::Name(_)));
        if r.y < st.top + above {
            st.top = r.y.saturating_sub(above);
        }
        if r.y + r.h + 1 > st.top + h {
            st.top = (r.y + r.h + 1).saturating_sub(h);
        }
        if r.x < st.left {
            st.left = r.x.saturating_sub(1);
        }
        if r.x + r.w > st.left + avail {
            st.left = (r.x + r.w).saturating_sub(avail);
        }
    }
    let st = &mut app.stuki;
    st.top = st.top.min(l.height().saturating_sub(h.min(l.height())));
    st.left = if l.w <= avail {
        0
    } else {
        st.left.min(l.w - avail)
    };
    let focus_rect = l.rect(st.focus);
    let buf = f.buffer_mut();
    for sy in 0..h {
        let y = st.top + sy;
        let Some(row) = l.cells.get(y) else { break };
        for sx in 0..avail {
            let x = st.left + sx;
            let Some(c) = row.get(x) else { break };
            if c.ch == '\0' {
                continue;
            }
            let inr = |r: &nslayout::Rect| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
            let style = if l.failing.as_ref().is_some_and(inr) {
                Style::default()
                    .fg(th.err)
                    .bg(th.err_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                match c.tag {
                    Tag::Border => Style::default().fg(th.punct),
                    Tag::Text => th.text(),
                    Tag::Cond => Style::default().fg(th.accent),
                    Tag::Label => Style::default().fg(th.header),
                    Tag::Dim => th.dim(),
                    Tag::Title => Style::default().fg(th.fg).add_modifier(Modifier::BOLD),
                }
            };
            let style = if focus_rect.as_ref().is_some_and(inr) {
                style.bg(th.sel_bg).add_modifier(Modifier::BOLD)
            } else {
                style
            };
            buf.set_string(
                inner.x + sx as u16,
                inner.y + sy as u16,
                c.shown().to_string(),
                style,
            );
        }
    }
    if l.height() > h {
        let total = l.height();
        let thumb = (h * h / total).max(1);
        let pos = st.top * (h - thumb) / (total - h).max(1);
        for y in 0..h {
            let on = y >= pos && y < pos + thumb;
            buf.set_string(
                inner.x + inner.width - 1,
                inner.y + y as u16,
                if on { "█" } else { "│" },
                Style::default().fg(if on { th.dim } else { th.border }),
            );
        }
    }
    if let Some(s) = side {
        draw_stuki_side(f, app, s);
    }
}

fn draw_stuki_side(f: &mut Frame<'_>, app: &App, area: Rect) {
    let th = app.theme;
    let blk = pane_block("Test cases", "]t / [t", false, &th);
    let inner = blk.inner(area);
    f.render_widget(blk, area);
    let w = inner.width as usize;
    let mut lines: Vec<Ln> = vec![];
    match &app.stuki.checks {
        None => lines.push(Line::from(Span::styled(
            if app.stuki.running() {
                "⟳ running…"
            } else {
                "not run yet"
            },
            th.dim(),
        ))),
        Some(Err(m)) => lines.extend(wrapped(m, w, Style::default().fg(th.warn), "")),
        Some(Ok(v)) => {
            for (i, vd) in v.iter().enumerate() {
                let cur = i == app.stuki.shown_test;
                let (m, c) = match (vd.pass, ascii::on()) {
                    (true, true) => ("[ok]", th.ok),
                    (false, true) => ("[FAIL]", th.err),
                    (true, false) => ("✔", th.ok),
                    (false, false) => ("✘", th.err),
                };
                let mut s = Style::default().fg(c).add_modifier(Modifier::BOLD);
                if cur {
                    s = s.bg(th.sel_bg);
                }
                lines.push(Line::from(vec![
                    Span::raw(if cur { "▶ " } else { "  " }),
                    Span::styled(format!("{m} Test case {}", i + 1), s),
                ]));
                if cur || !vd.pass {
                    if !vd.message.is_empty() {
                        lines.extend(wrapped(&vd.message, w, th.text(), "    "));
                    }
                    if cur {
                        for (n, val) in &vd.values {
                            lines.extend(wrapped(&format!("{n} = {val}"), w, th.dim(), "      "));
                        }
                    }
                }
            }
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("── keys ──", th.dim())));
    for t in [
        "j/k move   h parent   l child",
        "w/b sibling   {/} structogram",
        "e edit text   cc replace",
        "o/O add after/before: pick s d w r c",
        "dd delete  yy copy  p/P paste",
        "J/K move node down/up",
        "+c/+d add case / default",
        "th/tv/tr toggle name/vars/return",
        "u / Ctrl-r undo / redo",
        "<Space>f build from spec",
        "<Space>c copy picture as text",
    ] {
        lines.push(Line::from(Span::styled(t, th.dim())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

// ------------------------------------------------------------------ status line

fn mode_chip(app: &App) -> (String, Color) {
    let th = app.theme;
    match app.tool {
        Tool::Stuki => {
            if app.stuki.edit.is_some() {
                (" INSERT ".into(), th.ok)
            } else if app.stuki.menu.is_some() {
                (" ADD ".into(), th.warn)
            } else {
                (" STRUCT ".into(), th.accent)
            }
        }
        Tool::Code => {
            if app.code.focus == CodePane::Output {
                return (" VIEW ".into(), th.accent);
            }
            let l = app.vim.mode().label();
            let c = match l {
                "INSERT" => th.ok,
                "VISUAL" | "V-LINE" => th.warn,
                _ => th.accent,
            };
            (format!(" {l} "), c)
        }
        Tool::Spec => {
            if app.focus == Pane::Eval {
                return (" VIEW ".into(), th.accent);
            }
            let l = app.vim.mode().label();
            let c = match l {
                "INSERT" => th.ok,
                "VISUAL" | "V-LINE" => th.warn,
                _ => th.accent,
            };
            (format!(" {l} "), c)
        }
    }
}

fn draw_status(f: &mut Frame<'_>, app: &App, area: Rect) {
    let th = app.theme;
    // a prompt line replaces the status
    if let Overlay::Line(kind, input) = &app.overlay {
        let prefix = match kind {
            LineKind::Command => ":",
            LineKind::Search => "/",
        };
        let (text, off) = input_view(prefix, input);
        f.render_widget(Paragraph::new(text).style(th.text()), area);
        f.set_cursor_position((area.x + off.min(area.width as usize - 1) as u16, area.y));
        return;
    }
    if let Some(e) = &app.stuki.edit
        && app.tool == Tool::Stuki
    {
        let prefix = " edit › ";
        let (text, off) = input_view(prefix, &e.input);
        f.render_widget(Paragraph::new(text).style(th.text()), area);
        f.set_cursor_position((area.x + off.min(area.width as usize - 1) as u16, area.y));
        return;
    }
    let (chip, col) = mode_chip(app);
    let mut spans = vec![
        Span::styled(
            chip,
            Style::default()
                .fg(th.bar_bg)
                .bg(col)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    let pending = match app.tool {
        Tool::Spec => app.vim.pending_keys(),
        Tool::Stuki => app.stuki.keys.iter().collect(),
        Tool::Code if app.code.focus != CodePane::Output => app.vim.pending_keys(),
        Tool::Code => String::new(),
    };
    let pending = if app.ctrl_w {
        "^W".to_string()
    } else {
        pending
    };
    if !pending.is_empty() {
        spans.push(Span::styled(
            format!("{pending} "),
            Style::default().fg(th.warn).add_modifier(Modifier::BOLD),
        ));
    }
    let msg = if !app.status.is_empty() {
        app.status.clone()
    } else if app.tool == Tool::Stuki && !app.stuki.status.is_empty() {
        app.stuki.status.clone()
    } else if app.tool == Tool::Code && !app.code.status.is_empty() {
        app.code.status.clone()
    } else {
        String::new()
    };
    spans.push(Span::styled(msg, th.text()));
    let right = match app.tool {
        Tool::Spec => {
            let ok = app.results.iter().flatten().filter(|o| o.pass).count();
            let n = app.results.iter().flatten().count();
            let t = format!("tests {ok}/{n} ✔  ");
            let _ = &t;
            t
        }
        Tool::Stuki => match &app.stuki.checks {
            Some(Ok(v)) => format!(
                "tests {}/{} ✔  ",
                v.iter().filter(|x| x.pass).count(),
                v.len()
            ),
            _ => String::new(),
        },
        Tool::Code => format!("reads {}  ", app.code.source.label()),
    };
    let lw: usize = spans.iter().map(Span::width).sum();
    let rw = right.chars().count();
    if lw + rw < area.width as usize {
        spans.push(Span::raw(" ".repeat(area.width as usize - lw - rw)));
        spans.push(Span::styled(right, th.dim()));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bar_bg)),
        area,
    );
}

// ------------------------------------------------------------------ overlays

pub const HELP: &str = include_str!("help.txt");

fn draw_overlay(f: &mut Frame<'_>, app: &mut App, area: Rect) {
    let th = app.theme;
    // the Code tool's panes count as the editing panes, its output as the viewing one
    let focus = match app.tool {
        Tool::Code if app.code.focus == CodePane::Output => Pane::Eval,
        Tool::Code => Pane::Spec,
        _ => app.focus,
    };
    let tool = app.tool;
    match &mut app.overlay {
        Overlay::None | Overlay::Line(..) => {}
        Overlay::Help(scroll) => {
            let r = centered(area, 100, area.height.saturating_sub(2));
            f.render_widget(Clear, r);
            let blk = pane_block("Help", "j/k scroll · q closes", true, &th);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let lines = help_lines(&th, tool, focus);
            let h = inner.height as usize;
            *scroll = (*scroll).min(lines.len().saturating_sub(h));
            let shown: Vec<Ln> = lines.into_iter().skip(*scroll).take(h).collect();
            f.render_widget(Paragraph::new(shown), inner);
        }
        Overlay::Paste => {
            let r = centered(area, 64, 7);
            f.render_widget(Clear, r);
            let blk = pane_block("Paste a project", "Esc cancels", true, &th);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let t = "No system clipboard tool was found.\n\nPaste the copied project line now with your\nterminal's paste shortcut (Ctrl+Shift+V,\nCmd+V or right click).";
            f.render_widget(Paragraph::new(t).style(th.text()), inner);
        }
        Overlay::Confirm(crate::app::Pending::Convert) => {
            let r = centered(area, 66, 7);
            f.render_widget(Clear, r);
            let blk = pane_block("Replace the program?", "", true, &th);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let lines = vec![
                Line::from(Span::styled(
                    "The program was edited. Writing it again replaces your changes.",
                    th.text(),
                )),
                Line::default(),
                Line::from(vec![
                    Span::styled(" y ", th.selected()),
                    Span::styled(" replace   ", th.text()),
                    Span::styled(" Esc ", th.selected()),
                    Span::styled(" keep my program", th.text()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Overlay::Confirm(p) => {
            let what = match p {
                crate::app::Pending::Quit => "quit",
                crate::app::Pending::New => "start a new file",
                crate::app::Pending::Open | crate::app::Pending::Load(_) => "open another file",
                _ => "replace the current content",
            };
            let r = centered(area, 62, 7);
            f.render_widget(Clear, r);
            let blk = pane_block("Unsaved changes", "", true, &th);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let lines = vec![
                Line::from(Span::styled(
                    format!("There are unsaved changes. Really {what}?"),
                    th.text(),
                )),
                Line::default(),
                Line::from(vec![
                    Span::styled(" s ", th.selected()),
                    Span::styled(" save first   ", th.text()),
                    Span::styled(" d ", th.selected()),
                    Span::styled(" discard   ", th.text()),
                    Span::styled(" Esc ", th.selected()),
                    Span::styled(" cancel", th.text()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Overlay::Files(b) => {
            let r = centered(area, 90, area.height.saturating_sub(2).min(30));
            f.render_widget(Clear, r);
            let title = if b.save { "Save as" } else { "Open" };
            let right = format!(".{}  ·  a all  . hidden  / filter", b.kind.ext());
            let blk = pane_block(title, &right, true, &th);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            if inner.height < 4 {
                return;
            }
            let list_h = inner.height as usize - 3;
            b.height = list_h;
            if b.sel < b.scroll {
                b.scroll = b.sel;
            }
            if b.sel >= b.scroll + list_h {
                b.scroll = b.sel + 1 - list_h;
            }
            let mut lines: Vec<Ln> = vec![Line::from(Span::styled(
                b.dir.display().to_string(),
                th.accent(),
            ))];
            if b.entries.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (no matching files here)",
                    th.dim(),
                )));
            }
            for (i, e) in b.entries.iter().enumerate().skip(b.scroll).take(list_h) {
                let name = if e.dir {
                    format!("{}/", e.name)
                } else {
                    e.name.clone()
                };
                let st = if i == b.sel {
                    th.selected().add_modifier(Modifier::BOLD)
                } else if e.dir {
                    Style::default().fg(th.accent)
                } else {
                    th.text()
                };
                lines.push(Line::from(Span::styled(
                    format!(" {name:<w$}", w = inner.width as usize - 2),
                    st,
                )));
            }
            f.render_widget(
                Paragraph::new(lines),
                Rect {
                    height: inner.height - 2,
                    ..inner
                },
            );
            let y = inner.y + inner.height - 2;
            let msg = if !b.msg.is_empty() {
                b.msg.clone()
            } else if let Some(fl) = &b.filter {
                format!("filter: {}", fl.text())
            } else {
                "j/k move · l/Enter open · h up · ~ home · gg/G ends · q closes".into()
            };
            f.buffer_mut().set_string(
                inner.x,
                y,
                msg.chars().take(inner.width as usize).collect::<String>(),
                th.dim(),
            );
            let y = y + 1;
            if b.save {
                let label = "file name: ";
                let st = if b.naming { th.text() } else { th.dim() };
                let (shown, off) = input_view(label, &b.name);
                let end = unicode_width::UnicodeWidthStr::width(shown.as_str());
                f.buffer_mut().set_string(inner.x, y, shown, st);
                if b.naming {
                    f.set_cursor_position((inner.x + off as u16, y));
                } else {
                    f.buffer_mut().set_string(
                        inner.x + (end + 2) as u16,
                        y,
                        "(i edits the name)",
                        th.dim(),
                    );
                }
            }
            if let Some(fl) = &b.filter {
                let (_, off) = input_view("filter: ", fl);
                f.set_cursor_position((inner.x + off as u16, y - 1));
            }
        }
    }
    let _ = Menu::Insert { after: true };
}

/// How early a help section comes, depending on where the user is.
fn help_priority(tag: &str, tool: Tool, focus: Pane) -> u8 {
    let editing_here = tool != Tool::Stuki && focus != Pane::Eval;
    let eval_here = tool == Tool::Spec && focus == Pane::Eval;
    match (tag, tool) {
        ("general", _) => 0,
        ("spec", Tool::Spec) | ("stuki", Tool::Stuki) | ("code", Tool::Code) => 1,
        ("editing", _) if editing_here => 1,
        ("eval", _) if eval_here => 1,
        ("editing" | "eval", Tool::Spec) | ("editing", Tool::Code) => 2,
        ("files", _) => 7,
        ("about", _) => 9,
        _ => 5,
    }
}

/// The help text, the sections that matter here first and marked "(here)".
pub fn help_lines(th: &Theme, tool: Tool, focus: Pane) -> Vec<Ln> {
    let mut sections: Vec<(String, u8, Vec<&str>)> = vec![];
    for line in HELP.lines() {
        if let Some(h) = line.strip_prefix("# ") {
            let (title, tag) = h.rsplit_once(" @").unwrap_or((h, "general"));
            sections.push((title.to_string(), help_priority(tag, tool, focus), vec![]));
        } else if let Some(s) = sections.last_mut() {
            s.2.push(line);
        }
    }
    sections.sort_by_key(|s| s.1);
    let mut out: Vec<Ln> = vec![];
    for (title, prio, body) in sections {
        let here = match (prio == 1, ascii::on()) {
            (false, _) => "",
            (true, true) => "  <- you are here",
            (true, false) => "  ← you are here",
        };
        out.push(Line::from(vec![
            Span::styled(ascii::text(&title).into_owned(), th.accent()),
            Span::styled(here, th.dim()),
        ]));
        for l in body {
            out.push(match l.split_once("  ·  ") {
                Some((k, d)) => Line::from(vec![
                    Span::styled(
                        format!("{:<24}", ascii::text(k)),
                        Style::default().fg(th.warn),
                    ),
                    Span::styled(ascii::text(d).into_owned(), th.text()),
                ]),
                None => Line::from(Span::styled(ascii::text(l).into_owned(), th.text())),
            });
        }
    }
    out
}
