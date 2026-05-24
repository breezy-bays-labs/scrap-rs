//! Per-fixture insta snapshot tests pinning the parser's
//! `ParsedTestFile` output shape.
//!
//! Each test:
//! 1. Reads a fixture file from `crates/scrap4rs/tests/fixtures/...`.
//! 2. Parses it via `SynTestParser`.
//! 3. Snapshots the projected `ParsedTestFile` via
//!    `insta::assert_yaml_snapshot!` so diffs surface field-by-field.
//!
//! Snapshot discipline (per impl-plan Reusable Reference):
//! - S2.1 (this session) is the only one allowed to seed snapshots
//!   with `INSTA_UPDATE=auto`. Output `.snap` files become the
//!   contract — review carefully before committing.
//! - S2.2 / S2.3 / S2.4 run `cargo nextest run` WITHOUT the env var
//!   and use `cargo insta review` interactively to accept-or-reject
//!   any diffs.
//! - Prior snapshots that regenerate in a later session signal a
//!   bug, not an expected diff — investigate before accepting.

use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

/// Helper — read a fixture by its crate-relative path and parse it.
fn parse_fixture(rel: &str) -> scrap_core::domain::parsed::ParsedTestFile {
    let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let source = std::fs::read_to_string(&abs)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", abs.display()));
    SynTestParser::new()
        .parse_test_source(&source, &FilePath::new(rel))
        .expect("fixture parses cleanly")
}

#[test]
fn snapshot_nested_mods() {
    let file = parse_fixture("tests/fixtures/nested_mods.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_attribute_variants() {
    let file = parse_fixture("tests/fixtures/attribute_variants.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_opt_outs() {
    let file = parse_fixture("tests/fixtures/opt_outs/allows.rs");
    insta::assert_yaml_snapshot!(file);
}

// ─── S2.2 snapshots: explicit-assertion recognition ─────────────────

#[test]
fn snapshot_zero_assertion() {
    let file = parse_fixture("tests/fixtures/true_positives/zero_assertion.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_tautological() {
    let file = parse_fixture("tests/fixtures/true_positives/tautological.rs");
    insta::assert_yaml_snapshot!(file);
}

// ─── S2.3 snapshots: macro-form implicit-assertion sources ──────────

#[test]
fn snapshot_proptest_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/proptest_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_kani_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/kani_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_insta_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/insta_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_pretty_assertions_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/pretty_assertions_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_proptest_macro_suffix() {
    let file = parse_fixture("tests/fixtures/runner_shells/proptest_macro_suffix.rs");
    insta::assert_yaml_snapshot!(file);
}

// ─── S2.4 snapshots: non-macro implicit sources + should_panic ──────

#[test]
fn snapshot_quickcheck_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/quickcheck_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_cucumber_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/cucumber_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_trybuild_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/trybuild_shell.rs");
    insta::assert_yaml_snapshot!(file);
}

#[test]
fn snapshot_should_panic_shell() {
    let file = parse_fixture("tests/fixtures/runner_shells/should_panic_shell.rs");
    insta::assert_yaml_snapshot!(file);
}
