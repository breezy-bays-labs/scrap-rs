//! Command-line surface — clap-derive entry point + ExitCode shaping.
//!
//! The full clap surface (`--src`, `--config`, `--format`,
//! `--threshold-mode`, `--no-fail`, `--top`, `--only-failing`,
//! `--completions <SHELL>`, etc.) lands in P22 / P23. The bootstrap
//! ships a placeholder so the workspace builds and `cargo run -p
//! scrap4rs` produces non-empty output.

use std::process::ExitCode;

pub fn run() -> ExitCode {
    println!("scrap4rs (skeleton) — see https://github.com/breezy-bays-labs/scrap-rs");
    ExitCode::SUCCESS
}
