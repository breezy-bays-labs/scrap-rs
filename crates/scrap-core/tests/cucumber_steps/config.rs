//! Config-loader step definitions for the cucumber harness.
//!
//! Pulled out of `tests/cucumber.rs` per scrap-rs#18 W5.1 (SHOULD-FIX #5
//! mod-block split — CAO Concern A). The entry point in `cucumber.rs`
//! includes this file via `#[path = "cucumber_steps/config.rs"] mod
//! config_steps;`; cucumber-rs registers the step fns into the same
//! global registry as the file-walker steps in the entry file.
//!
//! Scenarios reset World state via the `Background: Given a fresh test
//! World` step; per-scenario `tempdir` lifetimes prevent file-walker /
//! config field cross-contamination (per advisory #15 fold-in).
//!
//! All `file_name` parameters use the literal `"test-adapter.toml"` so
//! the adapter-name-purity gate (source-only in W7.1; expanded to tests
//! in scrap-rs#37) stays clean.
//!
//! ## Lint allowances
//!
//! Inherits the same pedantic-relax battery as `tests/cucumber.rs`
//! (workspace `[lints]` propagates to integration tests). Per
//! tracked: scrap-rs#50, the file-walker harness carries
//! `#[allow(clippy::needless_pass_by_value)]` +
//! `clippy::manual_let_else` + `clippy::needless_raw_string_hashes`;
//! the same decisions hold here. Cucumber step fns also take owned
//! `String` from regex captures, which triggers
//! `clippy::needless_pass_by_value`.
#![allow(clippy::needless_pass_by_value)]

use cucumber::{gherkin, given, then, when};
use scrap_core::cli::config::{ConfigError, FileConfig, discover_config, load_config};

use super::World;

// ─── Given — fixture authoring ──────────────────────────────────────

/// Materializes a config TOML file under a fresh tempdir at
/// `test-adapter.toml` (adapter-name-agnostic literal per W4 / #37).
/// Uses the docstring-style `"""..."""` block of the Gherkin scenario
/// as the fixture body.
///
/// **Cucumber-rs Gherkin docstring quirk**: the opening `"""` line is
/// preserved as a leading `\n` in the delivered string, which shifts
/// effective line numbering by +1 relative to the visible docstring
/// body. The `.feature` scenario's expected line numbers must account
/// for this (verified empirically during W5.1 implementation).
#[given(regex = r"^a config fixture with the contents:$")]
fn config_fixture_with_contents(w: &mut World, step: &gherkin::Step) {
    let docstring = step
        .docstring
        .as_ref()
        .expect("scenario must supply a docstring block");
    let dir = tempfile::tempdir().expect("tempdir creation");
    let path = dir.path().join("test-adapter.toml");
    std::fs::write(&path, docstring).expect("write fixture");
    w.tempdir = Some(dir);
    w.config_fixture_path = Some(path);
}

#[given(regex = r"^a tempdir containing `test-adapter\.toml`$")]
fn tempdir_with_config_only(w: &mut World) {
    let dir = tempfile::tempdir().expect("tempdir creation");
    let path = dir.path().join("test-adapter.toml");
    std::fs::write(&path, "").expect("write fixture");
    w.tempdir = Some(dir);
    w.config_fixture_path = Some(path);
}

#[given(regex = r"^a tempdir containing `test-adapter\.toml` at the root$")]
fn tempdir_with_config_at_root(w: &mut World) {
    tempdir_with_config_only(w);
}

#[given(regex = r"^a deep subdirectory `a/b/c` inside the tempdir$")]
fn deep_subdir_in_tempdir(w: &mut World) {
    let tmp = w.tempdir.as_ref().expect("tempdir must be set first");
    let deep = tmp.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).expect("create deep subdir");
}

#[given(regex = r"^an isolated tempdir containing no `test-adapter\.toml`$")]
fn isolated_tempdir_without_config(w: &mut World) {
    let dir = tempfile::tempdir().expect("tempdir creation");
    let deep = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).expect("create deep subdir");
    w.tempdir = Some(dir);
}

// ─── When — invocation ──────────────────────────────────────────────

#[when(regex = r"^the caller invokes `load_config\(\)` on the fixture$")]
fn invoke_load_config(w: &mut World) {
    let path = w
        .config_fixture_path
        .as_ref()
        .expect("config_fixture_path must be set by a Given step");
    w.config_load_result = Some(load_config(path));
}

#[when(regex = r"^the caller invokes `discover_config\(\)` starting from that tempdir$")]
fn invoke_discover_from_tempdir(w: &mut World) {
    let start = w
        .tempdir
        .as_ref()
        .expect("tempdir given")
        .path()
        .to_path_buf();
    w.discover_result = Some(discover_config(&start, "test-adapter.toml"));
}

