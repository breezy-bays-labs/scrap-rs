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
fn fires_on_try_exists_surface_check() {
    // SHOULD-FIX: `try_exists()` is the RECOMMENDED existence API. A write
    // + `try_exists()` (surface only) + no read-back must fire, same as
    // `exists()`.
    assert!(fires(
        r#"
        #[test]
        fn writes_then_try_exists() {
            let p = "/tmp/scrap-e2e-try.txt";
            std::fs::write(p, b"data").unwrap();
            assert!(std::path::Path::new(p).try_exists().unwrap());
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
fn does_not_fire_when_read_back_is_inside_assert_matches() {
    // Cabinet CRITICAL #2: `assert_matches!(fs::read_to_string(p)?, Ok(s)
    // if ...)`'s SECOND arg is a pattern (not an Expr), so a whole-arglist
    // `Punctuated<Expr>` parse fails. A naive "drop all facts on parse
    // failure" loses the read → surface-only-io false-fires on a genuine
    // read+assert. The leading-Expr fallback captures the scrutinee
    // (`fs::read_to_string(p)?`, always arg 0) → read projected → suppressed.
    //
    // Discriminating: a surface check (`exists()`) is ALSO present, so the
    // detector WOULD fire if the read-back inside assert_matches! were
    // dropped. The leading-Expr fallback must capture the read → no fire.
    assert!(!fires(
        r#"
        #[test]
        fn reads_content_back_in_assert_matches() -> std::io::Result<()> {
            let p = "/tmp/scrap-e2e-am.txt";
            std::fs::write(p, b"data")?;
            assert!(std::path::Path::new(p).exists());
            assert_matches!(std::fs::read_to_string(p)?, Ok(s) if s == "data");
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

// ── Rebind-poison false-positive guards (cabinet CRITICAL #1) ────────────
//
// A name that is rebound (in ANY form), reassigned, or declared `mut`
// must NOT correlate a write to its pre-rebind value with a check on its
// post-rebind value — that is two different paths collapsing to one key.
// The binding-poison pre-pass routes any such name to a FRESH opaque key,
// so the write-site and check-site never group → no fire. (Fail-safe:
// miss rather than misfire.)

#[test]
fn does_not_fire_on_literal_rebind() {
    // T1: `let mut p = "/tmp/a"; write(p); p = "/tmp/b"; assert(p.exists());`
    // The write sees `p == "/tmp/a"`, the check sees `p == "/tmp/b"` — two
    // DIFFERENT files. Must NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn literal_rebind() {
            let mut p = "/tmp/scrap-rebind-a.txt";
            std::fs::write(p, b"x").unwrap();
            p = "/tmp/scrap-rebind-b.txt";
            assert!(std::path::Path::new(p).exists());
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_non_literal_rebind() {
    // T2 — THE GATE: a NON-LITERAL rebind. A name-based `bind:p` fallback
    // would still collapse the pre- and post-rebind `bind:p` keys and
    // FALSE-FIRE. The poisoned name must become FRESH OPAQUE so the write
    // and check land on distinct keys. Must NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn non_literal_rebind() {
            let mut p = make_path();
            std::fs::write(&p, b"x").unwrap();
            p = make_other();
            assert!(p.exists());
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_for_loop_rebind() {
    // T3: `p` is written outside the loop, then re-bound as the loop var.
    // The loop binding poisons `p` (Pat::Ident appears twice). Must NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn for_loop_rebind() {
            let p = "/tmp/scrap-forloop.txt";
            std::fs::write(p, b"x").unwrap();
            for p in [make_other()] {
                assert!(std::path::Path::new(p).exists());
            }
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_tuple_let_rebind() {
    // Tuple-destructure rebind: `let (a, p) = ...;` reaches `Pat::Ident`
    // leaves via default recursion, so `p` (bound twice across the body)
    // poisons. Pins that the poison pre-pass is form-agnostic (no
    // tuple-let enumeration). Must NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn tuple_let_rebind() {
            let p = "/tmp/scrap-tuple.txt";
            std::fs::write(p, b"x").unwrap();
            let (_a, p) = (1, make_other());
            assert!(p.exists());
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_in_macro_shadow_rebind() {
    // Fail-closed (scrap-rs#26 round-2): `PoisonScanner` now descends into
    // analyzed assertion-macro args, so the inner `let p` is a SECOND
    // `Pat::Ident` binding of `p` → count≥2 → poisoned → the outer write
    // and the in-macro check land on distinct opaque keys → no fire.
    assert!(!fires(
        r#"
        #[test]
        fn in_macro_shadow() {
            let p = "/tmp/scrap-inmacro-a.txt";
            std::fs::write(p, b"x").unwrap();
            assert!({ let p = "/tmp/scrap-inmacro-b.txt"; std::path::Path::new(p).exists() });
        }
        "#,
    ));
}

#[test]
fn fires_on_singly_bound_non_mut_name_positive_control() {
    // T5 — POSITIVE CONTROL: the canonical singly-bound, non-`mut` case
    // MUST STILL FIRE. If the poison pre-pass over-poisons (e.g. treats
    // count-1 names as suspect), the detector goes dead — this catches it.
    assert!(fires(
        r#"
        #[test]
        fn singly_bound_path() {
            let p = "/tmp/scrap-singly-bound.txt";
            std::fs::write(p, b"x").unwrap();
            assert!(std::path::Path::new(p).exists());
        }
        "#,
    ));
}

#[test]
fn fires_on_let_shadowing_with_same_value_regression() {
    // Regression: `let`-shadowing where the LATER binding is what both the
    // write and the check see still fires — shadowing rebinds the name, so
    // BOTH `p`s poison and the write/check land on distinct opaque keys →
    // NO fire. (Shadowing is a rebind; the fail-safe correctly treats it
    // as non-correlatable rather than guessing which binding each use
    // refers to.) Pins the post-poison behavior so a future change that
    // re-introduces last-let-wins is caught.
    assert!(!fires(
        r#"
        #[test]
        fn let_shadowing() {
            let p = "/tmp/scrap-shadow-a.txt";
            std::fs::write(p, b"x").unwrap();
            let p = "/tmp/scrap-shadow-b.txt";
            assert!(std::path::Path::new(p).exists());
        }
        "#,
    ));
}

// ── Fail-closed: poison-on-uncertainty (cabinet round-2) ─────────────────
//
// Any identifier that appears in a token region the walker did NOT fully
// analyze is poisoned (→ fresh unique opaque → can't group → suppressed).
// Two mechanisms: (A) `PoisonScanner` descends into ANALYZED
// assertion-macro args, so in-macro re-bindings count toward the ≥2
// poison trigger; (B) at give-up points (non-assertion macros, the
// unparseable tail of a partial assertion parse), every raw-token ident
// is harvested into the poison set. Each repro below pairs an outer
// `let`-bound write+check on `p` with an in-macro construct that re-binds
// or hides `p`; all MUST be suppressed.

#[test]
fn does_not_fire_on_in_macro_closure_param_rebind() {
    // Repro 1 (Move A): the closure param `|p|` is a SECOND `Pat::Ident`
    // binding of `p` → count≥2 → poisoned. `p` is `let`-bound outside so
    // the count reaches 2 (outer-let + closure-param).
    assert!(!fires(
        r#"
        #[test]
        fn in_macro_closure_param() {
            let p = "/tmp/scrap-closure.txt";
            std::fs::write(p, b"x").unwrap();
            assert!([make_other()].iter().any(|p| std::path::Path::new(p).exists()));
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_in_macro_tuple_let_rebind() {
    // Repro 2 (Move A): an in-macro tuple-`let (_a, p)` re-binds `p` →
    // count≥2 → poisoned.
    assert!(!fires(
        r#"
        #[test]
        fn in_macro_tuple_let() {
            let p = "/tmp/scrap-inmacro-tuple.txt";
            std::fs::write(p, b"x").unwrap();
            assert!({ let (_a, p) = (1, make_other()); std::path::Path::new(p).exists() });
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_in_macro_if_let_rebind() {
    // Repro 3 (Move A): an in-macro `if let Some(p)` re-binds `p` →
    // count≥2 → poisoned.
    assert!(!fires(
        r#"
        #[test]
        fn in_macro_if_let() {
            let p = "/tmp/scrap-inmacro-iflet.txt";
            std::fs::write(p, b"x").unwrap();
            assert!(if let Some(p) = make_opt() { std::path::Path::new(p).exists() } else { false });
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_in_macro_mut_assign_rebind() {
    // Repro 4 (Move A): an in-macro `let mut p` + reassignment. The `mut`
    // binding poisons `p` directly; it is also a second binding (count≥2).
    assert!(!fires(
        r#"
        #[test]
        fn in_macro_mut_assign() {
            let p = "/tmp/scrap-inmacro-mut.txt";
            std::fs::write(p, b"x").unwrap();
            assert!({ let mut p = make_path(); std::fs::write(&p, b"x").unwrap(); p = make_other(); std::path::Path::new(&p).exists() });
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_read_inside_matches_macro() {
    // Repro 5 (Move B): `matches!` is NOT an assertion macro, so its tokens
    // are a give-up region. `p` is harvested from the raw `matches!` tokens
    // → poisoned. The write + outer `exists()` therefore can't correlate
    // (and the read inside `matches!` is never captured — correct, the
    // path is opaque). MUST NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn read_inside_matches() -> std::io::Result<()> {
            let p = "/tmp/scrap-matches.txt";
            std::fs::write(p, b"data")?;
            assert!(std::path::Path::new(p).exists());
            assert!(matches!(std::fs::read_to_string(p)?, Ok(_)));
            Ok(())
        }
        "#,
    ));
}

#[test]
fn does_not_fire_on_read_inside_assert_matches_guard() {
    // Repro 6 (Move B): the `assert_matches!` GUARD tail (`if <expr>`) is a
    // give-up region — `parse_leading_expr` captures only the scrutinee
    // (arg 0 = `flag`), and the guard `fs::read_to_string(p)? == "data"`
    // is harvested → `p` poisoned. The outer write + exists can't
    // correlate. MUST NOT fire.
    assert!(!fires(
        r#"
        #[test]
        fn read_inside_assert_matches_guard() -> std::io::Result<()> {
            let p = "/tmp/scrap-am-guard.txt";
            let flag: std::io::Result<()> = Ok(());
            std::fs::write(p, b"data")?;
            assert!(std::path::Path::new(p).exists());
            assert_matches!(flag, Ok(_) if std::fs::read_to_string(p)? == "data");
            Ok(())
        }
        "#,
    ));
}

#[test]
fn does_not_fire_when_path_appears_in_unknown_macro() {
    // Unknown-macro gate (THE gate): a write + exists on `p`, then `p`
    // appears in a totally-unknown macro → `p` is harvested from the
    // unknown macro's give-up region → poisoned → no fire. This is the
    // by-construction proof that fail-closed covers macros we've never
    // heard of (custom test macros, future stdlib macros, ...).
    assert!(!fires(
        r#"
        #[test]
        fn path_in_unknown_macro() {
            let p = "/tmp/scrap-unknown.txt";
            std::fs::write(p, b"x").unwrap();
            assert!(std::path::Path::new(p).exists());
            totally_made_up_macro!(p);
        }
        "#,
    ));
}

// ── Over-suppression boundary: harvest the TAIL only, never the blob ─────

#[test]
fn fires_on_assert_matches_scrutinee_surface_check() {
    // Boundary proof (guards against whole-blob harvest): the SCRUTINEE of
    // `assert_matches!` is an ANALYZED region (arg 0). A write + a surface
    // check on `p` in the scrutinee, with no read, MUST still fire. If the
    // harvest poisoned the whole macro blob (scrutinee included), `p` would
    // be poisoned and the detector would go dead here.
    assert!(fires(
        r#"
        #[test]
        fn assert_matches_scrutinee_surface() {
            let p = "/tmp/scrap-am-scrutinee.txt";
            std::fs::write(p, b"data").unwrap();
            assert_matches!(p.metadata(), Ok(m) if m.len() > 0);
        }
        "#,
    ));
}
