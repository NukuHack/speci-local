//! Building and running a C# program with the `dotnet` on the PATH, in the background: the
//! part of the Code tab that both the window app and the terminal app share. The caller gets
//! a [`Run`] to poll; `wake` is called whenever there is something new to poll (it asks the
//! UI to redraw).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub enum Msg {
    /// A piece of the program's output.
    Out(String),
    /// The build failed: its messages (`Program.cs(3,9): error CS…`).
    Build(String),
    /// The end, with a summary.
    Done(String),
}

/// A build-and-run in the background.
pub struct Run {
    rx: mpsc::Receiver<Msg>,
    stop: Arc<AtomicBool>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// The program itself is running (not just being built).
    alive: Arc<AtomicBool>,
    started: Instant,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run").finish_non_exhaustive()
    }
}

impl Drop for Run {
    /// A run nobody waits for any more must not leave the program running: give the thread
    /// a moment to kill it (the app may be about to exit).
    fn drop(&mut self) {
        self.stop();
        for _ in 0..40 {
            if !self.alive.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Run {
    /// Builds `text` and runs it. `input` is written to the program first; with `keep_stdin`
    /// the input stays open for [`Run::send_line`] (the console mode).
    pub fn start(
        text: String,
        input: String,
        keep_stdin: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stdin = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(false));
        let job = Job {
            text,
            input,
            keep_stdin,
            tx,
            stop: stop.clone(),
            stdin: stdin.clone(),
            alive: alive.clone(),
            wake: Arc::new(wake),
        };
        std::thread::spawn(move || job.go());
        Self {
            rx,
            stop,
            stdin,
            alive,
            started: Instant::now(),
        }
    }

    /// The next thing the run has produced, if there is one.
    pub fn try_recv(&self) -> Option<Msg> {
        self.rx.try_recv().ok()
    }

    /// Kills the program (or stops before it starts).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// One more line for the running program. False when it is not taking input.
    pub fn send_line(&self, line: &str) -> bool {
        let Ok(mut slot) = self.stdin.lock() else {
            return false;
        };
        let Some(w) = slot.as_mut() else {
            return false;
        };
        writeln!(w, "{line}").is_ok() && w.flush().is_ok()
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// The 1-based lines `Program.cs(12,5): error CS…` messages point to.
pub fn error_lines(build: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = build
        .lines()
        .filter(|l| l.contains(": error "))
        .filter_map(|l| {
            let rest = l.strip_prefix("Program.cs(")?;
            rest.split([',', ')']).next()?.parse().ok()
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

struct Job {
    text: String,
    input: String,
    keep_stdin: bool,
    tx: mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    alive: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

const NO_DOTNET: &str = "`dotnet` was not found on the PATH. Install the .NET SDK (dotnet.microsoft.com/download), then start the app again.";

fn dotnet() -> Command {
    let mut c = Command::new("dotnet");
    c.env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    c
}

impl Job {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        (self.wake)();
    }

    fn go(self) {
        let end = |j: &Job, s: String| j.send(Msg::Done(s));
        // the newest SDK decides the target framework
        let major = match dotnet().arg("--version").output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .trim()
                .split('.')
                .next()
                .and_then(|m| m.parse::<u32>().ok())
                .unwrap_or(8),
            Ok(o) => {
                let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
                return end(&self, format!("`dotnet --version` failed: {e}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return end(&self, NO_DOTNET.into());
            }
            Err(e) => return end(&self, format!("Can't start dotnet: {e}")),
        };
        if major < 6 {
            return end(
                &self,
                format!("The .NET SDK is too old (version {major}): .NET 6 or newer is needed"),
            );
        }
        let dir = std::env::temp_dir().join("speci-local-csharp");
        if let Err(e) = write_project(&dir, &self.text, major) {
            return end(
                &self,
                format!("Can't write the project in {}: {e}", dir.display()),
            );
        }

        // build, so that what runs (and what Stop kills) is the program itself
        let out = dir.join("out");
        let built = dotnet()
            .arg("build")
            .arg(dir.join("speci.csproj"))
            .args(["-c", "Debug", "-v", "q", "--nologo", "-o"])
            .arg(&out)
            .stdin(Stdio::null())
            .output();
        let built = match built {
            Ok(b) => b,
            Err(e) => return end(&self, format!("Can't start dotnet: {e}")),
        };
        if !built.status.success() {
            let text = String::from_utf8_lossy(&built.stdout);
            self.send(Msg::Build(clean_build(&text, &dir)));
            return end(&self, "The program has errors".into());
        }
        if self.stop.load(Ordering::Relaxed) {
            return end(&self, "Stopped".into());
        }

        let mut child = match dotnet()
            .arg(out.join("speci.dll"))
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return end(&self, format!("Can't start the program: {e}")),
        };
        self.alive.store(true, Ordering::Relaxed);
        if let Some(mut w) = child.stdin.take() {
            let mut text = self.input.clone();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            let _ = w.write_all(text.as_bytes());
            let _ = w.flush();
            if self.keep_stdin
                && let Ok(mut slot) = self.stdin.lock()
            {
                *slot = Some(w);
            }
        }
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|r| Box::new(r) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut r| {
            let (tx, wake) = (self.tx.clone(), self.wake.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let _ = tx.send(Msg::Out(String::from_utf8_lossy(&buf[..n]).into_owned()));
                    wake();
                }
            })
        })
        .collect();

        let started = Instant::now();
        let status = wait(&mut child, &self.stop);
        self.alive.store(false, Ordering::Relaxed);
        if let Ok(mut slot) = self.stdin.lock() {
            *slot = None;
        }
        for r in readers {
            let _ = r.join();
        }
        let secs = started.elapsed().as_secs_f32();
        end(
            &self,
            match status {
                None => "Stopped".into(),
                Some(s) if s.success() => format!("Finished in {secs:.1} s"),
                Some(s) => format!(
                    "The program ended with an error (exit code {}) after {secs:.1} s",
                    s.code().map_or("?".into(), |c| c.to_string())
                ),
            },
        );
    }
}

/// Waits for the program; `None` when it was stopped (and killed).
fn wait(child: &mut Child, stop: &AtomicBool) -> Option<ExitStatus> {
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

/// `Program.cs(3,9): error CS…` lines without the paths around them.
fn clean_build(text: &str, dir: &Path) -> String {
    let dir = dir.display().to_string();
    let mut out = String::new();
    let mut seen = std::collections::HashSet::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let l = l
            .replace(&format!("{dir}/"), "")
            .replace(&format!("{dir}\\"), "");
        let l = match l.rfind(" [") {
            Some(i) if l.ends_with(".csproj]") => l[..i].to_string(),
            _ => l,
        };
        if seen.insert(l.clone()) {
            out.push_str(&l);
            out.push('\n');
        }
    }
    out
}

fn write_project(dir: &Path, text: &str, major: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let proj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    <TargetFramework>net{major}.0</TargetFramework>\n    <AssemblyName>speci</AssemblyName>\n    <Nullable>disable</Nullable>\n    <ImplicitUsings>disable</ImplicitUsings>\n    <InvariantGlobalization>true</InvariantGlobalization>\n    <UseAppHost>false</UseAppHost>\n    <NuGetAudit>false</NuGetAudit>\n  </PropertyGroup>\n</Project>\n"
    );
    let write_if_changed = |name: &str, content: &str| -> std::io::Result<()> {
        let path: PathBuf = dir.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|old| old == content) {
            return Ok(());
        }
        std::fs::write(path, content)
    };
    write_if_changed("speci.csproj", &proj)?;
    write_if_changed("Program.cs", text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_dotnet() -> bool {
        Command::new("dotnet").arg("--version").output().is_ok()
    }

    const FOREVER: &str = "using System;\nclass Program { static void Main() { Console.WriteLine(\"up\"); while (true) { } } }\n";

    /// Stopping, or just dropping a run, kills the program itself.
    #[test]
    fn a_dropped_run_leaves_nothing_running() {
        if !have_dotnet() {
            return;
        }
        for explicit_stop in [true, false] {
            let run = Run::start(FOREVER.into(), String::new(), false, || {});
            // wait until the program says it is up
            let mut out = String::new();
            let start = Instant::now();
            while !out.contains("up") {
                assert!(
                    start.elapsed() < Duration::from_secs(120),
                    "never started: {out}"
                );
                while let Some(m) = run.try_recv() {
                    match m {
                        Msg::Out(s) => out.push_str(&s),
                        Msg::Build(s) => panic!("{s}"),
                        Msg::Done(s) => panic!("ended early: {s}"),
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(run.alive.load(Ordering::Relaxed));
            let alive = run.alive.clone();
            if explicit_stop {
                run.stop();
                // the end message comes
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
            assert!(!alive.load(Ordering::Relaxed), "the program still runs");
        }
    }
}
