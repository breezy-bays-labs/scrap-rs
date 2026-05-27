# Executable behavioral contract for the JSON envelope reporter
# (scrap-rs#14). Run via the cucumber-rs harness at
# `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/json_reporter.rs`.
#
# Scenarios pin the cross-port BDD consistency rule
# (`feedback_bdd-cross-port-consistency`): every adapter-layer
# surface ships a thin `.feature` even when behavioral surface is
# thin. File walker (#13) and parser (#12) shipped one each; this
# is the third adapter.
#
# Scenarios cover: default emit, view-flag immunity on `result.*`,
# optional-block elision, `schema_version: 1`, cross-adapter
# identical envelope.

Feature: JSON envelope reporter emits the v0.1 schema_version envelope
  As a CI consumer or contributor running scrap4rs --format json
  I want a stable, versioned wire envelope wrapping the analysis result
  So that the truthful gate (`result.*`) is immune to display-flag
  reshaping (`view.*`) and additive features (`delta`/`diagnostics`)
  don't break my parser.

  Background:
    Given a fresh test World
    And a fixture Report with one finding scoring 10 in `crates/foo/src/bar.rs`

  Scenario: default invocation produces full envelope with truthful gate intact
    When the caller invokes `emit()` with default options
    Then the envelope wire shape contains the top-level keys schema_version, tool, tool_version, language, timestamp, threshold_mode, result, view
    And `result.passed` equals `false`
    And `view.shown` length equals 1

  Scenario: --top truncates view but result stays full-fidelity
    Given an additional finding scoring 4 in `crates/bar/src/baz.rs`
    When the caller invokes `emit()` with `top = 1`
    Then `view.shown` length equals 1
    And `view.truncated` is true
    And `view.eligible_count` equals 2
    And `result.files` total findings count equals 2

  Scenario: --only-failing filters view but result stays full-fidelity
    Given an additional zero-score finding in `crates/bar/src/baz.rs`
    When the caller invokes `emit()` with `only_failing = true`
    Then `view.shown` length equals 1
    And `view.eligible_count` equals 1
    And `result.files` total findings count equals 2

  Scenario: delta block absent when no baseline supplied
    When the caller invokes `emit()` with default options
    Then the envelope wire shape does NOT contain the top-level key `delta`

  Scenario: diagnostics block absent without verbose mode
    When the caller invokes `emit()` with default options
    Then the envelope wire shape does NOT contain the top-level key `diagnostics`

  Scenario: schema_version is exactly 1 on every emit
    When the caller invokes `emit()` with default options
    Then the envelope's `schema_version` equals the integer 1

  Scenario: cross-adapter identical envelope modulo tool/language strings
    When the caller invokes `emit()` with adapter meta tool=`scrap4rs` language=`rust`
    And the caller invokes `emit()` with adapter meta tool=`scrap4ts` language=`typescript`
    Then both envelopes are byte-identical except for the `tool` and `language` fields
