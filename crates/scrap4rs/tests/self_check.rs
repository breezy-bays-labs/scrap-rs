//! In-repo dogfood: run each `scrap-core` detector against scrap4rs's
//! own source tree (`crates/scrap4rs/src/`) and assert zero findings.
//!
//! Per Christopher's per-detector self-check pattern (decision
//! 2026-05-26), every v0.1 detector adds one `test_<detector>_self_check`
//! here as it ships. The first such test (`test_zero_assertion_self_check`,
//! scrap-rs#30) lands with the zero-assertion detector;
//! `tautological_assertion_self_check` (scrap-rs#24) appends here on
//! the same scaffold; #25 / #26 / #27 follow with `test_no_op_io_self_check`,
//! `test_surface_only_io_self_check`, `test_large_example_self_check`.
//!
//! Why this lives in `crates/scrap4rs/tests/` rather than `crates/scrap-core/tests/`:
//! scrap-core deps deny AST libraries; the self-check needs to parse real
//! Rust source via `SynTestParser`. This is the cleanest cross-port
//! integration surface — scrap-core's detectors consume `ParsedTest`,
//! scrap4rs's parser produces it, and the self-check exercises the full
//! parser-to-detector stack on the workspace's own source.
//!
//! Why scrap-core's source isn't included: detectors live in scrap-core
//! and walking that tree would create a circular semantic dependency
//! (the detector grading its own implementation). scrap4rs's source
//! (the parser adapter + thin main) is the right dogfood surface.
//!
//! Self-check expectation: every test fn in `crates/scrap4rs/src/`
//! includes explicit `assert*!` macros or implicit-assertion sources
//! (`should_panic`, runner shells) or `.unwrap()`/`.expect()` chains —
//! so the zero-assertion detector should NEVER fire. A non-zero finding
//! count is a real regression: either scrap4rs's source has acquired a
//! genuinely zero-assertion test (fix the test) OR the detector has a
//! false positive (fix the detector). Either way, the test names the
//! offending fixture so the next step is clear.

use scrap_core::adapters::source::fs::FsWalker;
use scrap_core::cli::config::DetectorConfig;
use scrap_core::detectors::{no_op_io, surface_only_io, tautological_assertion, zero_assertion};
use scrap_core::domain::config::AnalysisConfig;
use scrap_core::domain::types::{FilePath, SourceRoot};
use scrap_core::ports::parser::TestParserPort;
use scrap_core::ports::source::SourcePort;
use scrap4rs::parser::SynTestParser;

/// Walk `crates/scrap4rs/src/`, parse every `.rs` file via `SynTestParser`,
/// and return the projected `ParsedTest`s. Skips files that fail to parse
/// (none expected on a clean tree; would surface as a separate test
/// regression if it happened).
fn parse_scrap4rs_src() -> Vec<scrap_core::domain::parsed::ParsedTest> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_root = manifest_dir.join("src");
    let cfg = AnalysisConfig::new(
        SourceRoot::new(&src_root),
        Vec::new(),
        vec!["rs".to_string()],
        true,
    );
    let walker = FsWalker::try_new(cfg).expect("walker construction");
    let outcome = walker.discover_test_files().expect("source walk");

    let parser = SynTestParser::new();
    let mut all_tests = Vec::new();
    for file_path in &outcome.files {
        // FsWalker returns paths relative to `AnalysisConfig::src` (per
        // its docstring: "Emitted file paths are relative to
        // `AnalysisConfig::src` so reports and snapshots are stable
        // across machines"). Reading from disk requires the absolute
        // path; we keep the FilePath stamp on `ParsedTest::identity`
        // relative so test names don't include `/Users/...` prefixes.
        let abs = src_root.join(file_path.as_path());
        let source =
            std::fs::read_to_string(&abs).unwrap_or_else(|e| panic!("read {}: {e}", abs.display()));
        let parsed = parser
            .parse_test_source(&source, &FilePath::new(file_path.as_path()))
            .unwrap_or_else(|e| panic!("parse {file_path}: {e:?}"));
        all_tests.extend(parsed.tests);
    }
    all_tests
}

