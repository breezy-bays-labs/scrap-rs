# BDD spec for `scrap-core::detectors::tautological_assertion::detect`.
#
# Per `feedback_bdd-cross-port-consistency`: ship a thin .feature even
# when surface is mechanical. Cross-port reading surface wins.
#
# Detector contract (locked at /shape gate, scrap-rs#24):
#   - Emits Some(Finding) unconditionally when one or more assertions
#     trip the tautology rule (`arguments_identical` OR
#     `single_arg_value == Some(Bool(true))`).
#   - Does NOT consult `opt_outs` (pipeline policy at scrap-rs#72 owns
#     suppression).
#   - Does NOT consult `implicit_assertion_sources` for `#[should_panic]`
#     suppression — same pipeline-side concern.
#   - assert!(false) is NOT flagged (Uncle Bob convention).
#
# Fixture-ingest scenarios (proptest body, `#[should_panic]` parser
# facts) live in `crates/scrap4rs/tests/features/parser.feature` —
# scrap-core stays AST-pure (no `scrap4rs` import in step impls).

Feature: Tautological-assertion detector
  As a contributor reviewing test code,
  I want assertions that cannot fail to be flagged,
  so that I notice tests that exist but assert nothing meaningful.

  Background:
    Given the tautological-assertion detector

  Scenario: A test with assert!(true) is flagged
    When a ParsedTest is built with one assertion whose single_arg_value is Bool(true)
    Then the detector emits a Finding with one TautologicalAssertion smell of penalty 10
    And the Finding's scrap_score is 10

  Scenario: A test with assert_eq!(x, x) is flagged
    When a ParsedTest is built with one assertion whose arguments_identical is true
    Then the detector emits a Finding with one TautologicalAssertion smell of penalty 10
    And the Finding's scrap_score is 10

  Scenario: Multiple tautological assertions aggregate to one Finding with multiple Smells
    When a ParsedTest is built with two assertions both with arguments_identical true
    Then the detector emits a Finding with two TautologicalAssertion smells
    And the Finding's scrap_score is 20

  Scenario: A test with assert!(false) is NOT flagged
    When a ParsedTest is built with one assertion whose single_arg_value is Bool(false)
    Then the detector emits no Finding

  Scenario: A test with a real comparison is NOT flagged
    When a ParsedTest is built with one assertion whose arguments_identical is false and single_arg_value is None
    Then the detector emits no Finding

  Scenario: A test with no assertions is NOT flagged
    When a ParsedTest is built with no assertions
    Then the detector emits no Finding
