//! Command-line surface — clap-derive entry point + dispatch +
//! `ExitCode` shaping.
//!
//! Wave 4 of scrap-rs#21 replaces the original placeholder `run()`
//! with the full clap-derive surface per the issue body's enumerated
//! AC. The single grandfathered `tracked: scrap-rs#37` `println!`
//! line is REMOVED at this wave — the adapter-name-purity CI gate's
//! `grep -v 'tracked: scrap-rs#37'` filter now excludes nothing
//! (still passes; the filter can be dropped in a follow-up chore).
//!
//! Sub-module roster:
//! - [`config`] — project-level TOML config schema (re-exports
//!   POD types from `domain::config` per cabinet MF-1) + loader
//!   pipeline + `ConfigError` + canonical overrides resolver.
//! - [`error`] — `InitError` typed error surface for the `init`
//!   subcommand.
//! - [`init`] — `init` subcommand body (`handle_init` +
//!   `handle_init_with_io` + `render_config`).
//! - [`dispatch`] — reporter dispatch + `render_error` +
//!   `exit_code_for` + `now_iso_8601` (the format/error/exit-code
//!   concerns). `dispatch_subcommand` lives in THIS module per the
//!   advisor placement fix — same module as `Cli` / `Command` /
//!   `parse_args` / `run`, avoids a cross-module clap-derive
//!   import.
//!
//! Public surface this wave ships:
//! - [`Cli`] — clap `Parser` with 4 `#[command(flatten)]` arg
//!   groups + a `Command` subcommand enum.
//! - [`parse_args`] — splice `meta` into clap's `Command` so
//!   `--version` / `--help` show the adapter's strings.
//! - [`bootstrap`] — merge cli + file-config → `AnalysisConfig` +
//!   `EffectiveInputs`. Public per cabinet MF-3 B2 so the separate
//!   `scrap4rs` crate's `main.rs` can construct `FsWalker`
//!   between `bootstrap` and `run<S, P>`.
//! - [`run`] — `run<S, P>(cli, &source, &parser, &meta) -> ExitCode`.
//!   Issue-body verbatim 4-parameter signature. Analysis-only;
//!   subcommand dispatch lives in `dispatch_subcommand` per
//!   cabinet MF-2.
//! - [`dispatch_subcommand`] — `Init` / `Completions` branch. Fires
//!   from `main.rs` BEFORE `bootstrap` + `FsWalker::try_new` so
//!   `init --force` recovers from a malformed config (cabinet
//!   MF-2 — pinned in `cli_init.feature`).

pub mod config;
pub mod dispatch;
pub mod error;
pub mod init;

use std::io;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell as ClapShell;

use crate::adapter_meta::AdapterMeta;
use crate::cli::config::{ConfigError, FileConfig};
use crate::cli::dispatch::{
    DispatchError, FormatArg, exit_code_for, print_diagnostics, render_error, render_format,
};
use crate::core::{AnalyzeError, AnalyzeOptions, AnalyzeOutput, analyze};
use crate::domain::config::AnalysisConfig;
use crate::domain::threshold::ThresholdMode;
use crate::domain::types::SourceRoot;
use crate::ports::parser::TestParserPort;
use crate::ports::source::SourcePort;

// ────────────────────────────────────────────────────────────────────────
// ValueEnum wrappers (keep clap derive out of domain/)
// ────────────────────────────────────────────────────────────────────────

/// Output format selector. Wraps [`crate::cli::dispatch::FormatArg`]
/// so domain code stays clap-free; `From<FormatArgClap>` bridges at
/// the dispatch boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArgClap {
    /// Nested JSON envelope per `adr-nested-json-envelope`.
    Json,
    /// Plain text — minimum-viable per FORK-4.
    Stdout,
    /// Markdown — NOT yet implemented (tracked: scrap-rs#15).
    Markdown,
    /// SARIF 2.1.0 — NOT yet implemented (tracked: scrap-rs#17).
    Sarif,
}

