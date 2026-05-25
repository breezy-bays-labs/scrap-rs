//! Command-line surface — clap-derive entry point + `ExitCode` shaping.
//!
//! The full clap surface (`--src`, `--config`, `--format`,
//! `--threshold-mode`, `--no-fail`, `--top`, `--only-failing`,
//! `--completions <SHELL>`, etc.) lands in P22 / P23. The bootstrap
//! ships a placeholder so the workspace builds and `cargo run -p
//! scrap4rs` produces non-empty output.
//!
//! Sub-module roster:
//! - [`config`] — project-level TOML config schema, loader, and
//!   canonical overrides resolver. Lands across W0–W7 of
//!   `scrap4rs/scrap-rs-20260524-config-schema` (scrap-rs#18; subsumes
//!   scrap-rs#34).

pub mod config;

use std::process::ExitCode;

/// CLI entry point — bootstrap placeholder. Returns `ExitCode::SUCCESS`
/// and prints a single line. The real clap-derive surface (analyzer
/// pipeline + `ExitCode` shaping) lands with the CLI sub-issue.
#[must_use]
pub fn run() -> ExitCode {
    // tracked: scrap-rs#37 — placeholder adapter-name literal; replaced
    // by `AdapterMeta::tool_name` threading in scrap-rs#21. The
    // source-only adapter-name-purity CI gate landing in
    // scrap-rs#18 W7.1 grandfathers this single line via the
    // `tracked: scrap-rs#37` comment marker so the gate ships
    // enforceable on every NEW addition while #21 handles the
    // structural replacement.
    println!("scrap4rs (skeleton) — see https://github.com/breezy-bays-labs/scrap-rs");
    ExitCode::SUCCESS
}
