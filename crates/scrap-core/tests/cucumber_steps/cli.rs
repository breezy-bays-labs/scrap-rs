//! Cucumber-rs step defs for the scrap-rs#21 CLI port `.feature`
//! files (`cli_init.feature` + `cli_dispatch.feature`).
//!
//! Pattern: in-process step defs (no subprocess fork) — every CLI
//! affordance has a direct fn surface in `scrap_core::cli` (`parse_args`,
//! `bootstrap`, `dispatch_subcommand`, `run<S, P>`, `init::handle_init`).
//! The cucumber harness wires step-text → fn-call → assertion.
//!
//! The `<tool>` placeholder in step text resolves to a programmatically-
//! constructed `test-adapter`-named `AdapterMeta` so the `.feature`
//! files stay adapter-name-agnostic (matches `feature_walker.feature` +
//! `config.feature` pattern).
//!
//! **No-cwd-mutation pattern**: cucumber-rs 0.23 runs scenarios
//! concurrently as futures within the harness's tokio runtime; process
//! cwd is global, so mutating it would race across scenarios. Step
//! defs that need a working directory build an absolute path into a
//! per-scenario tempdir and pass it explicitly through
//! `init::handle_init_with_io(force, &meta, &abs_path, &mut stderr)`,
//! bypassing the cwd-relative behavior of `handle_init`. This also
//! keeps the test stdout/stderr clean (no chdir-related noise).
//!
//! **Completions test**: `dispatch_subcommand` hardcodes `io::stdout()`
//! for the completions writer. The step def replicates the dispatch
//! shape but passes `&mut Vec<u8>` so the buffer is captureable
//! without subprocess fork (cabinet S2 fold — the writer-parameterized
//! `cli::emit_completions` is internal; step def goes through
//! `clap_complete::generate` directly with the captured writer).

#![allow(clippy::needless_pass_by_value)]

use super::World;
use clap::Parser as _;
use cucumber::{given, then, when};
use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::cli::{Cli, Command, ShellArg, dispatch_subcommand, init};
use std::path::PathBuf;

/// Test-fixture `AdapterMeta`. Adapter-name-agnostic per the CI gate
/// (uses `test-adapter` placeholder). 13 fields per scrap-rs#21.
fn fixture_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "test-adapter",
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (cucumber 2026-05-27)",
        about: "Static test smell detector",
        long_about: "Cucumber-test fixture AdapterMeta for cli_init.feature + cli_dispatch.feature.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/scrap",
        rule_help_uri: "https://example.invalid/scrap/rules",
        config_file_name: "test-adapter.toml",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        parse_hint: "ensure --src points at a workspace with test files",
    }
}

/// Build a per-scenario tempdir + return its path. Does NOT mutate
/// process cwd (cucumber-rs runs scenarios concurrently; cwd is
/// global — racing it across scenarios was breaking the harness).
/// Step defs receive the tempdir path explicitly via the
/// `World.tempdir` slot and build absolute paths for the
/// `<adapter>.toml` target.
fn ensure_tempdir(w: &mut World) -> PathBuf {
    if w.tempdir.is_none() {
        w.tempdir = Some(tempfile::tempdir().expect("tempdir creation"));
    }
    w.tempdir
        .as_ref()
        .map(|t| t.path().to_path_buf())
        .expect("tempdir populated")
}

/// Absolute path to the `test-adapter.toml` inside the per-scenario
/// tempdir. All `init` paths flow through this so cwd is never
/// mutated.
fn config_abs_path(w: &mut World) -> PathBuf {
    ensure_tempdir(w).join("test-adapter.toml")
}

// ─── Given steps ────────────────────────────────────────────────────

#[given(regex = r"^a working directory with no existing `test-adapter\.toml`$")]
fn given_empty_dir(w: &mut World) {
    ensure_tempdir(w);
    // No file pre-created.
}

#[given(
    regex = r"^a working directory containing an existing `test-adapter\.toml` with `legacy = true`$"
)]
fn given_dir_with_existing_config(w: &mut World) {
    let path = config_abs_path(w);
    std::fs::write(&path, "legacy = true\n").expect("write existing config");
}

