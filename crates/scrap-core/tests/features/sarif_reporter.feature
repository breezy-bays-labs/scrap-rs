# Executable behavioral contract for the SARIF 2.1.0 reporter
# (scrap-rs#17). Run via the cucumber-rs harness at
# `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/sarif_reporter.rs`.
#
# Scenarios pin the cross-port BDD consistency rule
# (`feedback_bdd-cross-port-consistency`): every adapter-layer surface
# ships a thin `.feature` even when behavioral surface is thin. This is
# the SARIF sibling to json_reporter.feature / table_reporter.feature.
#
# Scenarios cover: SARIF 2.1.0 top-level shape, adapter-name threading
# (driver.name from AdapterMeta, never a literal), one-result-per-Smell
# granularity (scrap-rs#17 D2), severity→level mapping, and roundtrip
# parseability.

Feature: SARIF 2.1.0 reporter emits GitHub Code Scanning-ingestible JSON
  As a CI consumer uploading results to GitHub Code Scanning
  I want scrap findings as SARIF 2.1.0 with one result per smell
  So that each test smell renders inline on the PR with the right
  severity, and the tool identity comes from the adapter (not a
  hardcoded name).

  Background:
    Given a fresh test World

  Scenario: empty report produces a well-formed SARIF document
    When the caller invokes SARIF `emit()` with no findings
    Then the SARIF `version` equals `2.1.0`
    And the SARIF `$schema` is the sarif-2.1.0 schema URL
    And the SARIF run has zero results

  Scenario: driver name comes from the adapter meta, not a literal
    When the caller invokes SARIF `emit()` with no findings
    Then the SARIF `runs[0].tool.driver.name` equals the adapter tool name

  Scenario: rules are emitted for every smell category
    When the caller invokes SARIF `emit()` with no findings
    Then the SARIF run defines 5 rules

  Scenario: one SARIF result per smell, not per finding
    Given a SARIF fixture finding with 2 smells in `crates/foo/src/bar.rs`
    When the caller invokes SARIF `emit()` over the fixture findings
    Then the SARIF run has 2 results

  Scenario: severity maps to the SARIF level
    Given a SARIF fixture finding with a high-severity smell in `crates/foo/src/bar.rs`
    When the caller invokes SARIF `emit()` over the fixture findings
    Then the first SARIF result has `level` equal to `error`

  Scenario: emitted SARIF parses back into a SarifLog
    Given a SARIF fixture finding with 2 smells in `crates/foo/src/bar.rs`
    When the caller invokes SARIF `emit()` over the fixture findings
    Then the emitted SARIF round-trips into a `SarifLog`
