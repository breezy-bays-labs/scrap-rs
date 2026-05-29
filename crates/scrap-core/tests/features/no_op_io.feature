# BDD spec for `scrap-core::detectors::no_op_io::detect`.
#
# Per `feedback_bdd-cross-port-consistency`: ship a thin .feature even
# when the surface is mechanical. Cross-port reading surface wins.
#
# Detector contract (locked at scrap-rs#25 cabinet):
#   - Emits Some(Finding) when the test carries >=1 ResultDiscarded fact
#     AND no positive check (no assertion, no implicit source, no
#     ResultAsserted). Penalty 8, severity Moderate.
#   - Does NOT consult `opt_outs` (pipeline policy at scrap-rs#72 owns
#     suppression).
#   - Strict subset of zero-assertion — both co-fire and STACK (Option A;
#     precedence policy deferred to scrap-rs#32).
#
# Fixture-ingest scenarios (`let _ = ...;` source → ResultDiscarded;
# `let _: () = ...;` FP guard) live in
# `crates/scrap4rs/tests/features/parser.feature` — scrap-core stays
# AST-pure (no `scrap4rs` import in step impls).

Feature: No-op-io detector
  As a contributor reviewing test code,
  I want tests whose I/O results are discarded without any check to be
  flagged, so that I notice tests that run but verify nothing.

  Background:
    Given the no-op-io detector

  Scenario: A test that discards a Result with no check is flagged
    When a ParsedTest is built with one ResultDiscarded fact of kind Call
    Then the detector emits a Finding with one NoOpIo smell of penalty 8
    And the no-op-io Finding's scrap_score is 8

  Scenario: A discard alongside a real assertion is NOT flagged
    When a ParsedTest is built with one ResultDiscarded fact and one real assertion
    Then the no-op-io detector emits no Finding

  Scenario: A discard alongside a ResultAsserted fact is NOT flagged
    When a ParsedTest is built with one ResultDiscarded fact and a ResultAsserted fact
    Then the no-op-io detector emits no Finding

  Scenario: A test with no ResultDiscarded fact is NOT flagged
    When a ParsedTest is built with no behavioral facts
    Then the no-op-io detector emits no Finding