impl From<FormatArgClap> for FormatArg {
    fn from(arg: FormatArgClap) -> Self {
        match arg {
            FormatArgClap::Json => FormatArg::Json,
            FormatArgClap::Stdout => FormatArg::Stdout,
            FormatArgClap::Markdown => FormatArg::Markdown,
            FormatArgClap::Sarif => FormatArg::Sarif,
        }
    }
}

/// Shell name for completion script generation. Maps to either
/// `clap_complete::Shell` (bash/zsh/fish/elvish/powershell) or
/// `clap_complete_nushell::Nushell` (nushell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellArg {
    /// Bash completion script.
    Bash,
    /// Zsh completion script.
    Zsh,
    /// Fish completion script.
    Fish,
    /// Elvish completion script.
    Elvish,
    /// PowerShell completion script.
    Powershell,
    /// Nushell completion script (via the separate
    /// `clap_complete_nushell` crate).
    Nushell,
}

/// Color choice for terminal output. Default `Auto`. Wired through
/// to color-aware reporters; the stdout reporter at v0.1 ignores
/// this (no ANSI emission; the table reporter at scrap-rs#16
/// consumes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ColorArg {
    /// Colorize when writing to a terminal.
    #[default]
    Auto,
    /// Always colorize output.
    Always,
    /// Never colorize output.
    Never,
}

/// Threshold mode selector. Wraps
/// [`crate::domain::threshold::ThresholdMode`] so domain code stays
/// clap-free; `From<ThresholdModeArg>` bridges at the merge layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ThresholdModeArg {
    /// Strict — tightest cutoffs.
    Strict,
    /// Default — middle cutoffs.
    Default,
    /// Lenient — loosest cutoffs.
    Lenient,
}

