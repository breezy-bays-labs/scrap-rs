//! End-to-end test of the `surface-only-io` detector (scrap-rs#26) — the
//! first correlation detector — against real Rust source via the syn
//! parser. Each source string parses through [`SynTestParser`]; the
//! resulting `ParsedTest`(s) feed
//! [`scrap_core::detectors::surface_only_io::detect`].
//!
//! These tests are the **real correlation guard**: they exercise the full
//! parser → detector stack so a write-site key and a check-site key that
//! drift (e.g. `lit:/tmp/x` vs `lit:"/tmp/x"`) surface as a missed smell
//! (detector returns `None` when it should fire). Isolated projection unit
//! tests in `parser::body` can't catch a key mismatch; only these same-key
//! round-trips can.

use scrap_core::detectors::surface_only_io::detect;
use scrap_core::domain::config::DetectorConfig;
use scrap_core::domain::parsed::ParsedTest;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

/// Parse a single-test source string and return its one `ParsedTest`.
fn parse_one(source: &str) -> ParsedTest {
    let mut tests = SynTestParser::new()
        .parse_test_source(source, &FilePath::new("e2e.rs"))
        .expect("source parses cleanly")
        .tests;
    assert_eq!(tests.len(), 1, "expected exactly one #[test] fn");
    tests.remove(0)
}

/// `true` when `surface-only-io` fires on the parsed source.
fn fires(source: &str) -> bool {
    detect(&parse_one(source), &DetectorConfig::default()).is_some()
}

// ── Each key form: write + surface check on the SAME key MUST fire ──────

#[test]
fn fires_on_literal_key_write_then_exists() {
    // The classic normalization-drift trap: the write-site `lit:` key and
    // the `Path::new(<lit>).exists()` check-site key must be byte-identical.
    assert!(fires(
        r#"
        #[test]
        fn writes_then_checks_existence() {
            std::fs::write("/tmp/scrap-e2e.txt", b"data").unwrap();
            assert!(std::path::Path::new("/tmp/scrap-e2e.txt").exists());
        }
        "#,
    ));
}

#[test]
fn fires_on_bound_ident_write_then_exists() {
    // `let p = "..."; fs::write(p, ..); assert!(p.exists());` — both sites
    // resolve `p` through the binding map to the same `lit:` key.
    assert!(fires(
        r#"
        #[test]
        fn bound_path_write_then_exists() {
            let p = "/tmp/scrap-e2e-bound.txt";
            std::fs::write(p, b"data").unwrap();
            assert!(p.exists());
        }
        "#,
    ));
}

#[test]
fn fires_on_tempfile_path_surface_check() {
    // The temp file IS created on disk (Tempfile write); `.path().exists()`
    // aliases back to the same handle key and only checks existence → fires.
    assert!(fires(
        r#"
        #[test]
        fn tempfile_only_checks_existence() -> std::io::Result<()> {
            let f = NamedTempFile::new()?;
            std::fs::write(f.path(), b"data")?;
            assert!(f.path().exists());
            Ok(())
        }
        "#,
    ));
}

#[test]
fn fires_on_metadata_length_only_check() {
    // A write + length-only `metadata().len()` (a SURFACE check, not a
    // read) + no content read-back → fires.
    assert!(fires(
        r#"
        #[test]
        fn writes_then_checks_only_length() -> std::io::Result<()> {
            let p = "/tmp/scrap-e2e-len.txt";
            std::fs::write(p, b"data")?;
            assert!(std::fs::metadata(p)?.len() > 0);
            Ok(())
        }
        "#,
    ));
}

// ── Read-back guard: write + read-back MUST NOT fire ────────────────────

#[test]
fn does_not_fire_when_content_is_read_back() {
    // HEADLINE: write + read_to_string on the same key → the test inspects
    // the substantive payload → no fire.
    assert!(!fires(
        r#"
        #[test]
        fn writes_then_reads_content_back() -> std::io::Result<()> {
            std::fs::write("/tmp/scrap-e2e-rb.txt", b"data")?;
            let got = std::fs::read_to_string("/tmp/scrap-e2e-rb.txt")?;
            assert_eq!(got, "data");
            Ok(())
        }
        "#,
    ));
}

#[test]
fn does_not_fire_when_read_back_is_inside_the_assertion_macro() {
    // The CANONICAL read-back idiom: the read lives INSIDE `assert_eq!`.
    // Proves macro-token descent reaches reads (not just surface checks):
    // the parser sees `fs::read_to_string(p)` nested in the assertion, so
    // the read disarms surface-only-io for that key.
    assert!(!fires(
        r#"
        #[test]
        fn reads_content_back_in_assert() -> std::io::Result<()> {
            std::fs::write("/tmp/scrap-e2e-rbm.txt", b"data")?;
            assert_eq!(std::fs::read_to_string("/tmp/scrap-e2e-rbm.txt")?, "data");
            Ok(())
        }
        "#,
    ));
}

#[test]
fn does_not_fire_when_only_writing() {
    // A write with no surface check at all is not the smell.
    assert!(!fires(
        r#"
        #[test]
        fn just_writes() -> std::io::Result<()> {
            std::fs::write("/tmp/scrap-e2e-w.txt", b"data")?;
            Ok(())
        }
        "#,
    ));
}

// ── Correlation isolation: different keys must not cross-correlate ──────

#[test]
fn does_not_fire_when_write_and_check_are_different_paths() {
    // Write to A, check existence of B → no key has both → no fire.
    assert!(!fires(
        r#"
        #[test]
        fn write_a_check_b() {
            std::fs::write("/tmp/scrap-e2e-a.txt", b"data").unwrap();
            assert!(std::path::Path::new("/tmp/scrap-e2e-b.txt").exists());
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_distinct_opaque_paths() {
    // Two `format!(..)` paths → distinct opaque keys → never correlate.
    assert!(!fires(
        r#"
        #[test]
        fn opaque_write_and_check() {
            let dir = "/tmp";
            std::fs::write(format!("{dir}/a.txt"), b"data").unwrap();
            assert!(std::path::Path::new(&format!("{dir}/a.txt")).exists());
        }
        "#,
    ));
}
