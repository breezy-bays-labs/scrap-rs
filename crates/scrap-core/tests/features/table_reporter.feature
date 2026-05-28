# Executable behavioral contract for the comfy-table terminal
# reporter (scrap-rs#16). Run via the cucumber-rs harness at
# `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/table_reporter.rs`.
#
# Scenarios pin the cross-port BDD consistency rule
# (`feedback_bdd-cross-port-consistency`): every adapter-layer
# surface ships a thin `.feature` even when behavioral surface is
# thin. File walker (#13), parser (#12), and json reporter (#14)
# shipped one each; this is the fourth adapter.
#
# Scenarios cover: default emit (per-Smell rows + footer), --top
# truncation, --only-failing filter, grouping switch (Smell ↔
# Finding), color on/off, PASSED footer, ThresholdMode::Strict
# echoed in footer, header AdapterMeta content. Last 2 added per
# cabinet CQO S1 fold-in 2026-05-27.

Feature: Table reporter renders the v0.1 human-facing terminal table
  As a CLI user or CI consumer running `scrap4rs --format table`
  I want a readable table per Finding/Smell with header + footer
  So that the gate verdict + per-row attribution is visible in the
  terminal without parsing JSON.

  Background:
    Given a fresh test World
    And a fixture Report for the table reporter with one finding scoring 10 in `crates/foo/src/bar.rs`

  Scenario: default invocation produces per-Smell rows with footer
    When the caller invokes table `emit()` with default options
    Then the table output contains the column header `file:line`
    And the table output contains the column header `Penalty`
    And the footer line contains `FAILED`
    And the footer line contains `'default' mode`

  Scenario: --top truncates rows
    Given an additional table-fixture finding scoring 5 in `crates/foo/src/baz.rs`
    And an additional table-fixture finding scoring 7 in `crates/foo/src/qux.rs`
    When the caller invokes table `emit()` with `top = 2`
    Then the table output contains exactly 2 data rows under the `file:line` header

  Scenario: --only-failing drops zero-score findings
    Given an additional table-fixture zero-score finding in `crates/foo/src/baz.rs`
    When the caller invokes table `emit()` with `only_failing = true`
    Then the table output contains exactly 1 data row under the `file:line` header

  Scenario: grouping = Finding switches to per-Finding rows
    When the caller invokes table `emit()` with `grouping = finding`
    Then the table output contains the column header `Test`
    And the table output contains the column header `Smells`
    And the table output contains the column header `Pass/Fail`
    And the table output does NOT contain the column header `file:line`

  Scenario: use_color = false produces ANSI-free output
    When the caller invokes table `emit()` with `use_color = false`
    Then the table output does NOT contain ANSI escape sequences

  Scenario: use_color = true produces ANSI-colored output
    When the caller invokes table `emit()` with `use_color = true`
    Then the table output contains an ANSI escape sequence

  Scenario: PASSED footer when report.passed = true
    Given the table-fixture Report has `passed = true` and zero findings exceeding threshold
    When the caller invokes table `emit()` with default options
    Then the footer line contains `PASSED`

  Scenario: Footer reflects ThresholdMode::Strict
    When the caller invokes table `emit()` with default options and threshold mode `strict`
    Then the footer line contains `'strict' mode`

  Scenario: Header contains AdapterMeta tool and tool_version
    When the caller invokes table `emit()` with adapter meta tool=`test-adapter` and tool_version=`0.1.0`
    Then the header line contains `test-adapter`
    And the header line contains `0.1.0`
    And the header line contains `1 tests inspected`