#[test]
fn test_zero_assertion_self_check() {
    // CQO FOLD-REQUIRED pattern (memory `feedback_pristine-test-output`):
    // collect all offending findings BEFORE asserting, so the failure
    // names every offender at once. One-fixture-fails-stops-the-loop
    // hides regressions from downstream agentic loops.
    let tests = parse_scrap4rs_src();
    assert!(
        !tests.is_empty(),
        "self-check guard: expected to find at least one #[test] fn in crates/scrap4rs/src/; \
         got zero — either the walker regressed or scrap4rs has no tests",
    );

    let cfg = DetectorConfig::default();
    let mut offenders: Vec<String> = Vec::new();
    for parsed in &tests {
        if zero_assertion::detect(parsed, &cfg).is_some() {
            offenders.push(format!(
                "{file}::{name}",
                file = parsed.identity.file_path,
                name = parsed.identity.qualified_name.as_str(),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "zero-assertion self-check failed: {n} test(s) in crates/scrap4rs/src/ trigger the detector:\n  - {list}\n\
         \nEither the test is genuinely smelly (add assertions / implicit source / .unwrap()) \
         or the detector has a false positive (fix the detector + add a runner-shell fixture).",
        n = offenders.len(),
        list = offenders.join("\n  - "),
    );
}

#[test]
fn test_tautological_assertion_self_check() {
    // scrap-rs#24 — `tautological-assertion` detector dogfood.
    //
    // The detector emits a `Finding` whenever a `ParsedTest` carries an
    // assertion whose `arguments_identical` is true OR whose
    // `single_arg_value` is `Some(LiteralValue::Bool(true))`. scrap4rs's
    // own production code (excluding test fixtures) must contain zero
    // such patterns. If this test fails, fix the tautology in-stream
    // rather than suppressing the detector — the production code should
    // not ship with intentionally meaningless assertions.
    //
    // Signature-aligned with its siblings at scrap-rs#99:
    // `tautological_assertion::detect(parsed, &cfg)` now consults
    // `DetectorConfig` for `enabled`/`penalty` (so it can be wired into
    // `detect_all` uniformly). opt-out + Skip/Advisory policy still live
    // in the pipeline driver (scrap-rs#72), not in the detector.
    let tests = parse_scrap4rs_src();
    assert!(
        !tests.is_empty(),
        "self-check guard: expected to find at least one #[test] fn in crates/scrap4rs/src/; \
         got zero — either the walker regressed or scrap4rs has no tests",
    );

    let cfg = DetectorConfig::default();
    let mut offenders: Vec<String> = Vec::new();
    for parsed in &tests {
        if tautological_assertion::detect(parsed, &cfg).is_some() {
            offenders.push(format!(
                "{file}::{name}",
                file = parsed.identity.file_path,
                name = parsed.identity.qualified_name.as_str(),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "tautological-assertion self-check failed: {n} test(s) in crates/scrap4rs/src/ trigger the detector:\n  - {list}\n\
         \nEither the test is genuinely smelly (replace the tautology with an assertion that can actually fail) \
         or the detector has a false positive (fix the detector + add a fixture under tests/fixtures/).",
        n = offenders.len(),
        list = offenders.join("\n  - "),
    );
}

#[test]
fn test_no_op_io_self_check() {
    // scrap-rs#25 — `no-op-io` detector dogfood.
    //
    // The detector emits a `Finding` when a `ParsedTest` carries a
    // `BehavioralFact::ResultDiscarded` AND no positive check (no
    // assertion, no implicit source, no `.unwrap()`/`.expect()` chain).
    // scrap4rs's own production tests either assert, use a runner shell,
    // or `.unwrap()`/`.expect()` their Results — so no-op-io must NEVER
    // fire. A non-zero count is a real regression: either a src test
    // genuinely discards a Result without checking it (fix the test) or
    // the detector over-fires (fix the detector + add a runner-shell
    // fixture).
    //
    // Like tautological_assertion + zero_assertion, no_op_io::detect
    // consults `DetectorConfig` (enabled/penalty) but NOT opt-outs —
    // those live in the pipeline driver (scrap-rs#72).
    let tests = parse_scrap4rs_src();
    assert!(
        !tests.is_empty(),
        "self-check guard: expected to find at least one #[test] fn in crates/scrap4rs/src/; \
         got zero — either the walker regressed or scrap4rs has no tests",
    );

    let cfg = DetectorConfig::default();
    let mut offenders: Vec<String> = Vec::new();
    for parsed in &tests {
        if no_op_io::detect(parsed, &cfg).is_some() {
            offenders.push(format!(
                "{file}::{name}",
                file = parsed.identity.file_path,
                name = parsed.identity.qualified_name.as_str(),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "no-op-io self-check failed: {n} test(s) in crates/scrap4rs/src/ trigger the detector:\n  - {list}\n\
         \nEither the test genuinely discards a Result without checking it (inspect or assert on the value, \
         or use .unwrap()/.expect()) or the detector has a false positive (fix the detector + add a fixture).",
        n = offenders.len(),
        list = offenders.join("\n  - "),
    );
}

#[test]
fn test_surface_only_io_self_check() {
    // scrap-rs#26 — `surface-only-io` detector dogfood (the first
    // correlation detector).
    //
    // The detector emits a `Finding` when, for some `path_key`, a
    // `ParsedTest` carries a FilesystemWrite AND a FilesystemSurfaceCheck
    // but NO FilesystemRead. scrap4rs's own production tests parse
    // in-memory source STRINGS (they do not write real files and then
    // check only their surface), so surface-only-io must NEVER fire. A
    // non-zero count is a real regression: either a src test genuinely
    // writes-and-surface-checks-without-reading (fix the test by reading
    // the content back and asserting on it) or the detector over-fires
    // (fix the detector + add a runner-shell fixture).
    //
    // Like its siblings, surface_only_io::detect consults `DetectorConfig`
    // (enabled/penalty) but NOT opt-outs — those live in the pipeline
    // driver (scrap-rs#72). It also deliberately does NOT consult
    // has_positive_check, so an honest `assert!(p.exists())` would still
    // fire it (the suppression-reconciliation design point); the self-check
    // surfaces any such src test rather than silencing it.
    let tests = parse_scrap4rs_src();
    assert!(
        !tests.is_empty(),
        "self-check guard: expected to find at least one #[test] fn in crates/scrap4rs/src/; \
         got zero — either the walker regressed or scrap4rs has no tests",
    );

    let cfg = DetectorConfig::default();
    let mut offenders: Vec<String> = Vec::new();
    for parsed in &tests {
        if surface_only_io::detect(parsed, &cfg).is_some() {
            offenders.push(format!(
                "{file}::{name}",
                file = parsed.identity.file_path,
                name = parsed.identity.qualified_name.as_str(),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "surface-only-io self-check failed: {n} test(s) in crates/scrap4rs/src/ trigger the detector:\n  - {list}\n\
         \nEither the test genuinely writes a file and checks only its surface (existence/metadata) without \
         reading the content back (fix the test by reading + asserting on the content) or the detector has a \
         false positive (fix the detector + add a runner-shell fixture).",
        n = offenders.len(),
        list = offenders.join("\n  - "),
    );
}
