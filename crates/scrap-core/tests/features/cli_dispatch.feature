# Executable behavioral contract for top-level CLI dispatch (scrap-rs#21).
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
# Coverage: --help, --version, completions, unknown --format value —
# the four top-level dispatch behaviors that exercise the clap-derive
# surface end-to-end (parse → branch → emit) without requiring real
# source files. Per-format reporter behavior is unit-tested in
# `cli/dispatch.rs::tests`; per-flag clap parsing is unit-tested in
# `cli/mod.rs::tests`.

Feature: CLI top-level dispatch follows the contracted shape
  As a scrap user
  I want predictable behavior for --help, --version, completions, and
    parse errors
  So that I can script against the binary and trust the exit codes.

  Background:
    Given a fresh test World

  Scenario: --help exits 0 and emits the adapter's about text on stdout
    When the user runs `<tool> --help`
    Then the command exits with code 0
    And stdout contains the substring `Static test smell detector`

  Scenario: --version exits 0 and emits a version string on stdout
    When the user runs `<tool> --version`
    Then the command exits with code 0
    And stdout matches the pattern `^test-adapter \d+\.\d+\.\d+`

  Scenario: completions subcommand emits a non-empty completion script
    When the user runs `<tool> completions zsh`
    Then the command exits with code 0
    And stdout is non-empty

  Scenario: unknown --format value exits 2 with a clap parser error on stderr
    When the user runs `<tool> --format bogus`
    Then the command exits with code 2
    And stderr contains the substring `invalid value 'bogus' for '--format`
