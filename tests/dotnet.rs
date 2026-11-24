//! Tests that need the .NET SDK (`dotnet` on the PATH): they build and run real C# programs,
//! which takes seconds per run, so they live here and not next to the unit tests.
//!
//! Without `dotnet` every test here is skipped with a message. Set `SPECI_REQUIRE_DOTNET=1`
//! (e.g. on a CI machine that has the SDK) to turn the skip into a failure.
//!
//! The runs share one project directory in the temp dir, so the tests take turns (`serial`).

use speci_local::csharp::{self, Converted, Inputs};
use speci_local::dotnet::{Msg, Run, error_lines};
use speci_local::stuki::model::Program;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const SPEC: &str = "Be:\nn∈N,\nszamok∈N[1..n]\n\nKi:\ndb∈N\n\nEf:\n-\n\nUf:\ndb=DARAB(i=1..n, szamok[i] mod 2 = 1)\n";
const TEST: &str = "n: 5\nszamok: [3, 72, 81, 4, 5]\ndb: 3\n";
const FOREVER: &str = "using System;\nclass Program { static void Main() { Console.WriteLine(\"up\"); while (true) { } } }\n";
/// A build takes a few seconds, the first one on a machine much longer.
const LIMIT: Duration = Duration::from_secs(180);

/// Holds the lock for one test, or is `None` when there is no `dotnet` and the test is skipped.
fn serial() -> Option<MutexGuard<'static, ()>> {
    static LOCK: Mutex<()> = Mutex::new(());
    let have = std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !have {
        assert!(
            std::env::var_os("SPECI_REQUIRE_DOTNET").is_none(),
            "SPECI_REQUIRE_DOTNET is set, but `dotnet` is not usable"
        );
        eprintln!("no dotnet: skipped");
        return None;
    }
    Some(LOCK.lock().unwrap_or_else(|e| e.into_inner()))
}

fn convert(inputs: Inputs) -> Converted {
    match csharp::convert(SPEC, "odd numbers", &Program::default(), TEST, inputs) {
        Ok(c) => c,
        Err(csharp::ConvertError::Spec(e) | csharp::ConvertError::Test(e)) => {
            panic!("conversion failed: {e}")
        }
    }
}

/// What a finished run reported.
struct Finished {
    output: String,
    build: String,
    done: String,
}

fn finish(run: &Run) -> Finished {
    let mut f = Finished {
        output: String::new(),
        build: String::new(),
        done: String::new(),
    };
    let start = Instant::now();
    loop {
        match run.try_recv() {
            Some(Msg::Out(s)) => f.output.push_str(&s),
            Some(Msg::Build(s)) => f.build.push_str(&s),
            Some(Msg::Done(s)) => {
                f.done = s;
                return f;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
        assert!(start.elapsed() < LIMIT, "the run never finished");
    }
}

fn run(text: &str, input: &str) -> Finished {
    finish(&Run::start(text.into(), input.into(), false, || {}))
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
