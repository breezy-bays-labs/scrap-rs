//! Integration test pinning the wire-shape of a positive
//! `zero-assertion` detector emission.
//!
//! Lives in `tests/` (not `src/`) so the snapshot file co-locates with
//! the harness path-prefix that `insta::Settings` expects, and so the
//! snapshot becomes the contract that downstream reporters
//! (scrap-rs#14 json envelope, future SARIF reporter) consume.
//!
//! **Pre-#76 interim shape**: this PR is sequenced behind scrap-rs#76
//! (Smell.span field + 5-arg `Smell::new`). Until #76 merges and this
//! branch rebases, the emitted `Smell` carries no `span` field — the
//! snapshot reflects that shape. Post-rebase, the detector's
//! `Smell::new(...)` call appends `Some(parsed.identity.span)` as the
//! 5th positional arg, and this snapshot regenerates to add
//! `span: { start_line: 1, end_line: 5 }` to the Smell. Both the call
//! site (in `detectors/zero_assertion.rs`) and this snapshot regenerate
//! together at rebase time; the regeneration is an expected, known
//! diff, not a regression.

use scrap_core::cli::config::DetectorConfig;
use scrap_core::detectors::zero_assertion::detect;
use scrap_core::domain::parsed::ParsedTest;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::collections::BTreeSet;

/// Build a positive-trigger `ParsedTest`: empty assertions, empty
/// implicit sources, empty behavioral facts. Detector fires under
/// `DetectorConfig::default()`.
fn smelly_test() -> ParsedTest {
    ParsedTest::new(
        TestIdentity::new(
            FilePath::new("crates/foo/src/bar.rs"),
            QualifiedName::new("foo::bar::tests::it_does_a_thing"),
            Span::new(1, 5, 1, 1),
        ),
        Vec::new(),
        Vec::new(),
        3,
        Vec::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    )
}

#[test]
fn positive_finding_pins_wire_shape() {
    let parsed = smelly_test();
    let finding =
        detect(&parsed, &DetectorConfig::default()).expect("smelly test triggers zero-assertion");

    insta::assert_yaml_snapshot!("positive_finding", finding);
}