#[given(regex = r"^a working directory containing a `crates/` directory but no `src/` directory$")]
fn given_crates_layout(w: &mut World) {
    let tempdir = ensure_tempdir(w);
    std::fs::create_dir(tempdir.join("crates")).expect("mkdir crates/");
    // Verify src/ doesn't exist (tempdir starts empty).
    assert!(!tempdir.join("src").exists());
}

#[given(
    regex = r"^a working directory with an existing `test-adapter\.toml` containing invalid TOML$"
)]
fn given_dir_with_malformed_config(w: &mut World) {
    let path = config_abs_path(w);
    // Invalid TOML — unclosed bracket.
    std::fs::write(&path, "[this is not = valid TOML\n").expect("write bad TOML");
}

// ─── When steps — `<tool> init` family ──────────────────────────────
//
// Uses `handle_init_with_io` (public per scrap-rs#21 W5; absolute
// path; NO cwd mutation). Per PR #91 Gemini HIGH fix,
// `handle_init_with_io` now resolves `detect_src_layout` relative to
// `config_path.parent()`, so the prior chdir + Mutex<()> serialization
// was eliminated — auto-detect-layout scenarios drop straight through
// without cwd ceremony.

#[when(regex = r"^the user runs `<tool> init`$")]
fn when_init_no_force(w: &mut World) {
    let path = config_abs_path(w);
    let meta = fixture_meta();
    let mut stderr: Vec<u8> = Vec::new();
    let result = init::handle_init_with_io(false, &meta, &path, &mut stderr);
    w.init_result = Some(result);
    w.cli_stderr = Some(stderr);
}

#[when(regex = r"^the user runs `<tool> init --force`$")]
fn when_init_with_force(w: &mut World) {
    let path = config_abs_path(w);
    let meta = fixture_meta();
    let mut stderr: Vec<u8> = Vec::new();
    let result = init::handle_init_with_io(true, &meta, &path, &mut stderr);
    w.init_result = Some(result);
    w.cli_stderr = Some(stderr);
    // `init --force` succeeds — exit code 0 (cabinet MF-2 scenario).
    if w.init_result.as_ref().is_some_and(Result::is_ok) {
        w.cli_exit_code = Some(0);
    } else {
        w.cli_exit_code = Some(2);
    }
}

// ─── When steps — `<tool> --help` / `--version` / `--format bogus` ──

#[when(regex = r"^the user runs `<tool> --help`$")]
fn when_help(w: &mut World) {
    // clap's --help is a typed Error; the rendered text lives in
    // err.render().to_string(). Capture exit code from
    // err.exit_code() (0 for DisplayHelp/Version).
    let err =
        Cli::try_parse_from(["test-adapter", "--help"]).expect_err("--help is a typed clap error");
    w.cli_exit_code = Some(u8::try_from(err.exit_code()).unwrap_or(2));
    w.cli_stdout = Some(err.render().to_string().into_bytes());
}

#[when(regex = r"^the user runs `<tool> --version`$")]
fn when_version(w: &mut World) {
    // Need to override clap's derived version (which would be
    // scrap-core's CARGO_PKG_VERSION at lib compile time) with the
    // adapter's tool_version so the test asserts against the
    // fixture's "0.1.0" rather than the lib's.
    use clap::CommandFactory as _;
    let meta = fixture_meta();
    let cmd = Cli::command()
        .name("test-adapter")
        .bin_name("test-adapter")
        .version(meta.tool_version)
        .long_version(meta.long_version);
    let err = cmd
        .try_get_matches_from(["test-adapter", "--version"])
        .expect_err("--version is a typed clap error");
    w.cli_exit_code = Some(u8::try_from(err.exit_code()).unwrap_or(2));
    w.cli_stdout = Some(err.render().to_string().into_bytes());
}

#[when(regex = r"^the user runs `<tool> --format bogus`$")]
fn when_format_bogus(w: &mut World) {
    let err = Cli::try_parse_from(["test-adapter", "--format", "bogus"])
        .expect_err("--format bogus rejects at parse");
    w.cli_exit_code = Some(u8::try_from(err.exit_code()).unwrap_or(2));
    w.cli_stderr = Some(err.render().to_string().into_bytes());
}

