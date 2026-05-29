//! scrap4rs — Rust-source adapter binary for the scrap test-smell
//! detector.
//!
//! ~60-line entry point: constructs a 13-field `AdapterMeta` literal,
//! calls `cli::parse_args` to drive clap, then branches:
//!
//! - Subcommands (`init`, `completions`) dispatch via
//!   `cli::dispatch_subcommand` BEFORE `bootstrap` + `FsWalker::try_new`
//!   so `init --force` recovers from a malformed config and
//!   `completions zsh` never touches user config (cabinet MF-2 fix).
//! - The analysis path calls `cli::bootstrap` → constructs `FsWalker`
//!   with the merged `AnalysisConfig` → calls
//!   `cli::run<S, P>(cli, &source, &parser, &meta)` per the issue
//!   body's verbatim 4-parameter signature (FORK-11 Option A).
//!
//! `SCRAP4RS_LONG_VERSION` is stamped by `build.rs` at compile time
//! (git short hash + Hinnant civil-date `YYYY-MM-DD`) and drives the
//! `--version` long output via the `AdapterMeta` literal below.

use std::process::ExitCode;

use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::source::fs::FsWalker;
use scrap_core::cli::{self, dispatch};
use scrap_core::core::AnalyzeError;
use scrap4rs::parser::SynTestParser;

const ABOUT: &str = "Static test smell detector for Rust";
const LONG_ABOUT: &str = "Detects zero-assertion, tautological, no-op-IO, surface-only-IO, and \
                          large-example smells in Rust test files. Reads sources via syn; emits \
                          a nested JSON envelope, markdown report, SARIF, or plain stdout summary.";
const AFTER_HELP: &str = "\
EXAMPLES:
  scrap4rs --src crates/scrap-core --format json
  scrap4rs init
  scrap4rs --src src --format stdout --top 20 --only-failing
  scrap4rs --exclude \"tests/**\" --exclude \"benches/**\" --format json | jq

INVESTIGATION:
  # First-run scan: keep the report short
  scrap4rs --src . --top 20

  # CI-friendly: emit JSON for downstream tooling, never block on smells
  scrap4rs --format json --no-fail > scrap.json";

const PARSE_HINT: &str = "ensure --src points at a Cargo workspace with test files";

const EXTENSIONS: &[&str] = &["rs"];

const DEFAULT_EXCLUDES: &[&str] = &["tests/**", "benches/**", "examples/**"];

fn main() -> ExitCode {
    let meta = AdapterMeta {
        tool_name: env!("CARGO_PKG_NAME"),
        language: "rust",
        tool_version: env!("CARGO_PKG_VERSION"),
        long_version: env!("SCRAP4RS_LONG_VERSION"),
        about: ABOUT,
        long_about: LONG_ABOUT,
        after_help: AFTER_HELP,
        extensions: EXTENSIONS,
        tool_info_uri: "https://github.com/breezy-bays-labs/scrap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/scrap-rs#detection-rules",
        config_file_name: "scrap.toml",
        default_excludes: DEFAULT_EXCLUDES,
        parse_hint: PARSE_HINT,
    };
    let cli = cli::parse_args(&meta);
    // Cabinet MF-2: subcommand pre-dispatch — fires BEFORE any
    // config-file load or walker construction so `init --force`
    // recovers from a malformed config and `completions zsh` never
    // touches user config.
    if cli.command.is_some() {
        return cli::dispatch_subcommand(cli, &meta);
    }
    let bootstrap = match cli::bootstrap(&cli, &meta) {
        Ok(b) => b,
        Err(e) => {
            dispatch::render_error(&e, &meta);
            return ExitCode::from(2);
        }
    };
    let source = match FsWalker::try_new(bootstrap.analysis_config.clone()) {
        Ok(s) => s,
        Err(e) => {
            let wrapped = AnalyzeError::from(e);
            dispatch::render_error(&wrapped, &meta);
            return ExitCode::from(2);
        }
    };
    let parser = SynTestParser::new();
    cli::run(cli, &source, &parser, &meta)
}
