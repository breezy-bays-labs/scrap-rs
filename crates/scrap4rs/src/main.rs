//! W4 bridge `main.rs` — satisfies `cargo check -p scrap4rs` at the
//! W4 commit boundary per cabinet MF-3 B1. The production-quality
//! main.rs with `env!("SCRAP4RS_LONG_VERSION")` (build.rs-stamped) +
//! the EXAMPLES `after_help` block lands in W5 once build.rs ships.

use std::process::ExitCode;

use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::source::fs::FsWalker;
use scrap_core::cli::{self, dispatch};
use scrap_core::core::AnalyzeError;
use scrap4rs::parser::SynTestParser;

const ABOUT: &str = "Static test smell detector for Rust";
const LONG_ABOUT: &str = "Detects zero-assertion, tautological, no-op-IO, surface-only-IO, and large-example smells in Rust test files (W4 bridge — full copy lands W5).";
const AFTER_HELP: &str = "";
const PARSE_HINT: &str = "ensure --src points at a Cargo workspace with test files";
const EXTENSIONS: &[&str] = &["rs"];
const DEFAULT_EXCLUDES: &[&str] = &["tests/**", "benches/**", "examples/**"];

fn main() -> ExitCode {
    let meta = AdapterMeta {
        tool_name: env!("CARGO_PKG_NAME"),
        language: "rust",
        tool_version: env!("CARGO_PKG_VERSION"),
        // W4 bridge: long_version reuses tool_version because
        // build.rs (which stamps `SCRAP4RS_LONG_VERSION`) lands in
        // W5. The bridge keeps `cargo check -p scrap4rs` clean at
        // this commit boundary per cabinet MF-3 B1.
        long_version: env!("CARGO_PKG_VERSION"),
        about: ABOUT,
        long_about: LONG_ABOUT,
        after_help: AFTER_HELP,
        extensions: EXTENSIONS,
        tool_info_uri: "https://github.com/breezy-bays-labs/scrap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/scrap-rs#detection-rules",
        config_file_name: "scrap4rs.toml",
        default_excludes: DEFAULT_EXCLUDES,
        parse_hint: PARSE_HINT,
    };
    let cli = cli::parse_args(&meta);
    // MF-2: subcommand pre-dispatch — happens BEFORE any config-file
    // load or walker construction so `init --force` recovers from a
    // malformed config and `completions zsh` never touches user
    // config. `dispatch_subcommand` lives in cli/mod.rs (not
    // dispatch.rs) per advisor placement nit.
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
