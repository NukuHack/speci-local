//! Tests that need the .NET SDK (`dotnet` on the PATH): they build and run real C# programs,
//! which takes seconds per run, so they live here and not next to the unit tests. See
//! `common/mod.rs` for how they are skipped without the SDK.

use speci_local::csharp::{self, Converted, Inputs};
use speci_local::dotnet::{Msg, Run, error_lines};
use speci_local::stuki::model::Program;
use std::time::{Duration, Instant};

mod common;
use common::{LIMIT, run, serial};

const SPEC: &str = "Be:\nn∈N,\nszamok∈N[1..n]\n\nKi:\ndb∈N\n\nEf:\n-\n\nUf:\ndb=DARAB(i=1..n, szamok[i] mod 2 = 1)\n";
const TEST: &str = "n: 5\nszamok: [3, 72, 81, 4, 5]\ndb: 3\n";
const FOREVER: &str = "using System;\nclass Program { static void Main() { Console.WriteLine(\"up\"); while (true) { } } }\n";

fn convert(inputs: Inputs) -> Converted {
    match csharp::convert(SPEC, "odd numbers", &Program::default(), TEST, inputs) {
        Ok(c) => c,
        Err(csharp::ConvertError::Spec(e) | csharp::ConvertError::Test(e)) => {
            panic!("conversion failed: {e}")
        }
    }
}

/// The program with the test's values written into it prints them.
#[test]
fn program_with_test_values_runs() {
    let Some(_guard) = serial() else { return };
    let c = convert(Inputs::Test);
    let f = run(&c.code, "");
    assert_eq!(f.output.trim(), "db = 3", "{}\n{}", f.build, f.done);
    assert!(f.done.starts_with("Finished"), "{}", f.done);
}

/// The console program reads the test case's input, and what it prints is what the test case
/// expects (and another input gives another output).
#[test]
fn console_program_matches_the_test_case() {
    let Some(_guard) = serial() else { return };
    let c = convert(Inputs::Console);
    let stdin = c.stdin.expect("console input");
    assert_eq!(stdin, "5\n3 72 81 4 5\n");
    let spec = speci_local::parse(SPEC).expect("the specification parses");
    let expected = csharp::console_output(&spec, TEST).expect("expected output");

    let f = run(&c.code, &stdin);
    assert_eq!(f.output.trim(), "3", "{}\n{}", f.build, f.done);
    assert!(csharp::same_output(&expected, &f.output));

    let f = run(&c.code, "7\n1 3 5 7 9 11 13\n");
    assert!(!csharp::same_output(&expected, &f.output), "{}", f.output);
}

/// A compile error is reported with the lines it points to.
#[test]
fn compile_error_names_its_line() {
    let Some(_guard) = serial() else { return };
    let c = convert(Inputs::Test);
    let broken = c.code.replace("db = db + 1;", "db = db + oops;");
    assert_ne!(broken, c.code, "the generated program changed shape");
    let f = run(&broken, "");
    assert!(f.build.contains("error CS0103"), "{}\n{}", f.build, f.done);
    assert!(!error_lines(&f.build).is_empty(), "{}", f.build);
}

/// Stopping, or just dropping a run, kills the program itself.
#[test]
fn dropped_run_leaves_nothing_running() {
    let Some(_guard) = serial() else { return };
    for explicit_stop in [true, false] {
        let run = Run::start(FOREVER.into(), String::new(), false, || {});
        // wait until the program says it is up
        let mut out = String::new();
        let start = Instant::now();
        while !out.contains("up") {
            assert!(start.elapsed() < LIMIT, "never started: {out}");
            while let Some(m) = run.try_recv() {
                match m {
                    Msg::Out(s) => out.push_str(&s),
                    Msg::Build(s) => panic!("{s}"),
                    Msg::Done(s) => panic!("ended early: {s}"),
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let alive = run.alive_flag();
        assert!(alive.load(std::sync::atomic::Ordering::Relaxed));
        if explicit_stop {
            run.stop();
            let start = Instant::now();
            loop {
                match run.try_recv() {
                    Some(Msg::Done(s)) => {
                        assert_eq!(s, "Stopped");
                        break;
                    }
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
                assert!(start.elapsed() < Duration::from_secs(10), "not stopped");
            }
        }
        drop(run);
        assert!(
            !alive.load(std::sync::atomic::Ordering::Relaxed),
            "the program still runs"
        );
    }
}
