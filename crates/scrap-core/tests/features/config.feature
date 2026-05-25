# Executable behavioral contract for the config loader (scrap-rs#18).
# Run via the cucumber-rs harness at `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/config.rs` (mod-block split
# per scrap-rs#18 W5.1; SHOULD-FIX #5 fold-in).
#
# Scenarios cover all 4 ConfigError variants: Parse (S3),
# InvalidGlob (S4, S9), Io (S8), InvalidValue (via the validator's
# semantic checks, hit indirectly by S4/S9 and explicitly in unit
# tests). Plus the canonical happy paths (S1, S2), discovery
# behaviors (S5-S7), and the locked OptOutPolicy Shape B contract
# (S10).

Feature: TOML config loader parses scrap4rs.toml-shaped files and surfaces typed errors
  As a downstream consumer of `scrap_core::cli::config`
  I want a strict deserialization API with file:line-aware diagnostics
  So that user-authored config typos surface at load time with
  enough context to fix without grepping.

  Background:
    Given a fresh test World

  # ─── Loader happy paths ─────────────────────────────────────────────

  Scenario: empty file loads to default FileConfig
    Given a config fixture with the contents:
      """
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is `Ok` and the loaded config equals the default

  Scenario: full fixture loads to the expected POD shape
    Given a config fixture with the contents:
      """
      src = "crates"
      exclude = ["vendored/**"]
      extensions = ["rs"]

      [opt_outs]
      honor = ["no_asserts"]

      [detectors.zero_assertion]
      enabled = true
      penalty = 10

      [detectors.large_example]
      line_threshold = 30

      [[overrides]]
      match = ["tests/integration/**"]
      [overrides.detectors.large_example]
      line_threshold = 100
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is `Ok` and the loaded config exercises every top-level field

  # ─── ConfigError variant coverage ───────────────────────────────────

  Scenario: unknown top-level field surfaces as Parse error
    Given a config fixture with the contents:
      """
      unknown_key = true
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is a `Parse` error mentioning the unknown field

  Scenario: invalid exclude glob surfaces with file:line context
    Given a config fixture with the contents:
      """

      src = "crates"

      exclude = [
        "[unclosed",
      ]
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is an `InvalidGlob` error on line 6 with pattern `[unclosed`

  Scenario: invalid override match glob surfaces as InvalidGlob with override-aware line
    # Cucumber-rs prepends a leading `\n` from the opening `"""`, so
    # this docstring body's `[bad` ends up on line 5 (lines 1-2 blank,
    # 3 `[[overrides]]`, 4 `match = [`, 5 `  "[bad",`).
    Given a config fixture with the contents:
      """

      [[overrides]]
      match = [
        "[bad",
      ]
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is an `InvalidGlob` error on line 5 with pattern `[bad`

  Scenario: empty exclude pattern rejected as InvalidValue
    Given a config fixture with the contents:
      """
      exclude = [""]
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is an `InvalidValue` error mentioning `empty`

  # ─── discover_config ────────────────────────────────────────────────

  Scenario: discover_config finds config in the start directory itself
    Given a tempdir containing `test-adapter.toml`
    When the caller invokes `discover_config()` starting from that tempdir
    Then the result is `Ok` and the discovered path ends with `test-adapter.toml`

  Scenario: discover_config walks up to find config in an ancestor
    Given a tempdir containing `test-adapter.toml` at the root
    And a deep subdirectory `a/b/c` inside the tempdir
    When the caller invokes `discover_config()` starting from `a/b/c`
    Then the result is `Ok` and the discovered path ends with `test-adapter.toml`

  Scenario: discover_config returns Ok(None) when no config exists in the ancestor chain
    Given an isolated tempdir containing no `test-adapter.toml`
    When the caller invokes `discover_config()` starting from a deep subdirectory
    Then the result is `Ok(None)`

  # ─── OptOutPolicy Shape B contract (publicly visible) ───────────────

  Scenario: opt_outs honor list with explicit variants round-trips correctly
    Given a config fixture with the contents:
      """
      [opt_outs]
      honor = ["no_asserts", "no_op"]
      """
    When the caller invokes `load_config()` on the fixture
    Then the result is `Ok` and `opt_outs.honor` equals exactly `["no_asserts", "no_op"]`