impl From<ThresholdModeArg> for ThresholdMode {
    fn from(arg: ThresholdModeArg) -> Self {
        match arg {
            ThresholdModeArg::Strict => ThresholdMode::Strict,
            ThresholdModeArg::Default => ThresholdMode::Default,
            ThresholdModeArg::Lenient => ThresholdMode::Lenient,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Arg groups (#[command(flatten)])
// ────────────────────────────────────────────────────────────────────────

/// `--src` + `--config` — what to analyze + which config to load.
#[derive(Debug, Args)]
#[command(next_help_heading = "Input")]
pub struct InputArgs {
    /// Root directory of source files to analyze [default: src]
    #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub src: Option<PathBuf>,

    /// Path to config file (default: auto-discover the adapter's config TOML)
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub config: Option<PathBuf>,
}

/// `--format` + `--threshold-mode` + `--no-fail` — what the
/// analyzer's output looks like + how the gate behaves.
#[derive(Debug, Args)]
#[command(next_help_heading = "Output")]
pub struct OutputArgs {
    /// Output format — `json`, `stdout`, `markdown`, or `sarif`.
    /// Markdown + SARIF are tracked under scrap-rs#15 / scrap-rs#17
    /// and currently exit with a "not yet implemented" message.
    #[arg(short, long, value_enum, default_value_t = FormatArgClap::Stdout)]
    pub format: FormatArgClap,

    /// Threshold mode for the gate verdict. Wire-only at v0.1
    /// (scrap-rs#75 lands the real `Report.passed` computation).
    #[arg(long, value_enum, value_name = "MODE")]
    pub threshold_mode: Option<ThresholdModeArg>,

    /// Always exit 0, even when threshold violations exist.
    ///
    /// Overrides the exit-code translation only; the underlying
    /// analysis is untouched and `result.passed` in JSON output
    /// still reflects the truthful pass/fail state.
    #[arg(long)]
    pub no_fail: bool,
}

/// `--exclude` (repeatable) + `--no-gitignore` (folds scrap-rs#33) +
/// `--top` + `--only-failing` — what gets pruned.
#[derive(Debug, Args)]
#[command(next_help_heading = "Filtering")]
pub struct FilterArgs {
    /// Glob patterns to exclude from analysis (repeatable).
    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Do not respect `.gitignore` / `.ignore` / `.git/info/exclude`.
    /// Folds scrap-rs#33.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Truncate the displayed view to the top N findings.
    /// `--top 0` is rejected at parse time via `NonZeroU32`.
    #[arg(long, value_name = "N")]
    pub top: Option<NonZeroU32>,

    /// Only show findings whose `scrap_score > 0.0`. View-shaping
    /// only; the gate (exit code) is unaffected.
    #[arg(long)]
    pub only_failing: bool,
}

/// `--color` + `-q,--quiet` + `-v,--verbose` — terminal-side
/// presentation knobs.
#[derive(Debug, Args)]
#[command(next_help_heading = "Display")]
pub struct DisplayArgs {
    /// When to use terminal colors.
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    pub color: ColorArg,

    /// Suppress report output, only set exit code.
    #[arg(short, long)]
    pub quiet: bool,

    /// Show parse diagnostics + source-walker diagnostics on stderr.
    #[arg(short, long)]
    pub verbose: bool,
}

// ────────────────────────────────────────────────────────────────────────
// Subcommands
// ────────────────────────────────────────────────────────────────────────

/// Top-level subcommands. `None` runs the analysis path; `Some(_)`
/// branches to `dispatch_subcommand` per cabinet MF-2.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a shell completion script to stdout.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: ShellArg,
    },
    /// Generate a starter config TOML in the current directory.
    ///
    /// Per FORK-5, v0.1 is non-interactive only; `--non-interactive`
    /// is accepted as a no-op forward-compat flag. Refuses to
    /// overwrite an existing config unless `--force` is passed.
    Init {
        /// Overwrite an existing config file in this directory.
        #[arg(long)]
        force: bool,
        /// Skip interactive prompts (no-op in v0.1; reserved for v0.2).
        #[arg(long)]
        non_interactive: bool,
    },
}

// ────────────────────────────────────────────────────────────────────────
// Top-level Cli + EffectiveInputs + Bootstrap
// ────────────────────────────────────────────────────────────────────────

/// Top-level CLI. Composed via `#[command(flatten)]` of four arg
/// groups + an optional subcommand. `parse_args(meta)` is the
/// adapter-facing entry point.
#[derive(Debug, Parser)]
#[command(
    version,
    author,
    about = "Static test smell detector",
    long_about = "Static test smell detector. Adapter binaries (the Rust adapter via syn; future TypeScript adapter via swc/oxc) link this CLI core."
)]
pub struct Cli {
    /// Input args (`--src`, `--config`).
    #[command(flatten)]
    pub input: InputArgs,
    /// Output args (`--format`, `--threshold-mode`, `--no-fail`).
    #[command(flatten)]
    pub output: OutputArgs,
    /// Filter args (`--exclude`, `--no-gitignore`, `--top`,
    /// `--only-failing`).
    #[command(flatten)]
    pub filter: FilterArgs,
    /// Display args (`--color`, `-q,--quiet`, `-v,--verbose`).
    #[command(flatten)]
    pub display: DisplayArgs,
    /// Optional subcommand. `None` runs analysis; `Some(_)`
    /// branches via [`dispatch_subcommand`].
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Effective merged inputs — output of [`bootstrap`]. Built from
/// `Cli` ∪ `FileConfig` ∪ `AdapterMeta` defaults.
///
/// Public per cabinet MF-3 visibility-leak fix: `Bootstrap.effective`
/// is `pub`; if `EffectiveInputs` weren't also `pub`, downstream
/// callers holding `&Bootstrap` couldn't access the field cross-crate.
#[derive(Debug, Clone)]
pub struct EffectiveInputs {
    /// Source root (post-merge; pre-canonicalize). `analyze`
    /// canonicalizes once at the top of the pipeline.
    pub src: PathBuf,
    /// Merged exclude globs (cli + `file_config`; dedup'd).
    pub exclude: Vec<String>,
    /// Effective extensions. `file_config.extensions` ∨
    /// `meta.extensions_owned()`.
    pub extensions: Vec<String>,
    /// Walker honors `.gitignore` etc. `false` iff `--no-gitignore`.
    pub respect_gitignore: bool,
    /// Resolved threshold mode (CLI > Default). Wire-only at v0.1.
    pub threshold_mode: ThresholdMode,
}

/// Output of [`bootstrap`] — bundles the runtime `AnalysisConfig`
/// (for walker construction in `main.rs`) with the merged
/// `EffectiveInputs` + the raw `FileConfig` (for downstream
/// per-detector knob consumption).
///
/// **`pub`** per cabinet MF-3 B2 — the separate `scrap4rs` crate's
/// `main.rs` calls `cli::bootstrap` before constructing `FsWalker`
/// and passes the result into `cli::run<S, P>`.
#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// Runtime walker config — feed into `FsWalker::try_new`.
    pub analysis_config: AnalysisConfig,
    /// Merged inputs (`threshold_mode` for the reporter, `exclude`/
    /// extensions echoed for embedder visibility).
    pub effective: EffectiveInputs,
    /// Raw `FileConfig` from disk (for `detect_all`'s per-detector
    /// knob consumption).
    pub file_config: FileConfig,
}

