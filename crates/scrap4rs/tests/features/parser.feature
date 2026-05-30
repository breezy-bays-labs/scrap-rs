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

  # Historical note: scrap-rs#12 used `@wip` tags during incremental
  # landing; final state has zero `@wip`-tagged scenarios. The
  # cucumber harness retains the `not @wip` filter as future-facing
  # scaffolding for any later incremental-landing PR.

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

  Scenario: Opt-out attributes are projected as test opt-outs
    When I parse the source:
      """
      #[test]
      #[allow(scrap::no_asserts)]
      fn it() {}
      """
    Then test "it" has the opt-out NoAsserts
    And test "it" has 1 opt-out

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

  # ─── scrap-rs#24 tautological-assertion structural-fact scenarios ───
  #
  # The tautological-assertion DETECTOR contract (filter+aggregate of
  # the two structural facts) is covered by
  # `crates/scrap-core/tests/features/tautological_assertion.feature`.
  # The two scenarios below cover the PARSER's structural-fact
  # extraction on real `.rs` fixtures — they exercise the syn-side
  # `extract_tautology_facts` helper through the SynTestParser surface,
  # which scrap-core can't do without importing scrap4rs.

  Scenario: assert!(true) inside proptest! is NOT visible to the parser (no-recurse boundary)
    When I parse the fixture tests/fixtures/runner_shells/tautological_inside_proptest.rs
    Then test "it" has 0 explicit assertions
    And test "it" has the implicit assertion source Proptest

  Scenario: #[should_panic] fn with assert!(true) projects single_arg_value Bool(true)
    When I parse the fixture tests/fixtures/true_positives/tautological_with_should_panic.rs
    Then test "assert_true_with_should_panic_is_still_tautological" has 1 explicit assertion
    And test "assert_true_with_should_panic_is_still_tautological" assertion 0 has name "assert"
    And test "assert_true_with_should_panic_is_still_tautological" assertion 0 has single_arg_value Bool(true)
    And test "assert_true_with_should_panic_is_still_tautological" assertion 0 has arguments_identical false
    And test "assert_true_with_should_panic_is_still_tautological" has the implicit assertion source ShouldPanic

  # ─── no-op-io ResultDiscarded projection (scrap-rs#25) ───
  #
  # The no-op-io DETECTOR contract lives in
  # `crates/scrap-core/tests/features/no_op_io.feature`. The scenarios
  # below cover the PARSER's structural-fact extraction of the
  # `let _ = ...;` discard shape on real `.rs` fixtures, including the
  # `let _: () = ...;` false-positive guard — neither of which scrap-core
  # can exercise without importing scrap4rs.

  # The fixture body is `let _ = std::fs::write("...", b"data");`. As of
  # scrap-rs#26 the `std::fs::write(...)` call ALSO projects a located
  # `FilesystemWrite` fact (the first half of the surface-only-io
  # correlation), so the body now carries TWO behavioral facts: the
  # `ResultDiscarded { Call }` discard shape this scenario asserts, AND
  # the `FilesystemWrite { Write }`. The scenario's contract is the
  # presence of the `ResultDiscarded` fact (second `Then` line).
  Scenario: A bare `let _ = call();` discard projects a ResultDiscarded fact
    When I parse the fixture tests/fixtures/true_positives/no_op_io.rs
    Then test "writes_a_file_but_checks_nothing" has 2 behavioral facts
    And test "writes_a_file_but_checks_nothing" has the ResultDiscarded fact of kind Call

  Scenario: A type-ascribed `let _: () = call();` does NOT project a discard (FP guard)
    When I parse the fixture tests/fixtures/runner_shells/no_op_io_unit_binding.rs
    Then test "intentional_unit_binding_does_not_smell" has 0 behavioral facts
    And test "intentional_unit_binding_does_not_smell" has the implicit assertion source ShouldPanic
