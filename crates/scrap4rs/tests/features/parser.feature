# Behavioral contract for the syn-based test parser
# (`scrap4rs::parser::SynTestParser`).
#
# Pin the public surface of `TestParserPort::parse_test_source` —
# what facts the parser MUST recover from each canonical input shape.
# These scenarios are deliberately thin; per-fixture exhaustive
# coverage lives in the insta snapshot battery
# (`crates/scrap4rs/tests/parser_snapshots.rs`) and the property tests
# (`crates/scrap4rs/tests/parser_props.rs`).
#
# Pattern mirrors `crates/scrap-core/tests/features/file_walker.feature`
# — every port adapter in scrap-rs ships a `.feature` public-contract
# surface for cross-port consistency. See feedback memory
# `feedback_bdd-cross-port-consistency.md`.
#
# Step matcher convention (per impl-plan Reusable Reference):
# every `When` step uses one of exactly two matchers:
#   - `When I parse the source:`  followed by a Gherkin docstring (inline source)
#   - `When I parse the fixture <path>`  (file-backed source)
# Step definitions that introduce a third `When` shape are a regression.

Feature: scrap4rs syn-based test parser — TestParserPort contract

  Background:
    Given a SynTestParser

  Scenario: Empty source compiles to an empty test inventory
    When I parse the source:
      """
      """
    Then parsing succeeds
    And the parsed file contains 0 tests
    And the parsed file contains 0 diagnostics

  Scenario: Malformed source returns a syntax error
    When I parse the source:
      """
      fn missing_brace() {
      """
    Then parsing fails with a ParseError::Syntax

  # Scenarios below are tagged `@wip` and skipped until the
  # implementing session lands their support. The tag is removed
  # scenario-by-scenario as each Wave 2 / 3 session unlocks it:
  #   @wave2-s2-1  → Wave 2 / S2.1 (top-level walker + attributes)
  #   @wave2-s2-2  → Wave 2 / S2.2 (explicit assertion macros)
  #   @wave2-s2-3  → Wave 2 / S2.3 (macro-form implicit sources)
  #   @wave2-s2-4  → Wave 2 / S2.4 (non-macro implicit + should_panic)
  # The cucumber harness filters `@wip` out (`not @wip`); CI logs
  # stay clean during Wave 2 instead of showing N failing scenarios.

  @wip @wave2-s2-1
  Scenario: A bare #[test] fn yields one parsed test with zero assertions
    When I parse the source:
      """
      #[test]
      fn it() {}
      """
    Then parsing succeeds
    And the parsed file contains 1 test
    And test "it" has 0 explicit assertions
    And test "it" has 0 implicit assertion sources
    And test "it" has the attribute "test"

  @wip @wave2-s2-1
  Scenario Outline: Test attributes are projected as parsed-attribute facts
    When I parse the source:
      """
      #[<attr>]
      #[test]
      fn it() {}
      """
    Then test "it" has the attribute "<attr>"

    Examples:
      | attr         |
      | tokio::test  |
      | rstest       |
      | should_panic |
      | ignore       |

  @wip @wave2-s2-1
  Scenario: Nested-mod tests get fully-qualified names
    When I parse the source:
      """
      mod auth {
          mod login_tests {
              #[test]
              fn it_logs_in() {}
          }
      }
      """
    Then parsing succeeds
    And the parsed file contains 1 test
    And test "auth::login_tests::it_logs_in" exists

  @wip @wave2-s2-2
  Scenario: Explicit assertion macros populate parsed-assertion entries
    When I parse the source:
      """
      #[test]
      fn it() {
          assert!(true);
          assert_eq!(1, 1);
      }
      """
    Then test "it" has 2 explicit assertions
    And test "it" assertion 0 has name "assert"
    And test "it" assertion 1 has name "assert_eq"
    And test "it" has 0 implicit assertion sources

  @wip @wave2-s2-3-and-s2-4
  Scenario Outline: Implicit-assertion sources are recognized from runner shells
    When I parse the fixture <fixture>
    Then test "it" has the implicit assertion source <variant>

    Examples:
      | fixture                                                  | variant          |
      | tests/fixtures/runner_shells/proptest_shell.rs           | Proptest         |
      | tests/fixtures/runner_shells/quickcheck_shell.rs         | Quickcheck       |
      | tests/fixtures/runner_shells/cucumber_shell.rs           | Cucumber         |
      | tests/fixtures/runner_shells/kani_shell.rs               | Kani             |
      | tests/fixtures/runner_shells/trybuild_shell.rs           | Trybuild         |
      | tests/fixtures/runner_shells/insta_shell.rs              | Insta            |
      | tests/fixtures/runner_shells/pretty_assertions_shell.rs  | PrettyAssertions |
      | tests/fixtures/runner_shells/should_panic_shell.rs       | ShouldPanic      |

  @wip @wave2-s2-1
  Scenario: Opt-out attributes are projected as test opt-outs
    When I parse the source:
      """
      #[test]
      #[allow(scrap::no_asserts)]
      fn it() {}
      """
    Then test "it" has the opt-out NoAsserts
    And test "it" has 1 opt-out

  @wip @wave2-s2-1
  Scenario: body_line_count reflects the inner-block line span
    When I parse the source:
      """
      #[test]
      fn it() {
          let x = 1;
          assert_eq!(x, 1);
      }
      """
    Then test "it" has body_line_count 3
