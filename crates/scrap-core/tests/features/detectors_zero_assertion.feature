# Executable behavioral contract for the zero-assertion detector
# (scrap-rs#30). Run via the cucumber-rs harness at
# `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/detectors_zero_assertion.rs`
# (mod-block split per W5.1 convention).
#
# 5 scenarios cover the detector's three-clause detection rule:
# - 1 positive (empty facts → Some(Finding))
# - 1 negative via explicit assertion
# - 1 negative via implicit-assertion source (should_panic)
# - 1 config gate (enabled = false short-circuits)
# - 1 config gate (custom penalty override flows through)
#
# Per `feedback_bdd-cross-port-consistency`: ship the .feature even
# though the detector's surface is thin — cross-port reading wins
# over per-port pragmatism. Sibling tautological detector (scrap-rs#24)
# mirrors this shape.

Feature: zero-assertion detector flags tests with no observable effect
  As an agent author running scrap4rs in CI
  I want tests that exercise the SUT but never assert on its result to surface as findings
  So that I catch zero-coverage-by-construction before merge.

  Background:
    Given a fresh test World

  Scenario: detector fires on a test with no assertions, no implicit sources, and no behavioral facts
    Given a `ParsedTest` with no assertions and no implicit assertion sources
    When the caller invokes `zero_assertion::detect()` with the default `DetectorConfig`
    Then the result is `Some(Finding)` with category `zero_assertion`, severity `high`, actionability `auto_refactor`, and penalty 10

  Scenario: detector skips a test whose body has an explicit assertion macro
    Given a `ParsedTest` with one `assert_eq` assertion
    When the caller invokes `zero_assertion::detect()` with the default `DetectorConfig`
    Then the result is `None`

  Scenario: detector skips a test whose body has an implicit assertion source
    Given a `ParsedTest` with implicit assertion source `should_panic`
    When the caller invokes `zero_assertion::detect()` with the default `DetectorConfig`
    Then the result is `None`

  Scenario: detector respects `enabled = false` config short-circuit
    Given a `ParsedTest` with no assertions and no implicit assertion sources
    When the caller invokes `zero_assertion::detect()` with a `DetectorConfig` where `enabled = false`
    Then the result is `None`

  Scenario: detector applies custom penalty override
    Given a `ParsedTest` with no assertions and no implicit assertion sources
    When the caller invokes `zero_assertion::detect()` with a `DetectorConfig` where `penalty = 25`
    Then the result is `Some(Finding)` with category `zero_assertion` and penalty 25
