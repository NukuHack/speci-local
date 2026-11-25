//! speci-local: an offline editor for ELTE-style specifications (with test cases) and
//! structograms.
//!
//! The crate has two layers. The engine (parser, evaluator, runner, file formats, the
//! structogram model and its runner) is plain Rust. The `gui` feature (on by default) adds the
//! egui app on top of it; the terminal UI in `tui/` builds on the engine alone.

#[cfg(feature = "gui")]
pub mod app;
mod compact;
pub mod csharp;
pub mod data;
pub mod diag;
pub mod dotnet;
#[cfg(feature = "gui")]
mod editor;
pub mod eval;
pub mod file;
pub mod ioplan;
pub mod parser;
#[cfg(feature = "gui")]
pub mod platform;
pub mod reduction;
mod runner;
pub mod stuki;
#[cfg(feature = "gui")]
mod ui;

pub use parser::parse;
pub use runner::{Outcome, run_and_trace, run_test, run_test_with, scaffold, trace_test};
