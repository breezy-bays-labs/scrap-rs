//! Golden snapshot harness for the scrap-rs demo corpus.
//!
//! Walks `examples/<smell>/` for subdirectories containing
//! `expected.json`. For each fixture:
//!
//! - **Positive (`bad.rs`)**: parse via `SynTestParser`, run the
//!   zero-assertion detector on each `ParsedTest`, build a `Report`,
//!   emit the v0.1 JSON envelope, and compare against the committed
//!   `expected.json` via `serde_json::Value` equality.
//! - **Negative (`good.rs`)**: parse + run detectors, assert zero
//!   findings.
//!
//! Set `BLESS=1` to regenerate every fixture's `expected.json` from
//! live output. The harness appends a trailing `\n` on write (so an
//! editor's auto-insert-final-newline on the next save produces zero
//! `git diff`) and parses via `from_slice` on read (tolerates the
//! trailing `\n`). See `README.md` Section 4 "Bless workflow" for
//! the full developer flow.
//!
//! Determinism guards (so `expected.json` is byte-stable across
//! machines + versions):
//! - Fixed `AdapterMeta` literal: `tool: "scrap4rs"`,
//!   `language: "rust"`, `tool_version: "0.1.0"`,
//!   `config_file_name: "scrap4rs.toml"`.
//! - Fixed timestamp constant (`FIXED_TIMESTAMP`).
//! - `tool_version` is hard-coded `"0.1.0"`, NOT
//!   `env!("CARGO_PKG_VERSION")` — version bumps would otherwise
//!   invalidate every fixture's `expected.json`. See `README.md`
//!   Section "Wire shape" for the rationale.
//! - `serde_json::Map<String, Value>` is `BTreeMap`-backed (no
//!   `preserve_order` feature on the workspace dep), so emitted keys
//!   are alphabetical and byte-stable across `to_string_pretty`
//!   calls.

// `scrap_core::adapter_meta` import is a workaround for the
// scrap4rs re-export gap tracked in scrap-rs#86 (scrap4rs/src/lib.rs:18
// omits `adapter_meta` from its strict-superset re-export). Once #86
// lands, change to `use scrap4rs::adapter_meta::AdapterMeta;` and
// drop the `scrap-core` dev-dep from Cargo.toml.
use scrap_core::adapter_meta::AdapterMeta;
use scrap4rs::adapters::reporters::json::{EmitOptions, emit};
use scrap4rs::cli::config::DetectorConfig;
use scrap4rs::detectors::zero_assertion;
use scrap4rs::domain::finding::Finding;
use scrap4rs::domain::report::{FileReport, Report, Summary};
use scrap4rs::domain::threshold::ThresholdMode;
use scrap4rs::domain::types::FilePath;
use scrap4rs::parser::SynTestParser;
use scrap4rs::ports::parser::TestParserPort;
use serde_json::Value;
use std::path::{Path, PathBuf};

// ────────────────────────────────────────────────────────────────────
// Determinism guards (per shape doc D6)
// ────────────────────────────────────────────────────────────────────

/// Fixed timestamp injected into the envelope so `expected.json` is
/// deterministic across machines and CI runs.
const FIXED_TIMESTAMP: &str = "2026-05-27T00:00:00Z";

/// Hard-coded adapter version on the wire. NOT
/// `env!("CARGO_PKG_VERSION")` — version bumps would otherwise
/// invalidate every fixture's `expected.json`. See README "Wire
/// shape" for the rationale.
const TOOL_VERSION: &str = "0.1.0";

/// Construct the fixed `AdapterMeta` literal used across all
/// fixtures. `AdapterMeta` is `Copy` post-scrap-rs#21 (FORK-1 fold);
/// 13-field shape per the same wave.
fn harness_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "scrap4rs",
        language: "rust",
        tool_version: TOOL_VERSION,
        long_version: "0.1.0 (snapshot 2026-05-27)",
        about: "snapshot-test harness",
        long_about: "Demo-corpus snapshot harness AdapterMeta for the scrap-examples crate.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://github.com/breezy-bays-labs/scrap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/scrap-rs#detection-rules",
        config_file_name: "scrap4rs.toml",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        parse_hint: "ensure --src points at a Cargo workspace with test files",
    }
}

// ────────────────────────────────────────────────────────────────────
// Fixture discovery (per shape doc D4)
// ────────────────────────────────────────────────────────────────────

