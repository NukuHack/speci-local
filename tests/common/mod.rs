//! Helpers of the tests that need the .NET SDK (`dotnet` on the PATH): they build and run real
//! C# programs, which takes seconds per run.
//!
//! Without `dotnet` the tests are skipped with a message. Set `SPECI_REQUIRE_DOTNET=1`
//! (e.g. on a CI machine that has the SDK) to turn the skip into a failure.
//!
//! The runs of one test binary share one project directory in the temp dir, so its tests take
//! turns ([`serial`]).

#![allow(dead_code)] // each test binary uses some of it

use speci_local::dotnet::{Msg, Run};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A build takes a few seconds, the first one on a machine much longer.
pub const LIMIT: Duration = Duration::from_secs(180);

/// Holds the lock for one test, or is `None` when there is no `dotnet` and the test is skipped.
pub fn serial() -> Option<MutexGuard<'static, ()>> {
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

/// What a finished run reported.
pub struct Finished {
    pub output: String,
    pub build: String,
    pub done: String,
}

pub fn finish(run: &Run) -> Finished {
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

pub fn run(text: &str, input: &str) -> Finished {
    finish(&Run::start(text.into(), input.into(), false, || {}))
}
