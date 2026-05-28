# Executable behavioral contract for the `init` subcommand (scrap-rs#21).
# Run via the cucumber-rs harness at `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Step defs live in
# `crates/scrap-core/tests/cucumber_steps/cli.rs` (sibling to config.rs
# and json_reporter.rs).
#
# Adapter-name-pure: the `<tool>` token in step text is a placeholder
# the step-def resolver replaces with a programmatically-constructed
# `test-adapter`-named `AdapterMeta`. Zero `"scrap4rs"` literals in
# this file.
#
# Coverage: write-default, bail-without-force, overwrite-with-force,
# auto-detect-crates-layout, force-overrides-malformed-config — the
# five locked behaviors of `init` per the issue AC + cabinet MF-2
# fold (the malformed-config scenario pins that `init --force`
# recovers from a malformed config because main.rs dispatches the
# subcommand BEFORE bootstrap + FsWalker construction).

Feature: CLI init subcommand bootstraps a starter config TOML
  As a new scrap user
  I want to run one command to scaffold a working config
  So that I can edit a known-good template rather than write TOML
    from scratch and risk schema typos.

  Background:
    Given a fresh test World

  Scenario: init writes a default config when none exists
    Given a working directory with no existing `test-adapter.toml`
    When the user runs `<tool> init`
    Then the result is `Ok` and a file named `test-adapter.toml` exists in the directory
    And the file contents include the line `src = "src"`
    And the file contents include the line `# threshold_mode = "default"`
    And the file round-trips through `load_config()` without error

  Scenario: init bails when config already exists without --force
    Given a working directory containing an existing `test-adapter.toml` with `legacy = true`
    When the user runs `<tool> init`
    Then the result is an `InitError::Exists` referencing the existing path
    And the file contents are unchanged (`legacy = true` is preserved)

  Scenario: init overwrites the existing config when --force is passed
    Given a working directory containing an existing `test-adapter.toml` with `legacy = true`
    When the user runs `<tool> init --force`
    Then the result is `Ok` and the file is regenerated
    And the file contents no longer include `legacy = true`
    And the file contents include the line `src = "src"`

  Scenario: init detects the crates/ layout when src/ is absent
    Given a working directory containing a `crates/` directory but no `src/` directory
    When the user runs `<tool> init`
    Then the result is `Ok` and the file contents include the line `src = "crates"`

  Scenario: init --force succeeds even when existing config is malformed
    # Cabinet MF-2 fix — pins that subcommand dispatch happens BEFORE
    # bootstrap + FsWalker construction. Without the pre-dispatch,
    # `init --force` would fail with ConfigError::Parse → exit 2 — the
    # exact recovery path `--force` exists for.
    Given a working directory with an existing `test-adapter.toml` containing invalid TOML
    When the user runs `<tool> init --force`
    Then the result is `Ok` and the file is regenerated
    And the command exits with code 0
    And the file contents include the line `src = "src"`
