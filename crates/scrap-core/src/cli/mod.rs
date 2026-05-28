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
//! - [`error`] — `InitError` typed error surface for the `init`
//!   subcommand (lands with scrap-rs#21 W3).

pub mod config;
pub mod dispatch;
pub mod error;
pub mod init;

use std::process::ExitCode;

/// CLI entry point — bootstrap placeholder. Returns `ExitCode::SUCCESS`
/// and prints a single line. The real clap-derive surface (analyzer
/// pipeline + `ExitCode` shaping) lands with the CLI sub-issue.
#[must_use]
pub fn run() -> ExitCode {
    // Placeholder adapter-name literal; replaced by
    // `AdapterMeta::tool_name` threading in scrap-rs#21. The
    // source-only adapter-name-purity CI gate landing in scrap-rs#18
    // W7.1 grandfathers this single line via the per-line trailing
    // `tracked: scrap-rs#37` marker (in-line comment below) so the
    // gate ships enforceable on every NEW addition while #21
    // handles the structural replacement via AdapterMeta.
    println!("scrap4rs (skeleton) — see https://github.com/breezy-bays-labs/scrap-rs"); // tracked: scrap-rs#37 — AdapterMeta replacement
    ExitCode::SUCCESS
}