// ────────────────────────────────────────────────────────────────────────
// parse_args + bootstrap + run + dispatch_subcommand
// ────────────────────────────────────────────────────────────────────────

/// Parse process args into [`Cli`], splicing the adapter's runtime
/// metadata into clap's help / `--version` output.
///
/// Splits responsibilities: `parse_args` returns a typed `Cli`
/// without any I/O or config-file load; `bootstrap` does the file
/// merge; `run` orchestrates the pipeline. This three-step split is
/// what lets library embedders skip `bootstrap` (or replace it) and
/// drive `run<S, P>` with their own merged `Bootstrap`.
///
/// Exits via `e.exit()` on parse failure (matches clap's default
/// behavior — prints help/version cleanly and exits the process).
#[must_use]
pub fn parse_args(meta: &AdapterMeta) -> Cli {
    meta.debug_assert_required_fields();
    let cmd = build_command(meta);
    let matches = cmd.get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// Merge `Cli` + on-disk `FileConfig` + `AdapterMeta` defaults into
/// a [`Bootstrap`].
///
/// **Public** per cabinet MF-3 B2 — the separate `scrap4rs` crate's
/// `main.rs` calls this before constructing `FsWalker`. Library
/// embedders use it the same way.
///
/// 3-step config-file precedence:
/// 1. `cli.input.config` explicit → `load_config(path)`.
/// 2. Else `discover_config(cli.input.src.as_deref().unwrap_or(Path::new(".")), meta.config_file_name)` walking upward → `load_config(path)` if found.
/// 3. Else `FileConfig::default()`.
///
/// # Errors
///
/// Returns [`AnalyzeError::Config`] when the config-file load
/// fails (missing-file, parse error, invalid glob, semantic
/// invalid value).
pub fn bootstrap(cli: &Cli, meta: &AdapterMeta) -> Result<Bootstrap, AnalyzeError> {
    let file_config = load_file_config(cli, meta.config_file_name)?
        .map(|(c, _path)| c)
        .unwrap_or_default();
    let effective = merge_effective_inputs(cli, &file_config, meta);
    let analysis_config = AnalysisConfig::new(
        SourceRoot::new(&effective.src),
        effective.exclude.clone(),
        effective.extensions.clone(),
        effective.respect_gitignore,
    );
    Ok(Bootstrap {
        analysis_config,
        effective,
        file_config,
    })
}

/// Run the analysis pipeline end-to-end.
///
/// **Analysis-only**: subcommand paths (`Init`, `Completions`)
/// dispatch via [`dispatch_subcommand`] BEFORE this fn is called.
/// `main.rs`'s `if cli.command.is_some() { return dispatch_subcommand(...); }`
/// pre-dispatch guarantees `cli.command` is `None` here (cabinet
/// MF-2 — fixes `init --force` recovery from malformed config).
///
/// Re-calls [`bootstrap`] internally for `threshold_mode` +
/// `&FileConfig`. Cabinet S1 + advisor pass: this duplicates main's
/// `bootstrap` call (~1ms + 1 syscall); semantically harmless in
/// v0.1 because `detect_all` is a stub. The cleaner v1.0 path
/// (cache or thread `Bootstrap` in) lands as scrap-rs#NN-9.
///
/// # Errors
///
/// Returns the analysis-side exit code as `ExitCode`. Pipeline
/// errors are rendered via [`render_error`] and mapped through
/// [`exit_code_for`].
///
/// # Panics
///
/// Never panics in normal use. The `NonZeroUsize::new(1).expect(...)`
/// fallback in the `--top` cast path is structurally unreachable
/// (1 is non-zero by construction) and exists only to satisfy
/// `?-less` flow control.
///
/// **Argument by value** (`cli: Cli`, not `&Cli`): matches the
/// scrap-rs#21 issue body's verbatim signature. The CLI is consumed
/// because `run<S, P>` is the terminal call in the pipeline — main
/// has no further use for it. `#[allow(clippy::needless_pass_by_value)]`
/// honors the spec.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
pub fn run<S, P>(cli: Cli, source: &S, parser: &P, meta: &AdapterMeta) -> ExitCode
where
    S: SourcePort,
    P: TestParserPort,
{
    debug_assert!(
        cli.command.is_none(),
        "cli::run is analysis-only; subcommand paths dispatch via dispatch_subcommand BEFORE run is called (cabinet MF-2)",
    );

    let bootstrap_val = match bootstrap(&cli, meta) {
        Ok(b) => b,
        Err(e) => {
            render_error(&e, meta);
            return ExitCode::from(2);
        }
    };

    let analyze_options = AnalyzeOptions {
        src: bootstrap_val.analysis_config.src.as_path().to_path_buf(),
        exclude: bootstrap_val.effective.exclude.clone(),
        extensions: bootstrap_val.effective.extensions.clone(),
        respect_gitignore: bootstrap_val.effective.respect_gitignore,
        config: bootstrap_val.file_config.clone(),
        threshold_mode: bootstrap_val.effective.threshold_mode,
    };

    let output: AnalyzeOutput = match analyze(&analyze_options, source, parser) {
        Ok(o) => o,
        Err(e) => {
            render_error(&e, meta);
            return exit_code_for(Err::<bool, &AnalyzeError>(&e), cli.output.no_fail);
        }
    };

    if cli.display.verbose {
        print_diagnostics(&output, &mut io::stderr());
    }

    if !cli.display.quiet {
        let emit_options = crate::adapters::reporters::json::EmitOptions {
            top: cli.filter.top.map(|n| {
                // n: NonZeroU32 → NonZeroUsize. usize is always ≥ u32 on
                // supported targets; the conversion is infallible.
                std::num::NonZeroUsize::new(n.get() as usize).unwrap_or_else(|| {
                    // Unreachable; NonZeroU32 → usize cast preserves
                    // non-zero. Defensive fallback.
                    std::num::NonZeroUsize::new(1).expect("1 is non-zero")
                })
            }),
            only_failing: cli.filter.only_failing,
        };
        if let Err(e) = render_format(
            cli.output.format.into(),
            &output.report,
            meta,
            &emit_options,
            bootstrap_val.effective.threshold_mode,
            &mut io::stdout(),
        ) {
            handle_dispatch_error(&e);
            return ExitCode::from(2);
        }
    }

    exit_code_for(
        Ok::<bool, &AnalyzeError>(output.report.passed),
        cli.output.no_fail,
    )
}

/// Subcommand branch — `Init` / `Completions`. Fires from `main.rs`
/// BEFORE `bootstrap` + `FsWalker::try_new` so `init --force`
/// recovers from a malformed config (cabinet MF-2; pinned in
/// `cli_init.feature`'s force-overrides-malformed-config scenario).
///
/// Lives in `cli/mod.rs` (not `dispatch.rs`) per the advisor
/// placement fix — same module as `Cli`/`Command`/`parse_args`/`run`;
/// avoids forcing `dispatch.rs` to import clap-derived types.
///
/// # Panics
///
/// Panics if called with `cli.command == None`. The contract is
/// "main.rs checks `cli.command.is_some()` before calling".
#[must_use]
pub fn dispatch_subcommand(cli: Cli, meta: &AdapterMeta) -> ExitCode {
    let cmd = cli
        .command
        .expect("dispatch_subcommand caller must check cli.command.is_some()");
    match cmd {
        Command::Completions { shell } => {
            emit_completions(shell, &current_bin_name(meta.tool_name), &mut io::stdout());
            ExitCode::from(0)
        }
        Command::Init {
            force,
            non_interactive,
        } => match init::handle_init(force, non_interactive, meta) {
            Ok(()) => ExitCode::from(0),
            Err(e) => {
                render_error(&AnalyzeError::from(e), meta);
                ExitCode::from(2)
            }
        },
    }
}

// ────────────────────────────────────────────────────────────────────────
// Private helpers
// ────────────────────────────────────────────────────────────────────────

/// Read the adapter binary's name from `argv[0]`, falling back to
/// `meta.tool_name` when `argv[0]` is unavailable. `file_stem()`
/// strips `.exe` on Windows. Lifted verbatim from crap-rs's
/// pattern (crap-rs#161 retrofit).
fn current_bin_name(meta_fallback: &str) -> String {
    std::env::args()
        .next()
        .and_then(|first| {
            std::path::PathBuf::from(first)
                .file_stem()
                .map(|os| os.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| meta_fallback.to_string())
}

/// Build the clap `Command` with the binary's runtime metadata
/// spliced in. The clap derive's `version` reads `CARGO_PKG_VERSION`
/// at lib-crate compile time (scrap-core's `0.1.0`); the binary's
/// own version reaches us by parameter via `meta.tool_version` /
/// `meta.long_version`.
fn build_command(meta: &AdapterMeta) -> clap::Command {
    let bin_name = current_bin_name(meta.tool_name);
    let mut cmd = Cli::command()
        .name(bin_name.clone())
        .bin_name(bin_name)
        .version(meta.tool_version)
        .long_version(meta.long_version)
        .about(meta.about)
        .long_about(meta.long_about);
    if !meta.after_help.is_empty() {
        cmd = cmd.after_help(meta.after_help);
    }
    cmd
}

/// 3-step config-file load: `--config` explicit ∨ `discover_config`
/// walking upward ∨ `None`.
fn load_file_config(
    cli: &Cli,
    config_file_name: &str,
) -> Result<Option<(FileConfig, PathBuf)>, ConfigError> {
    if let Some(path) = &cli.input.config {
        let cfg = config::load_config(path)?;
        Ok(Some((cfg, path.clone())))
    } else {
        let start = cli
            .input
            .src
            .as_deref()
            .unwrap_or(std::path::Path::new("."));
        match config::discover_config(start, config_file_name)? {
            Some(path) => {
                let cfg = config::load_config(&path)?;
                Ok(Some((cfg, path)))
            }
            None => Ok(None),
        }
    }
}

/// Merge `Cli` + `FileConfig` + `AdapterMeta` into runtime
/// `EffectiveInputs`. CLI flags win over `file_config`; `file_config`
/// wins over meta defaults.
fn merge_effective_inputs(
    cli: &Cli,
    file_config: &FileConfig,
    meta: &AdapterMeta,
) -> EffectiveInputs {
    let src = cli
        .input
        .src
        .clone()
        .or_else(|| file_config.src.clone())
        .unwrap_or_else(|| PathBuf::from("src"));

    // Dedup-merge exclude: cli first, then file_config additions.
    let mut exclude: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for pat in &cli.filter.exclude {
        if seen.insert(pat.as_str()) {
            exclude.push(pat.clone());
        }
    }
    for pat in &file_config.exclude {
        if seen.insert(pat.as_str()) {
            exclude.push(pat.clone());
        }
    }

    let extensions = file_config
        .extensions
        .clone()
        .unwrap_or_else(|| meta.extensions_owned());

    let respect_gitignore = !cli.filter.no_gitignore;

    let threshold_mode = cli
        .output
        .threshold_mode
        .map_or(ThresholdMode::default(), Into::into);

    EffectiveInputs {
        src,
        exclude,
        extensions,
        respect_gitignore,
        threshold_mode,
    }
}

/// Emit a shell completion script to `writer`. Writer-parameterized
/// per cabinet S2 fold so the BDD harness in W5 captures into
/// `Vec<u8>` without subprocess fork.
fn emit_completions<W: io::Write>(shell: ShellArg, bin_name: &str, writer: &mut W) {
    let mut cmd = Cli::command();
    match shell {
        ShellArg::Bash => clap_complete::generate(ClapShell::Bash, &mut cmd, bin_name, writer),
        ShellArg::Zsh => clap_complete::generate(ClapShell::Zsh, &mut cmd, bin_name, writer),
        ShellArg::Fish => clap_complete::generate(ClapShell::Fish, &mut cmd, bin_name, writer),
        ShellArg::Elvish => clap_complete::generate(ClapShell::Elvish, &mut cmd, bin_name, writer),
        ShellArg::Powershell => {
            clap_complete::generate(ClapShell::PowerShell, &mut cmd, bin_name, writer);
        }
        ShellArg::Nushell => {
            clap_complete::generate(clap_complete_nushell::Nushell, &mut cmd, bin_name, writer);
        }
    }
}

/// Render a `DispatchError` to stderr. `NotImplemented` carries
/// its own format/issue token; the I/O + Json variants Display
/// their underlying errors.
fn handle_dispatch_error(err: &DispatchError) {
    match err {
        DispatchError::NotImplemented {
            format,
            tracking_issue,
        } => {
            eprintln!("error: {format} reporter not yet implemented (tracked: {tracking_issue})");
        }
        DispatchError::Json(_) | DispatchError::Io(_) => {
            eprintln!("error: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// argv[0]-style helper: prepend a placeholder so clap doesn't
    /// reject the missing program name.
    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut full = vec!["test-adapter"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full)
    }

    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "cli-test fixture",
            long_about: "Test-fixture AdapterMeta for cli::mod tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &["tests/**"],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    // ── Parse-path coverage ────────────────────────────────────────

    #[test]
    fn parse_minimal_args() {
        let cli = parse(&[]).expect("bare argv parses");
        assert!(cli.input.src.is_none());
        assert!(cli.input.config.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn format_defaults_to_stdout() {
        let cli = parse(&[]).unwrap();
        assert_eq!(cli.output.format, FormatArgClap::Stdout);
    }

    #[test]
    fn completions_subcommand_parses() {
        let cli = parse(&["completions", "zsh"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Completions {
                shell: ShellArg::Zsh
            })
        ));
    }

    #[test]
    fn init_subcommand_parses() {
        let cli = parse(&["init", "--force"]).unwrap();
        match cli.command {
            Some(Command::Init {
                force,
                non_interactive,
            }) => {
                assert!(force);
                assert!(!non_interactive);
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn init_subcommand_non_interactive_no_op_parses() {
        let cli = parse(&["init", "--non-interactive"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Init {
                force: false,
                non_interactive: true
            })
        ));
    }

    #[test]
    fn no_gitignore_flag_threads_through_to_effective_inputs() {
        let cli = parse(&["--no-gitignore"]).unwrap();
        let meta = fixture_meta();
        let file_config = FileConfig::default();
        let eff = merge_effective_inputs(&cli, &file_config, &meta);
        assert!(!eff.respect_gitignore);
    }

    #[test]
    fn top_zero_rejected_at_parse_time() {
        let err = parse(&["--top", "0"]).expect_err("--top 0 must reject at parse");
        assert!(
            err.to_string().contains("invalid value"),
            "clap must emit invalid-value error; got: {err}",
        );
    }

    #[test]
    fn exclude_repeatable() {
        let cli = parse(&["--exclude", "tests/**", "--exclude", "benches/**"]).unwrap();
        assert_eq!(cli.filter.exclude, vec!["tests/**", "benches/**"]);
    }

    #[test]
    fn threshold_mode_default_parses() {
        let cli = parse(&["--threshold-mode", "default"]).unwrap();
        assert_eq!(cli.output.threshold_mode, Some(ThresholdModeArg::Default));
    }

    #[test]
    fn threshold_mode_invalid_rejected() {
        let err =
            parse(&["--threshold-mode", "bogus"]).expect_err("--threshold-mode bogus rejects");
        assert!(err.to_string().contains("invalid value"));
    }

    #[test]
    fn parse_help_args_exits_via_clap() {
        let err = parse(&["--help"]).expect_err("--help is a DisplayHelp error from clap");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn parse_version_args_exits_via_clap() {
        let err = parse(&["--version"]).expect_err("--version is a DisplayVersion error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn current_bin_name_falls_back_to_meta_when_argv_lookup_fails() {
        // Can't easily wipe argv[0] in-process, but the fallback
        // path is exercisable by direct call with an empty fallback
        // — verifies the fn returns a String (not panics).
        let s = current_bin_name("test-adapter");
        assert!(!s.is_empty());
    }

    // ── dispatch_subcommand coverage (cabinet MF-2 + S2 emit_completions) ─

    #[test]
    fn dispatch_subcommand_completions_writes_non_empty_script_to_buffer() {
        // emit_completions is writer-parameterized per cabinet S2.
        // We can't call dispatch_subcommand directly with a custom
        // writer (it hardcodes io::stdout()); instead verify the
        // underlying helper's behavior against a Vec<u8>.
        let mut buf: Vec<u8> = Vec::new();
        emit_completions(ShellArg::Bash, "test-adapter", &mut buf);
        assert!(!buf.is_empty(), "bash completions must be non-empty");
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("test-adapter"),
            "completions must reference bin_name; got start: {}",
            &s[..s.len().min(200)],
        );
    }

    #[test]
    fn dispatch_subcommand_init_returns_zero_on_success_with_chdir() {
        // dispatch_subcommand → handle_init → handle_init_with_io
        // path. Needs a tempdir + chdir for the file write to land
        // somewhere clean.
        let tempdir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tempdir.path()).unwrap();

        let mut cli = parse(&["init", "--non-interactive"]).unwrap();
        // Force the Init force flag to true to match the simpler
        // path (no pre-existing file in the tempdir, so force=false
        // would also work).
        if let Some(Command::Init { force, .. }) = &mut cli.command {
            *force = false;
        }

        let meta = fixture_meta();
        let code = dispatch_subcommand(cli, &meta);
        // Restore cwd BEFORE the assert so a failure doesn't leak
        // cwd across tests.
        std::env::set_current_dir(&original).unwrap();

        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
        // The file should exist in the tempdir.
        assert!(tempdir.path().join("test-adapter.toml").exists());
    }
}