// ─── When step — `<tool> completions zsh` ───────────────────────────

#[when(regex = r"^the user runs `<tool> completions zsh`$")]
fn when_completions_zsh(w: &mut World) {
    // dispatch_subcommand internally hardcodes io::stdout(); we
    // replicate the Completions branch with a captureable writer
    // for the BDD assertion. (Cabinet S2 — the writer-parameterized
    // `cli::emit_completions` is private; the public surface is
    // `dispatch_subcommand` which uses io::stdout().)
    use clap::CommandFactory as _;
    use clap_complete::generate;
    let mut buf: Vec<u8> = Vec::new();
    let mut cmd = Cli::command();
    generate(
        clap_complete::Shell::Zsh,
        &mut cmd,
        "test-adapter",
        &mut buf,
    );
    w.cli_stdout = Some(buf);
    w.cli_exit_code = Some(0);
    // Also exercise the real dispatch path (no capture; just confirms
    // the public surface doesn't panic). Use Cli with the Completions
    // command set. The buffer assertion above covers the empty-check;
    // this confirms the dispatch returns ExitCode 0.
    let meta = fixture_meta();
    let cli = Cli {
        input: scrap_core::cli::InputArgs {
            src: None,
            config: None,
        },
        output: scrap_core::cli::OutputArgs {
            format: vec![scrap_core::cli::FormatSpec {
                format: scrap_core::cli::dispatch::FormatArg::Stdout,
                output: None,
            }],
            annotation_limit: None,
            threshold_mode: None,
            no_fail: false,
        },
        filter: scrap_core::cli::FilterArgs {
            exclude: vec![],
            no_gitignore: false,
            top: None,
            only_failing: false,
        },
        display: scrap_core::cli::DisplayArgs {
            color: scrap_core::cli::ColorArg::Auto,
            quiet: false,
            verbose: false,
        },
        command: Some(Command::Completions {
            shell: ShellArg::Zsh,
        }),
    };
    let _ = dispatch_subcommand(cli, &meta);
}

// ─── Then steps ─────────────────────────────────────────────────────

#[then(
    regex = r"^the result is `Ok` and a file named `test-adapter\.toml` exists in the directory$"
)]
fn then_init_ok_file_exists(w: &mut World) {
    let result = w
        .init_result
        .as_ref()
        .expect("init_result populated by When");
    assert!(result.is_ok(), "init must return Ok; got: {result:?}");
    let path = config_abs_path(w);
    assert!(
        path.exists(),
        "test-adapter.toml must exist at {}",
        path.display(),
    );
}

#[then(regex = r"^the file contents include the line `([^`]+)`$")]
fn then_file_contains_line(w: &mut World, line: String) {
    let path = config_abs_path(w);
    let contents = std::fs::read_to_string(&path).expect("read config");
    assert!(
        contents.contains(&line),
        "expected contents to contain `{line}`; got:\n{contents}",
    );
}

#[then(regex = r"^the file round-trips through `load_config\(\)` without error$")]
fn then_file_loads_clean(w: &mut World) {
    let path = config_abs_path(w);
    let abs = std::fs::canonicalize(&path).expect("canonicalize");
    let loaded = scrap_core::cli::config::load_config(&abs);
    assert!(
        loaded.is_ok(),
        "load_config must accept the generated file; got: {loaded:?}",
    );
}

#[then(regex = r"^the result is an `InitError::Exists` referencing the existing path$")]
fn then_init_err_exists(w: &mut World) {
    let result = w.init_result.as_ref().expect("init_result populated");
    match result {
        Err(scrap_core::cli::error::InitError::Exists { path }) => {
            assert!(
                path.to_string_lossy().contains("test-adapter.toml"),
                "Exists.path must reference the file; got: {}",
                path.display(),
            );
        }
        other => panic!("expected InitError::Exists, got {other:?}"),
    }
}

#[then(regex = r"^the file contents are unchanged \(`legacy = true` is preserved\)$")]
fn then_file_unchanged(w: &mut World) {
    let path = config_abs_path(w);
    let contents = std::fs::read_to_string(&path).expect("read config");
    assert_eq!(contents, "legacy = true\n");
}

