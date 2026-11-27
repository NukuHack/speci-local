//! End-to-end tests: simulated keys, rendered into an in-memory terminal.

use crate::app::{App, Tool};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

pub fn keys(app: &mut App, s: &str) {
    let mut it = s.chars();
    while let Some(c) = it.next() {
        let ev = if c == '<' {
            let mut name = String::new();
            for d in it.by_ref() {
                if d == '>' {
                    break;
                }
                name.push(d);
            }
            match name.as_str() {
                "Esc" => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                "CR" => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                "Tab" => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                "S-Tab" => KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                "BS" => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                "lt" => KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE),
                "Space" => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
                "F5" => KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
                n if n.starts_with("C-") => KeyEvent::new(
                    KeyCode::Char(n.chars().nth(2).unwrap()),
                    KeyModifiers::CONTROL,
                ),
                n if n.starts_with("A-") => {
                    KeyEvent::new(KeyCode::Char(n.chars().nth(2).unwrap()), KeyModifiers::ALT)
                }
                n => panic!("unknown key <{n}>"),
            }
        } else {
            KeyEvent::new(
                KeyCode::Char(c),
                if c.is_uppercase() {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                },
            )
        };
        app.on_key(ev);
    }
}

/// Wait for the background evaluation to finish.
pub fn settle(app: &mut App) {
    for _ in 0..400 {
        app.tick();
        if !app.wants_tick() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("evaluation did not finish");
}

pub fn render(app: &mut App, w: u16, h: u16) -> String {
    app.tick();
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let buf = t.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn fresh() -> App {
    let mut a = App::new(true);
    settle(&mut a);
    a
}

#[test]
fn shows_three_panes_at_150x40() {
    let mut a = fresh();
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert_eq!(s.lines().count(), 40);
    for w in [
        "1 Specification",
        "2 Test data",
        "3 Evaluation",
        "Test case 1  passed",
        "DARAB",
        "NORMAL",
    ] {
        assert!(s.contains(w), "missing {w}");
    }
}

#[test]
fn every_size_renders_without_panicking() {
    for (w, h) in [
        (150, 40),
        (120, 30),
        (100, 24),
        (80, 24),
        (60, 20),
        (40, 12),
        (30, 8),
        (21, 6),
    ] {
        for tool in [Tool::Spec, Tool::Stuki, Tool::Code] {
            let mut a = fresh();
            a.tool = tool;
            let _ = render(&mut a, w, h);
            if tool == Tool::Code {
                keys(&mut a, "<Space>m");
                let _ = render(&mut a, w, h);
                keys(&mut a, "<Tab>");
                let _ = render(&mut a, w, h);
            }
            keys(&mut a, "<C-w>l");
            let _ = render(&mut a, w, h);
            keys(&mut a, "<C-w>l?");
            let _ = render(&mut a, w, h);
        }
    }
}

#[test]
fn panes_are_reachable_with_keys_only() {
    let mut a = fresh();
    keys(&mut a, "<Tab>");
    assert_eq!(a.focus, crate::app::Pane::Data);
    keys(&mut a, "<Tab>");
    assert_eq!(a.focus, crate::app::Pane::Eval);
    keys(&mut a, "<S-Tab><S-Tab>");
    assert_eq!(a.focus, crate::app::Pane::Spec);
    keys(&mut a, "<C-w>j");
    keys(&mut a, "<C-w>3");
    assert_eq!(a.focus, crate::app::Pane::Eval);
    keys(&mut a, "<C-w>o");
    assert!(a.zoom);
    let s = render(&mut a, 150, 40);
    assert!(!s.contains("1 Specification") || s.contains("[3 Eval]"));
}

#[test]
fn editing_the_spec_reevaluates_and_marks_errors() {
    let mut a = fresh();
    // break the postcondition: add a stray operator at the end of the last line
    keys(&mut a, "G$a +<Esc>");
    settle(&mut a);
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert!(
        a.spec_ed.errors.len() + a.spec_diags.len() > 0
            || a.results[0].as_ref().is_some_and(|o| !o.pass)
    );
    assert!(s.contains('✘'));
    // undo restores it
    keys(&mut a, "u");
    settle(&mut a);
    assert!(a.results[0].as_ref().unwrap().pass);
}

#[test]
fn tests_can_be_added_deleted_and_stepped_through() {
    let mut a = fresh();
    keys(&mut a, "<Space>t");
    assert_eq!(a.doc.tests.len(), 2);
    keys(&mut a, "<Space>s");
    assert!(a.doc.tests[1].contains("szamok"), "{:?}", a.doc.tests[1]);
    keys(&mut a, "<Esc>[t");
    assert_eq!(a.current, 0);
    keys(&mut a, "]t");
    assert_eq!(a.current, 1);
    keys(&mut a, "<Space>d");
    assert_eq!(a.doc.tests.len(), 1);
    // stepper
    settle(&mut a);
    keys(&mut a, "<C-w>3sl");
    assert_eq!(a.view, crate::app::View::Stepper);
    assert_eq!(a.step, 1);
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert!(s.contains("Step 2 of"));
    keys(&mut a, "S");
    let s = render(&mut a, 150, 40);
    assert!(s.contains("DARAB") || s.contains("Uf"));
}

#[test]
fn commands_work() {
    let mut a = fresh();
    keys(&mut a, ":set light<CR>");
    assert!(!a.theme.dark);
    keys(&mut a, ":tnew<CR>");
    assert_eq!(a.doc.tests.len(), 2);
    keys(&mut a, "<Esc>:t 1<CR>");
    assert_eq!(a.current, 0);
    keys(&mut a, "<C-w>1:3<CR>");
    assert_eq!(a.spec_ed.line_col().0, 3);
}

#[test]
fn save_and_open_roundtrip() {
    let dir = std::env::temp_dir().join(format!("speci-tui-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("x.sp");
    let mut a = fresh();
    keys(&mut a, &format!(":w {}<CR>", f.display()));
    assert!(f.exists());
    assert!(!a.dirty());
    keys(&mut a, "GoUf: x<Esc>");
    assert!(a.dirty());
    // :q asks first, d discards
    keys(&mut a, ":q<CR>");
    assert!(!a.quit);
    let s = render(&mut a, 100, 30);
    assert!(s.contains("Unsaved changes"));
    keys(&mut a, "d");
    assert!(a.quit);
    let mut b = App::new(true);
    b.load(&f);
    assert_eq!(b.doc.spec, fresh().doc.spec);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn structogram_from_spec_and_editing() {
    let mut a = fresh();
    keys(&mut a, "<Space>f");
    assert_eq!(a.tool, Tool::Stuki);
    settle(&mut a);
    for _ in 0..50 {
        a.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
        if a.stuki.checks.is_some() {
            break;
        }
    }
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert!(s.contains("┌") && s.contains("Test case 1"));
    // navigate and edit with the keyboard only
    keys(&mut a, "gg");
    keys(&mut a, "jj");
    let before = a.stuki.program.content();
    keys(&mut a, "cc");
    keys(&mut a, "x:=1<lt>=2<CR>");
    assert_ne!(a.stuki.program.content(), before);
    let s = render(&mut a, 150, 40);
    assert!(s.contains("x:=1≤2"), "{s}");
    keys(&mut a, "u");
    assert_eq!(a.stuki.program.content(), before);
    keys(&mut a, "<C-r>");
    assert_ne!(a.stuki.program.content(), before);
    // add a loop after the cursor, then delete it again
    let n = a.stuki.program.content();
    keys(&mut a, "ow");
    assert_ne!(a.stuki.program.content(), n);
    keys(&mut a, "dd");
    assert_eq!(a.stuki.program.content(), n);
}

#[test]
fn help_opens_and_scrolls() {
    let mut a = fresh();
    keys(&mut a, "?");
    let s = render(&mut a, 150, 40);
    assert!(s.contains("Leader key"));
    keys(&mut a, "jjjjG");
    let s = render(&mut a, 150, 40);
    assert!(s.contains("About") || s.contains("Close this help"));
    keys(&mut a, "q");
    assert!(matches!(a.overlay, crate::app::Overlay::None));
}

#[test]
fn dot_repeat_visual_and_search_work_in_the_editors() {
    let mut a = fresh();
    keys(&mut a, "<C-w>2");
    keys(&mut a, "ggOa<Esc>j.");
    assert!(a.doc.tests[0].starts_with("a\n"), "{:?}", a.doc.tests[0]);
    keys(&mut a, "ggvjd");
    assert!(!a.doc.tests[0].starts_with("a\na"));
    keys(&mut a, "u");
    keys(&mut a, "gg/szamok<CR>");
    let (line, _) = a.data().line_col();
    assert!(
        a.doc.tests[0]
            .lines()
            .nth(line - 1)
            .unwrap()
            .contains("szamok")
    );
}

#[test]
fn failing_test_shows_the_problem_and_enter_jumps_to_it() {
    let mut a = fresh();
    keys(&mut a, "<C-w>2");
    keys(&mut a, "/db: 3<CR>");
    keys(&mut a, "$r9<Esc>");
    settle(&mut a);
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert!(s.contains("✘ Test case 1  failed"));
    keys(&mut a, "<C-w>3");
    keys(&mut a, "<CR>");
    assert!(
        a.focus != crate::app::Pane::Eval,
        "should jump to the location"
    );
}

#[test]
fn paste_of_a_project_line_imports_it() {
    let mut a = fresh();
    let line = speci_local::file::to_compact(
        &speci_local::file::Doc {
            title: "T".into(),
            spec: "Be:\nx∈Z\n\nKi:\n\nEf:\n-\n\nUf:\n".into(),
            tests: vec!["x: 1\n".into()],
        },
        &speci_local::stuki::model::Program::default(),
    );
    a.on_paste(&line);
    assert_eq!(a.doc.title, "T");
    assert!(a.doc.spec.contains("x∈Z"));
}

fn help_order(app: &App) -> Vec<String> {
    ui::help_lines(&app.theme, app.tool, app.focus)
        .iter()
        .map(ToString::to_string)
        .filter(|l| {
            [
                "Moving around",
                "Specification tool",
                "Editing text",
                "Evaluation pane",
                "Structogram: moving",
                "Structogram: leader",
                "Files",
            ]
            .iter()
            .any(|h| l.starts_with(h))
        })
        .collect()
}

fn position(order: &[String], head: &str) -> usize {
    order.iter().position(|l| l.starts_with(head)).unwrap()
}

#[test]
fn help_puts_the_current_context_first() {
    let mut a = fresh();
    // typing in the specification: general, then editing, before the structogram sections
    let o = help_order(&a);
    assert!(position(&o, "Moving around") < position(&o, "Editing text"));
    assert!(position(&o, "Editing text") < position(&o, "Evaluation pane"));
    assert!(position(&o, "Evaluation pane") < position(&o, "Structogram: moving"));
    // in the evaluation pane that one comes before the text editing
    keys(&mut a, "<C-w>3");
    let o = help_order(&a);
    assert!(position(&o, "Evaluation pane") < position(&o, "Editing text"));
    // in the structogram tool its own sections lead and the editor ones sink
    keys(&mut a, "<Space>2");
    let o = help_order(&a);
    assert!(position(&o, "Moving around") < position(&o, "Structogram: moving"));
    assert!(position(&o, "Structogram: moving") < position(&o, "Editing text"));
    assert!(position(&o, "Structogram: leader") < position(&o, "Files"));
    let s = ui::help_lines(&a.theme, a.tool, a.focus)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(s.contains("Structogram: moving and editing  ← you are here"));
}

#[test]
fn leader_hint_depends_on_the_tool() {
    let mut a = fresh();
    keys(&mut a, "<Space>");
    assert!(a.status.contains("f →structogram") && a.status.contains("scaffold"));
    keys(&mut a, "<Esc><Space>2<Space>");
    assert!(a.status.contains("build from spec") && !a.status.contains("scaffold"));
    keys(&mut a, "<Esc><Space>3<Space>");
    assert!(a.status.contains("from spec+stuki") && a.status.contains("run"));
}

#[test]
#[ignore = "prints the help for a visual check"]
fn show_help_screens() {
    let mut a = fresh();
    keys(&mut a, "?");
    let _ = std::fs::write("/tmp/help_spec.txt", render(&mut a, 150, 40));
    keys(&mut a, "q<Space>2?");
    let _ = std::fs::write("/tmp/help_stuki.txt", render(&mut a, 150, 40));
}

fn assert_ascii(screen: &str, what: &str) {
    if let Some((i, c)) = screen.char_indices().find(|(_, c)| !c.is_ascii()) {
        let line = screen[..i].lines().count();
        panic!("{what}: non-ASCII {c:?} on line {line}\n{screen}");
    }
}

#[test]
fn ascii_mode_draws_only_ascii_in_every_view() {
    let mut a = fresh();
    a.set_ascii(true);
    let s = render(&mut a, 150, 40);
    assert_ascii(&s, "spec tool");
    // symbols show as the words the language accepts, accents are folded
    assert!(s.contains("n in N,"), "{s}");
    assert!(s.contains("Paratlan szamok"));
    assert!(s.contains("[ok] Test case 1  passed"));
    assert!(s.contains('#'), "focused border is heavier in ASCII");
    // failing test, stepper, step list
    keys(&mut a, "<C-w>2/db: 3<CR>$r9<Esc>");
    settle(&mut a);
    assert_ascii(&render(&mut a, 150, 40), "failing test");
    keys(&mut a, "<C-w>3");
    assert!(render(&mut a, 150, 40).contains("[FAIL]"));
    for view in ["s", "S", "r"] {
        keys(&mut a, view);
        assert_ascii(&render(&mut a, 150, 40), "evaluation view");
    }
    // overlays
    for open in ["?", "<Esc>:<CR>", "<Space>o", "<Space>w"] {
        keys(&mut a, open);
        assert_ascii(&render(&mut a, 150, 40), open);
        keys(&mut a, "<Esc><Esc>");
        keys(&mut a, "q");
        a.overlay = crate::app::Overlay::None;
    }
    // narrow layouts too
    for (w, h) in [(100, 30), (60, 20)] {
        assert_ascii(&render(&mut a, w, h), "narrow");
    }
}

#[test]
fn ascii_structogram_is_plain_text_art() {
    let mut a = fresh();
    a.set_ascii(true);
    keys(&mut a, "<Space>f");
    settle(&mut a);
    for _ in 0..50 {
        a.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
        if a.stuki.checks.is_some() {
            break;
        }
    }
    let s = render(&mut a, 150, 40);
    println!("{s}");
    assert_ascii(&s, "structogram");
    assert!(s.contains('+') && s.contains('|') && s.contains('-'));
    // editing text with symbols: the edit line spells them out
    keys(&mut a, "ggjjcc");
    keys(&mut a, "i<lt>=3");
    let s = render(&mut a, 150, 40);
    assert_ascii(&s, "edit line");
    assert!(s.contains("i<=3"));
    keys(&mut a, "<CR>");
    // the exported picture is ASCII as well
    let words = crate::stuki::Stuki::layout_words(&a.doc.spec);
    let picture = crate::nslayout::to_text(&crate::nslayout::layout(
        &a.stuki.program,
        100,
        &words,
        None,
    ));
    assert_ascii(&picture, "picture");
}

#[test]
fn ascii_cursor_stays_on_the_character_it_belongs_to() {
    let mut a = fresh();
    a.set_ascii(true);
    keys(&mut a, "ggdGi∈x<Esc>");
    // `∈` shows as " in " (4 columns), so the x is at column 4 after the line number gutter
    let _ = render(&mut a, 150, 40);
    let gutter = 4; // two digits + two spaces
    let x0 = a.spec_ed.cursor_xy.unwrap().0;
    keys(&mut a, "0");
    let _ = render(&mut a, 150, 40);
    let left = a.spec_ed.cursor_xy.unwrap().0;
    assert_eq!(left, 1 + gutter, "column 0 of the spec pane");
    assert_eq!(x0 - left, 4, "after the 4-column symbol");
}

#[test]
fn backslash_names_type_symbols_without_alt() {
    let mut a = fresh();
    keys(&mut a, "ggdGi\\all x\\in y \\le z \\ne w<Esc>");
    assert_eq!(a.doc.spec, "∀ x∈ y ≤ z ≠ w");
    // converted on Tab too, and on Esc at the end of the line
    keys(&mut a, "ggdGi\\ex<Tab>y \\not<Esc>");
    assert_eq!(a.doc.spec, "∃y ¬");
    // unknown names are left alone
    keys(&mut a, "ggdGi\\foo bar<Esc>");
    assert_eq!(a.doc.spec, "\\foo bar");
    // the structogram text line has them too
    keys(&mut a, "<Space>2");
    keys(&mut a, "ggjjcc");
    keys(&mut a, "i\\le 3<CR>");
    assert!(a.stuki.program.content().contains('≤'));
}

#[test]
fn partial_redraws_stay_correct_in_ascii_mode() {
    // one long-lived terminal: only changed cells are sent, like on a real screen
    let mut a = fresh();
    a.set_ascii(true);
    let mut live = Terminal::new(TestBackend::new(150, 40)).unwrap();
    let dump = |t: &Terminal<TestBackend>| {
        let buf = t.backend().buffer();
        (0..40)
            .map(|y| {
                (0..150)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    for step in [
        "", "j", "jjj", "G", "<C-d>", "gg", "<Tab>", "<Tab>", "<Space>f", "j", "j", "o", "<Esc>",
        "<Space>1", "?", "jjj", "q",
    ] {
        keys(&mut a, step);
        a.tick();
        live.draw(|f| ui::draw(f, &mut a)).unwrap();
        let mut fresh_term = Terminal::new(TestBackend::new(150, 40)).unwrap();
        fresh_term.draw(|f| ui::draw(f, &mut a)).unwrap();
        let (l, f) = (dump(&live), dump(&fresh_term));
        assert_ascii(&l, step);
        assert_eq!(
            l, f,
            "after {step:?} the incremental screen differs from a full redraw"
        );
    }
}

#[test]
#[ignore = "prints ASCII screens for a visual check"]
fn show_ascii_screens() {
    let mut a = fresh();
    a.set_ascii(true);
    keys(&mut a, "<C-w>2/db: 3<CR>$r9<Esc>");
    settle(&mut a);
    let _ = std::fs::write("/tmp/ascii_spec.txt", render(&mut a, 150, 40));
    keys(&mut a, "<C-w>1<Space>f");
    settle(&mut a);
    for _ in 0..50 {
        a.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = std::fs::write("/tmp/ascii_stuki.txt", render(&mut a, 150, 40));
}

// ------------------------------------------------------------------ the Code tool

fn code_app() -> App {
    let mut a = fresh();
    keys(&mut a, "<Space>3");
    let _ = render(&mut a, 150, 40);
    a
}

/// Waits for the build and the run (a build takes a few seconds).
fn finish_run(a: &mut App) {
    for _ in 0..6000 {
        a.tick();
        if !a.code.running() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the run never finished");
}

fn have_dotnet() -> bool {
    std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .is_ok()
}

#[test]
fn the_tools_go_round_with_gt_and_f2() {
    let mut a = fresh();
    assert_eq!(a.tool, Tool::Spec);
    keys(&mut a, "gt");
    assert_eq!(a.tool, Tool::Stuki);
    keys(&mut a, "gt");
    assert_eq!(a.tool, Tool::Code);
    keys(&mut a, "gt");
    assert_eq!(a.tool, Tool::Spec);
    keys(&mut a, "gT");
    assert_eq!(a.tool, Tool::Code);
    keys(&mut a, "gT");
    assert_eq!(a.tool, Tool::Stuki);
    keys(&mut a, "<Space>3");
    assert_eq!(a.tool, Tool::Code);
    a.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(a.tool, Tool::Spec);
    keys(&mut a, ":code<CR>");
    assert_eq!(a.tool, Tool::Code);
    let s = render(&mut a, 150, 40);
    assert!(s.contains("3 Code"), "{s}");
}

#[test]
fn the_first_visit_writes_the_program_and_that_is_not_a_change() {
    let mut a = code_app();
    let text = a.code.text();
    assert!(text.contains("class Program"), "{text}");
    assert!(
        text.contains("szamok = new int[] { 0, 3, 72, 81, 4, 5 };"),
        "{text}"
    );
    assert!(!a.code.dirty() && !a.project_dirty());
    let s = render(&mut a, 150, 40);
    for w in [
        "1 C# program",
        "3 Output",
        "test values",
        "test 1/1",
        "NORMAL",
        "Program written",
    ] {
        assert!(s.contains(w), "missing {w}\n{s}");
    }
    // test mode has no console input
    assert!(!s.contains("Console input"), "{s}");
}

#[test]
fn the_console_mode_has_the_input_and_the_expected_output() {
    let mut a = code_app();
    keys(&mut a, "<Space>m");
    let text = a.code.text();
    assert!(
        text.contains("Console.ReadLine()") && !text.contains("{ 0, 3, 72"),
        "{text}"
    );
    assert_eq!(a.code.input.text(), "5\n3 72 81 4 5\n");
    let s = render(&mut a, 150, 40);
    for w in ["2 Console input", "Expected output (test 1)", "console"] {
        assert!(s.contains(w), "missing {w}\n{s}");
    }
    // Tab goes program → input → output → program
    assert_eq!(a.code.focus, crate::code::CodePane::Editor);
    keys(&mut a, "<Tab>");
    assert_eq!(a.code.focus, crate::code::CodePane::Input);
    keys(&mut a, "<Tab>");
    assert_eq!(a.code.focus, crate::code::CodePane::Output);
    keys(&mut a, "<Tab>");
    assert_eq!(a.code.focus, crate::code::CodePane::Editor);
    // the expected output, and nothing to compare yet
    let spec = a.doc.spec.clone();
    let test = a.doc.tests[0].clone();
    assert_eq!(a.code.expected(&spec, &test), Ok("3\n".to_string()));
    assert_eq!(a.code.verdict(&spec, &test), None);
    // back to the test values: the program is written again, the input box is hidden
    keys(&mut a, "<Space>m");
    assert!(a.code.text().contains("{ 0, 3, 72"));
    assert!(!render(&mut a, 150, 40).contains("Console input"));
}

#[test]
fn an_edited_program_is_kept_and_converting_asks_first() {
    let mut a = code_app();
    keys(&mut a, "Gomine<Esc>");
    assert!(a.code.text().contains("mine"));
    assert!(a.code.dirty() && a.project_dirty());
    // the mode changes, the program stays
    keys(&mut a, "<Space>m");
    assert!(a.code.text().contains("mine") && !a.code.text().contains("Console.ReadLine()"));
    assert!(a.code.status.contains("kept"), "{}", a.code.status);
    // writing it again asks
    keys(&mut a, "<Space>f");
    let s = render(&mut a, 120, 30);
    assert!(s.contains("Replace the program?"), "{s}");
    keys(&mut a, "<Esc>");
    assert!(a.code.text().contains("mine"));
    keys(&mut a, "<Space>f");
    keys(&mut a, "y");
    assert!(!a.code.text().contains("mine") && a.code.text().contains("Console.ReadLine()"));
}

#[test]
fn csharp_editing_indents_and_tabs() {
    let mut a = code_app();
    keys(&mut a, "ggdGiclass A {<CR>x;<Esc>");
    assert_eq!(a.code.text(), "class A {\n    x;");
    keys(&mut a, "ggdGi{}<Esc>i<CR><Esc>");
    assert_eq!(a.code.text(), "{\n    \n}");
    keys(&mut a, "ggdGi<Tab>y<Esc>");
    assert_eq!(a.code.text(), "    y");
    // and the highlighting does not disturb the text
    let _ = render(&mut a, 150, 40);
}

#[test]
fn a_test_case_change_rewrites_an_unedited_program() {
    let mut a = code_app();
    assert_eq!(a.code.key, Some((crate::code::Source::Test, 0)));
    // a new test case (it is selected) has no values yet: the program can't be written for it
    keys(&mut a, "<Space>1<Space>t");
    assert_eq!((a.doc.tests.len(), a.current), (2, 1));
    keys(&mut a, "<Space>3");
    let _ = render(&mut a, 150, 40);
    assert!(a.code.status.contains("Can't convert"), "{}", a.code.status);
    // back to the first: the program is written again, as long as nobody edited it
    keys(&mut a, "[t");
    let _ = render(&mut a, 150, 40);
    assert_eq!(a.current, 0);
    assert!(
        a.code.status.contains("Program written"),
        "{}",
        a.code.status
    );
    assert!(a.code.unedited());
    // an edited program is left alone
    keys(&mut a, "Gomine<Esc>]t");
    let _ = render(&mut a, 150, 40);
    assert!(a.code.text().contains("mine"));
}

#[test]
fn the_project_file_holds_the_program_and_the_structograms() {
    let dir = std::env::temp_dir().join(format!("speci-tui-code-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let plain = dir.join("plain.sp");
    let f = dir.join("p.sp");

    // never visited the Code tool: a plain file, as before
    let mut a = fresh();
    keys(&mut a, &format!(":w {}<CR>", plain.display()));
    let text = std::fs::read_to_string(&plain).unwrap();
    assert!(
        !text.contains("--- code ---") && !text.contains("--- stuki ---"),
        "{text}"
    );

    // structograms and a program, saved from the Code tool
    keys(&mut a, "<Space>f"); // the structograms
    assert_eq!(a.tool, Tool::Stuki);
    keys(&mut a, "<Space>3");
    let _ = render(&mut a, 150, 40);
    keys(&mut a, "Gomine<Esc>");
    assert!(a.code.dirty());
    keys(&mut a, &format!(":w {}<CR>", f.display()));
    assert!(!a.code.dirty() && !a.project_dirty() && !a.stuki.dirty());
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains("/*--- code ---\nusing System;"), "{text}");
    assert!(text.contains("/*--- stuki ---"), "{text}");

    // opened by another window: all of it is back
    let mut b = App::new(true);
    b.load(&f);
    assert_eq!(b.doc.spec, a.doc.spec);
    // the file keeps the program without its trailing blank space
    assert_eq!(b.code.text().trim_end(), a.code.text().trim_end());
    assert!(!b.code.dirty());
    assert_eq!(
        b.stuki.program.diagrams.len(),
        a.stuki.program.diagrams.len()
    );
    assert!(!b.stuki.program.diagrams.is_empty());
    // it was opened from a file: converting asks first
    b.tool = Tool::Code;
    keys(&mut b, "<Space>f");
    assert!(render(&mut b, 120, 30).contains("Replace the program?"));
    // a file without a program empties the editor, and the next visit fills it again
    let mut c = fresh();
    c.tool = Tool::Code;
    let _ = render(&mut c, 100, 30);
    c.load(&plain);
    assert!(c.code.is_empty());
    let _ = render(&mut c, 100, 30);
    assert!(c.code.text().contains("class Program") && !c.code.dirty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn notes_list_what_the_conversion_could_not_do() {
    let mut a = code_app();
    keys(&mut a, ":notes<CR>");
    assert!(a.status.contains("nothing to point out"), "{}", a.status);
    // a note (made up here) is listed in the output pane
    a.code.notes = vec!["`x` can't be translated".into()];
    keys(&mut a, ":notes<CR>");
    assert!(
        a.code.output.contains("- `x` can't be translated"),
        "{}",
        a.code.output
    );
    assert!(render(&mut a, 120, 30).contains("Things to check"));
}

#[test]
fn the_help_leads_with_the_code_tool() {
    let mut a = code_app();
    let lines = ui::help_lines(&a.theme, a.tool, a.focus);
    let order: Vec<String> = lines
        .iter()
        .map(ToString::to_string)
        .filter(|l| {
            [
                "Moving around",
                "C# code",
                "Editing text",
                "Evaluation pane",
                "Files",
            ]
            .iter()
            .any(|h| l.starts_with(h))
        })
        .collect();
    assert!(position(&order, "Moving around") < position(&order, "C# code"));
    assert!(position(&order, "C# code") < position(&order, "Editing text"));
    assert!(order[position(&order, "C# code")].contains("you are here"));
    keys(&mut a, "?");
    assert!(render(&mut a, 150, 40).contains("C# code"));
}

#[test]
fn the_code_tool_draws_only_ascii_in_ascii_mode() {
    let mut a = code_app();
    a.set_ascii(true);
    assert_ascii(&render(&mut a, 150, 40), "code tool");
    keys(&mut a, "<Space>m");
    assert_ascii(&render(&mut a, 150, 40), "code tool, console");
    for (w, h) in [(100, 30), (60, 20)] {
        assert_ascii(&render(&mut a, w, h), "narrow code tool");
    }
    keys(&mut a, "?");
    assert_ascii(&render(&mut a, 150, 40), "code help");
}

#[test]
#[ignore = "prints the Code tool screens for a visual check"]
fn show_code_screens() {
    if have_dotnet() {
        let mut a = code_app();
        keys(&mut a, "<Space>m<Space>r");
        finish_run(&mut a);
        let _ = std::fs::write("/tmp/code_console.txt", render(&mut a, 150, 40));
        keys(&mut a, "<Space>m");
        let _ = std::fs::write("/tmp/code_test.txt", render(&mut a, 150, 40));
        let _ = std::fs::write("/tmp/code_narrow.txt", render(&mut a, 70, 24));
    }
}

#[test]
fn functions_setting_is_saved_with_the_project_and_set_again() {
    let dir = std::env::temp_dir().join(format!("speci-tui-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("cfg.sp");
    let mut a = code_app();
    assert!(!a.code.text().contains("static void Input"));
    // Space F: Input, Process and Output become functions; the unedited program is rewritten
    keys(&mut a, "<Space>F");
    assert!(a.code.functions);
    assert!(
        a.code.text().contains("static void Process("),
        "{}",
        a.code.text()
    );
    assert!(a.code.dirty());
    assert!(render(&mut a, 150, 40).contains("functions"));
    keys(&mut a, "<Space>m");
    assert_eq!(a.code.source, crate::code::Source::Console);
    keys(&mut a, &format!(":w {}<CR>", f.display()));
    assert!(!a.code.dirty());
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains("// speci-local: {"), "{text}");
    assert!(text.contains("\"functions\":true"), "{text}");

    let mut b = App::new(true);
    b.load(&f);
    assert!(b.code.functions && b.code.source == crate::code::Source::Console);
    assert!(!b.code.text().contains("// speci-local"));
    assert!(!b.code.dirty());
    // :functions off keeps an edited program and says so
    b.tool = Tool::Code;
    keys(&mut b, ":functions off<CR>");
    assert!(!b.code.functions && b.code.text().contains("static void Process("));
    assert!(b.code.status.contains("kept"), "{}", b.code.status);
    assert!(b.code.dirty());

    // a new project has the defaults again
    let mut c = code_app();
    keys(&mut c, "<Space>F");
    assert!(c.code.dirty());
    c.code.mark_saved(); // (a project with unsaved changes asks before it is replaced)
    keys(&mut c, "<Space>n");
    assert!(!c.code.functions);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reduction_table_is_written_under_the_specification() {
    let mut a = fresh();
    assert_eq!(a.tool, Tool::Spec);
    keys(&mut a, "<Space>g");
    let spec = a.spec_ed.text();
    assert!(spec.contains("/* Visszavezetés"), "{spec}");
    assert!(spec.contains("db = DARAB(i=e..u, T(i))"), "{spec}");
    assert_eq!(a.doc.spec, spec);
    assert!(speci_local::parse(&spec).is_ok(), "{spec}");
    // again: updated, not doubled
    keys(&mut a, ":reduction<CR>");
    assert_eq!(a.spec_ed.text(), spec);
    // nothing to reduce
    a.spec_ed.set_text("Be: n∈N\nKi: m∈N\nEf: -\nUf: m=n+1\n");
    keys(&mut a, "<Space>g");
    assert!(a.status.contains("nothing to reduce"), "{}", a.status);
    assert!(!a.spec_ed.text().contains("Visszavezetés"));
}

// ------------------------------------------------------------------ links and conversions

#[test]
fn the_code_tool_turns_its_program_into_structograms() {
    // the program the tool writes from the specification converts back
    let mut a = code_app();
    keys(&mut a, "<Space>s");
    assert_eq!(a.tool, Tool::Stuki);
    assert!(!a.stuki.program.diagrams.is_empty());
    assert!(
        a.status.contains("Structograms built from the C# program"),
        "{}",
        a.status
    );
    // and a hand-written one, with two methods
    let mut a = code_app();
    a.code.reset(
        Some(speci_local::stuki::from_csharp::DEMO_SOURCE.to_string()),
        None,
    );
    keys(&mut a, ":fromcode<CR>");
    assert_eq!(a.tool, Tool::Stuki);
    assert_eq!(a.stuki.program.diagrams.len(), 2);
    assert!(a.status.contains("Main, Fibonacci"), "{}", a.status);
    // a program without a method says why
    let mut a = code_app();
    a.code.reset(Some("class A { int x; }".into()), None);
    keys(&mut a, "<Space>s");
    assert_eq!(a.tool, Tool::Code);
    assert!(a.status.contains("Can't make structograms"), "{}", a.status);
}

#[test]
fn the_conversion_commands_work_without_a_clipboard() {
    let mut a = code_app();
    a.code.reset(
        Some(speci_local::stuki::from_csharp::DEMO_SOURCE.to_string()),
        None,
    );
    keys(&mut a, ":csurl<CR>");
    assert!(
        a.status.contains("2 method(s): Main, Fibonacci"),
        "{}",
        a.status
    );
    keys(&mut a, ":csline<CR>");
    assert!(a.status.contains("copied"), "{}", a.status);
    // a link given as an argument: to a line, to the content (also into a file)
    let url = speci_local::convert::run(
        speci_local::convert::Op::CsToUrl,
        speci_local::stuki::from_csharp::DEMO_SOURCE,
    )
    .unwrap()
    .text;
    a.command(&format!("toline {url}"));
    assert!(a.status.contains("2 structogram(s)"), "{}", a.status);
    let out = std::env::temp_dir().join("speci_tui_decode_test.json");
    let _ = std::fs::remove_file(&out);
    a.command(&format!("decode {url} {}", out.display()));
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("\"Fibonacci\"")
    );
    let _ = std::fs::remove_file(&out);
    a.command("tourl hello world");
    assert_ne!(a.status, "");
    a.command("weblink");
    assert!(a.status.contains("web editor link"), "{}", a.status);
}

#[test]
fn the_conversion_menu_is_documented_on_screen() {
    let mut a = fresh();
    keys(&mut a, "<Space>u");
    let s = render(&mut a, 150, 40);
    for want in ["Convert links", "web editor link", "C# program", "STUKI1"] {
        assert!(s.contains(want), "{want}\n{s}");
    }
    keys(&mut a, "<Esc>");
    assert!(!render(&mut a, 150, 40).contains("Convert links"));
    // the help lists the keys and the commands
    let lines: Vec<String> = ui::help_lines(&a.theme, a.tool, a.focus)
        .iter()
        .map(ToString::to_string)
        .collect();
    let all = lines.join("\n");
    for want in ["Space u", ":decode", ":csurl", ":weblink", ":fromcode"] {
        assert!(all.contains(want), "help lacks {want}");
    }
    // ASCII mode keeps the menu 7-bit
    keys(&mut a, "<Space>A<Space>u");
    let s = render(&mut a, 150, 40);
    assert!(
        s.is_ascii(),
        "{}",
        s.chars().find(|c| !c.is_ascii()).unwrap()
    );
}