#[when(regex = r"^the caller invokes `discover_config\(\)` starting from `a/b/c`$")]
fn invoke_discover_from_deep_subdir(w: &mut World) {
    let tmp = w.tempdir.as_ref().expect("tempdir given");
    let start = tmp.path().join("a").join("b").join("c");
    w.discover_result = Some(discover_config(&start, "test-adapter.toml"));
}

#[when(regex = r"^the caller invokes `discover_config\(\)` starting from a deep subdirectory$")]
fn invoke_discover_from_isolated_deep(w: &mut World) {
    let tmp = w.tempdir.as_ref().expect("tempdir given");
    let start = tmp.path().join("a").join("b").join("c");
    w.discover_result = Some(discover_config(&start, "test-adapter.toml"));
}

// ─── Then — load_config happy paths ─────────────────────────────────

#[then(regex = r"^the result is `Ok` and the loaded config equals the default$")]
fn assert_load_ok_default(w: &mut World) {
    let cfg = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect("Ok");
    assert_eq!(*cfg, FileConfig::default());
}

#[then(regex = r"^the result is `Ok` and the loaded config exercises every top-level field$")]
fn assert_load_ok_full_fixture(w: &mut World) {
    let cfg = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect("Ok");
    assert!(cfg.src.is_some(), "src expected non-None");
    assert!(!cfg.exclude.is_empty(), "exclude expected non-empty");
    assert!(cfg.extensions.is_some(), "extensions expected non-None");
    assert!(
        cfg.opt_outs.honor.is_some(),
        "opt_outs.honor expected non-None"
    );
    assert!(!cfg.detectors.is_empty(), "detectors expected non-empty");
    assert!(!cfg.overrides.is_empty(), "overrides expected non-empty");
}

// ─── Then — load_config error variants ──────────────────────────────

#[then(regex = r"^the result is a `Parse` error mentioning the unknown field$")]
fn assert_parse_error_unknown_field(w: &mut World) {
    let err = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect_err("Err");
    match err {
        ConfigError::Parse { source, .. } => {
            let msg = source.to_string();
            assert!(
                msg.contains("unknown") || msg.contains("unknown_key"),
                "expected unknown-field message, got: {msg}",
            );
        }
        other => panic!("expected ConfigError::Parse, got {other:?}"),
    }
}

#[then(regex = r"^the result is an `InvalidGlob` error on line (\d+) with pattern `(.+)`$")]
fn assert_invalid_glob_error_at_line(w: &mut World, expected_line: u32, expected_pattern: String) {
    let err = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect_err("Err");
    match err {
        ConfigError::InvalidGlob { line, pattern, .. } => {
            assert_eq!(
                *line, expected_line,
                "expected line {expected_line}, got {line}"
            );
            assert_eq!(
                *pattern, expected_pattern,
                "expected pattern `{expected_pattern}`, got `{pattern}`"
            );
        }
        other => panic!("expected ConfigError::InvalidGlob, got {other:?}"),
    }
}

#[then(regex = r"^the result is an `InvalidValue` error mentioning `(.+)`$")]
fn assert_invalid_value_error_mentioning(w: &mut World, needle: String) {
    let err = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect_err("Err");
    match err {
        ConfigError::InvalidValue { message, .. } => {
            assert!(
                message.contains(&needle),
                "expected `{needle}` in message, got: {message}",
            );
        }
        other => panic!("expected ConfigError::InvalidValue, got {other:?}"),
    }
}

// ─── Then — discover_config results ─────────────────────────────────

#[then(regex = r"^the result is `Ok` and the discovered path ends with `test-adapter\.toml`$")]
fn assert_discover_ok_ends_with_test_adapter(w: &mut World) {
    let found = w
        .discover_result
        .as_ref()
        .expect("discover_result")
        .as_ref()
        .expect("Ok");
    let path = found.as_ref().expect("Some(path)");
    assert!(
        path.ends_with("test-adapter.toml"),
        "expected discovered path to end with `test-adapter.toml`, got: {}",
        path.display(),
    );
}

#[then(regex = r"^the result is `Ok\(None\)`$")]
fn assert_discover_ok_none(w: &mut World) {
    let result = w
        .discover_result
        .as_ref()
        .expect("discover_result")
        .as_ref()
        .expect("Ok");
    assert!(result.is_none(), "expected Ok(None), got Ok({result:?})");
}

// ─── Then — OptOutPolicy Shape B contract ───────────────────────────

#[then(
    regex = r#"^the result is `Ok` and `opt_outs\.honor` equals exactly `\["no_asserts", "no_op"\]`$"#
)]
fn assert_opt_outs_honor_equals(w: &mut World) {
    use scrap_core::domain::opt_outs::OptOut;
    let cfg = w
        .config_load_result
        .as_ref()
        .expect("load_result")
        .as_ref()
        .expect("Ok");
    assert_eq!(
        cfg.opt_outs.honor,
        Some(vec![OptOut::NoAsserts, OptOut::NoOp]),
    );
}