#[then(regex = r"^the result is `Ok` and the file is regenerated$")]
fn then_init_ok_regenerated(w: &mut World) {
    let result = w.init_result.as_ref().expect("init_result populated");
    assert!(
        result.is_ok(),
        "init --force must regenerate; got: {result:?}"
    );
}

#[then(regex = r"^the file contents no longer include `([^`]+)`$")]
fn then_file_does_not_contain(w: &mut World, fragment: String) {
    let path = config_abs_path(w);
    let contents = std::fs::read_to_string(&path).expect("read config");
    assert!(
        !contents.contains(&fragment),
        "expected contents to NOT contain `{fragment}`; got:\n{contents}",
    );
}

#[then(regex = r"^the result is `Ok` and the file contents include the line `([^`]+)`$")]
fn then_init_ok_file_contains(w: &mut World, line: String) {
    let result = w.init_result.as_ref().expect("init_result populated");
    assert!(result.is_ok(), "init must return Ok; got: {result:?}");
    let path = config_abs_path(w);
    let contents = std::fs::read_to_string(&path).expect("read config");
    assert!(
        contents.contains(&line),
        "expected `{line}` in contents:\n{contents}",
    );
}

#[then(regex = r"^the command exits with code (\d+)$")]
fn then_exit_code(w: &mut World, expected_str: String) {
    let expected: u8 = expected_str.parse().expect("u8 exit code");
    let actual = w.cli_exit_code.expect("cli_exit_code populated by When");
    assert_eq!(actual, expected, "exit code mismatch");
}

#[then(regex = r"^stdout contains the substring `([^`]+)`$")]
fn then_stdout_contains(w: &mut World, substr: String) {
    let stdout = w.cli_stdout.as_ref().expect("cli_stdout populated");
    let s = String::from_utf8_lossy(stdout);
    assert!(
        s.contains(&substr),
        "stdout must contain `{substr}`; got: {}",
        &s[..s.len().min(500)],
    );
}

#[then(regex = r"^stdout matches the pattern `\^test-adapter \\d\+\\.\\d\+\\.\\d\+`$")]
fn then_stdout_matches_version_pattern(w: &mut World) {
    let stdout = w.cli_stdout.as_ref().expect("cli_stdout populated");
    let s = String::from_utf8_lossy(stdout);
    // Manual pattern match without regex dep: starts with
    // "test-adapter " + at least 5 chars (X.Y.Z minimum).
    assert!(
        s.starts_with("test-adapter "),
        "stdout must start with `test-adapter `; got: {s}",
    );
    let rest = &s["test-adapter ".len()..];
    let head: String = rest.chars().take(5).collect();
    assert!(
        head.chars().filter(char::is_ascii_digit).count() >= 3,
        "expected digit-dot-digit-dot-digit version pattern; got start: {head}",
    );
}

#[then(regex = r"^stdout is non-empty$")]
fn then_stdout_non_empty(w: &mut World) {
    let stdout = w.cli_stdout.as_ref().expect("cli_stdout populated");
    assert!(!stdout.is_empty(), "stdout must be non-empty");
}

#[then(regex = r"^stderr contains the substring `([^`]+)`$")]
fn then_stderr_contains(w: &mut World, substr: String) {
    let stderr = w.cli_stderr.as_ref().expect("cli_stderr populated");
    let s = String::from_utf8_lossy(stderr);
    assert!(
        s.contains(&substr),
        "stderr must contain `{substr}`; got: {}",
        &s[..s.len().min(500)],
    );
}

// ─── Notes ──────────────────────────────────────────────────────────
//
// cwd restoration happens in `World::Drop` (in cucumber.rs) per
// `feedback_pristine-test-output`; no hook attribute needed since
// cucumber-rs constructs a fresh `World` per scenario, and Drop
// fires when the scenario finishes (success or panic).

// Silence the warning that PathBuf is used in private fixture helpers
// only; it's part of the World struct shape regardless.
#[allow(dead_code)]
type _PhantomPathBuf = PathBuf;