/// Absolute path to `crates/scrap-examples/examples/`.
fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Walk `examples/` for fixture subdirectories.
///
/// A directory is a fixture if it contains `bad.rs` (the smell-
/// triggering source — the file that asserts this directory's intent
/// to be a fixture). Verify-mode handling of missing `expected.json`
/// (half-finished WIP) is the per-test caller's concern, not the
/// discovery layer's. BLESS mode is precisely the operation that
/// GENERATES the first `expected.json`, so requiring it as a
/// discovery precondition would be circular.
///
/// Panics if the root is missing or no fixtures are discovered — an
/// empty corpus is a hard error, not a green pass.
fn discover_fixtures() -> Vec<PathBuf> {
    let root = examples_root();
    let read = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read examples root {}: {e}", root.display()));

    let mut fixtures: Vec<PathBuf> = read
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() && path.join("bad.rs").is_file() {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    // Deterministic order across platforms: sort by directory name.
    // `discover_fixtures` is otherwise at the mercy of `read_dir`'s
    // filesystem-defined order, which varies (Linux ext4 vs macOS
    // APFS vs ZFS) and would cause failures to surface in different
    // orders across CI matrix runners.
    fixtures.sort();

    assert!(
        !fixtures.is_empty(),
        "no fixtures discovered under {} — corpus must be non-empty",
        root.display(),
    );
    fixtures
}

// ────────────────────────────────────────────────────────────────────
// Pipeline + I/O helpers
// ────────────────────────────────────────────────────────────────────

/// Read `source_path`, parse it via `SynTestParser`, run every wired
/// detector against each `ParsedTest`, and build a single-`FileReport`
/// `Report` with the surviving findings.
///
/// `relative_path` is the path string embedded in the wire envelope's
/// `result.files[].file_path` and `result.files[].findings[].test.file_path`
/// fields (the `expected.json` golden). Uses a fixture-relative form
/// so the same `expected.json` is byte-stable across worktrees with
/// different absolute prefixes.
///
/// **NB**: only the zero-assertion detector is wired today. As more
/// detectors land in `scrap_core::detectors`, append their `detect`
/// calls here; the corpus reflects the wire shape of every detector
/// regardless of which subdirectory the fixture lives in. Cross-
/// detector behaviour (a `bad.rs` that triggers two detectors) is
/// captured naturally.
fn run_pipeline(source_path: &Path, relative_path: &str) -> Report {
    let source = std::fs::read_to_string(source_path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", source_path.display()));

    let parsed_file = SynTestParser::new()
        .parse_test_source(&source, &FilePath::new(relative_path))
        .unwrap_or_else(|e| panic!("parse fixture {}: {e:?}", source_path.display()));

    let cfg = DetectorConfig::default();
    let findings: Vec<Finding> = parsed_file
        .tests
        .iter()
        .filter_map(|parsed_test| zero_assertion::detect(parsed_test, &cfg))
        .collect();

    let summary = Summary::from_findings(findings.iter());
    let file_report = FileReport::new(FilePath::new(relative_path), findings);
    Report {
        files: vec![file_report],
        summary,
        // Gate verdict is the analyzer pipeline's concern (scrap-rs#75);
        // the harness emits a default `false` — the corpus is the wire
        // shape, not the gate. When CLI #21 lands, the pipeline will
        // compute this; until then, fixtures pin `passed: false`.
        passed: false,
    }
}

/// Emit the v0.1 JSON envelope for a `Report` and parse it back to a
/// `serde_json::Value`. Returning `Value` (not bytes) feeds the
/// round-trip diff in `assert_envelope_matches`.
fn emit_envelope(report: &Report) -> Value {
    let mut buf = Vec::new();
    emit(
        report,
        &harness_meta(),
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit JSON envelope");
    serde_json::from_slice(&buf).expect("emit produced valid JSON envelope")
}

/// Read `<fixture_dir>/expected.json` and parse to `Value`. Uses
/// `from_slice` (tolerates trailing `\n`) per cabinet `CEng MF1`.
fn read_expected(fixture_dir: &Path) -> Value {
    let path = fixture_dir.join("expected.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Pretty-print `value` to `<fixture_dir>/expected.json` with a
/// trailing `\n` (cabinet `CEng MF1` contract). Editor's auto-insert-final-
/// newline on next save produces zero `git diff`.
fn write_expected(fixture_dir: &Path, value: &Value) {
    let pretty = serde_json::to_string_pretty(value).expect("serialize expected.json");
    let path = fixture_dir.join("expected.json");
    std::fs::write(&path, format!("{pretty}\n"))
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// True iff `BLESS=1` (or any truthy value) is set in the env.
fn bless_mode() -> bool {
    std::env::var("BLESS").is_ok_and(|v| !v.is_empty() && v != "0")
}

// ────────────────────────────────────────────────────────────────────
// Tests (per shape doc D11)
// ────────────────────────────────────────────────────────────────────

/// For every discovered fixture: parse `bad.rs`, run the detector
/// pipeline, emit the envelope, and either bless `expected.json`
/// (if `BLESS=1`) or compare via `serde_json::Value` equality.
///
/// Accumulates mismatches before asserting (pristine-test-output
/// pattern per `feedback_pristine-test-output`) — one bad fixture
/// must not hide the others from downstream agentic loops.
#[test]
fn bad_rs_emission_matches_expected_json() {
    let bless = bless_mode();
    let mut mismatches: Vec<String> = Vec::new();

    for fixture_dir in discover_fixtures() {
        let bad_path = fixture_dir.join("bad.rs");
        // discover_fixtures already filtered by bad.rs presence.

        let fixture_name = fixture_dir
            .file_name()
            .and_then(|s| s.to_str())
            .expect("fixture directory has a valid UTF-8 name");
        let relative_path = format!("crates/scrap-examples/examples/{fixture_name}/bad.rs");

        let report = run_pipeline(&bad_path, &relative_path);
        let actual = emit_envelope(&report);

        if bless {
            write_expected(&fixture_dir, &actual);
            continue;
        }

        // Verify mode: missing `expected.json` is a HARD FAILURE.
        // A fixture with `bad.rs` but no `expected.json` is a
        // half-blessed state that must not land on main; the dev
        // needs to run BLESS=1 and commit the result before the PR
        // merges. Per `feedback_pristine-test-output`: never emit
        // stderr "skipping" noise that agentic loops would misread
        // as a passing test.
        if !fixture_dir.join("expected.json").is_file() {
            mismatches.push(format!(
                "fixture: {fixture_name} — bad.rs exists but expected.json is missing; run BLESS=1 cargo test -p scrap-examples and commit the result",
            ));
            continue;
        }

        let expected = read_expected(&fixture_dir);
        if actual != expected {
            let actual_pretty = serde_json::to_string_pretty(&actual).unwrap_or_default();
            let expected_pretty = serde_json::to_string_pretty(&expected).unwrap_or_default();
            mismatches.push(format!(
                "fixture: {fixture_name}\n--- expected ---\n{expected_pretty}\n--- actual ---\n{actual_pretty}",
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} fixture(s) drifted; re-run with BLESS=1 to regenerate after review:\n\n{}",
        mismatches.len(),
        mismatches.join("\n\n"),
    );
}

/// For every discovered fixture: parse `good.rs`, run the detector
/// pipeline, assert zero findings. Same accumulate-and-assert
/// pattern as `bad_rs_emission_matches_expected_json`.
#[test]
fn good_rs_does_not_trigger() {
    let mut unexpected_triggers: Vec<String> = Vec::new();

    for fixture_dir in discover_fixtures() {
        let fixture_name = fixture_dir
            .file_name()
            .and_then(|s| s.to_str())
            .expect("fixture directory has a valid UTF-8 name");
        let good_path = fixture_dir.join("good.rs");
        if !good_path.is_file() {
            unexpected_triggers.push(format!("fixture: {fixture_name} — missing good.rs"));
            continue;
        }
        let relative_path = format!("crates/scrap-examples/examples/{fixture_name}/good.rs");

        let report = run_pipeline(&good_path, &relative_path);
        let finding_count: usize = report.files.iter().map(|f| f.findings.len()).sum();
        if finding_count > 0 {
            unexpected_triggers.push(format!(
                "fixture: {fixture_name} (good.rs) triggered {finding_count} finding(s) — detector is over-eager OR good.rs accidentally contains the smell",
            ));
        }
    }

    assert!(
        unexpected_triggers.is_empty(),
        "{} fixture(s) had unexpected triggers on good.rs:\n  - {}",
        unexpected_triggers.len(),
        unexpected_triggers.join("\n  - "),
    );
}
