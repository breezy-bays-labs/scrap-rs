# Changelog

All notable changes to this project will be documented in this file. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

scrap-rs follows a deliberate **no-public-release** policy through v0.x —
mokumo is the sole consumer; tags exist for git pinning only. The first
crates.io publish + GitHub Release lands at **v1.0**, gated on the
triple-crate workspace (`scrap-core` + `scrap4rs` + `scrap4ts`) being
live. See `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`
§3 for the full release roadmap.

## [Unreleased]

### Added

- `Smell.span: Option<Span>` — additive per-Smell line attribution
  inside a `Finding`. Detectors emit `Some(span)` when they have
  per-assertion location (e.g. a `tautological-assertion` smell can
  point at the exact `assert_eq!(x, x)` line); `None` when the smell
  is whole-test (e.g. `zero-assertion`, where the whole body is the
  evidence). The enclosing `Finding::test.span` always covers the
  whole test body; `Smell::span` is strictly narrower or absent.
  Both constructors `Smell::new` and `Smell::with_message` accept
  the new `span: Option<Span>` parameter additively (slot order
  mirrors `ParseDiagnostic::new(kind, span, message)` in
  `domain/parsed.rs` — same crate, same layer, established
  precedent). Wire-shape uses
  `#[serde(skip_serializing_if = "Option::is_none")]` so existing
  emitters/parsers stay byte-compatible — no `schema_version` bump
  per ADR `adr-nested-json-envelope.md` D2 forward-compat rules;
  insta snapshot at `wire_envelope_snapshot__report_v01.snap` is
  byte-identical. Unblocks scrap-rs#30 (zero-assertion),
  scrap-rs#24 (tautological), scrap-rs#25, scrap-rs#26, scrap-rs#31
  — all consume the new constructor signature.
  Closes scrap-rs#76.
- CI: `scorecard` job in `.github/workflows/ci.yml` adopts crap-rs's
  templated `scorecard` composite action
  (`breezy-bays-labs/crap-rs/.github/actions/scorecard@726b805` — the
  CAPSTONE quick-start commit). Production-code CRAP analysis with
  sticky PR comment + HTML report artifact via a single `uses:`
  invocation; no `cargo binstall` plumbing on the scrap-rs side (the
  action handles install). Composite actions are git-ref consumed, so
  no release-tarball dependency. Inputs follow the templated example's
  preset surface: `threshold-preset: default` (cognitive=15),
  `run-mode: full` (no baseline), `gate-mode: gate-on-analysis` (hard
  gate), `comment-mode: sticky`, `html-report: true`, `languages: rust`.
  Test-file excludes carried via `crap4rs.toml` at repo root (the
  action's `config:` input passes `--config <path>` through to the
  underlying binary; per-invocation `--exclude` flags aren't on the
  action's input surface). Symmetric to the existing test-smell
  detection: scrap-rs measures *test* code for smells (V1) via its
  own detectors; crap-rs measures *production* code for complexity
  (V2). Closes scrap-rs#20.
- `crap4rs.toml` at repo root — pins `preset = "default"` (= 15
  cognitive) and `exclude = ["**/tests/**"]`. Consumed by the new
  `scorecard` CI job via the composite action's `config:` input.
- CI: `zizmor` workflow-security audit job in `.github/workflows/ci.yml`
  (mirrors crap4rs precedent). Runs `pipx run 'zizmor>=1.5,<2'
  .github/` on every PR + push to main. `unpinned-uses` is the
  primary gate; `excessive-permissions`, `template-injection`,
  `artipacked`, `cache-poisoning` ride along as secondary gates at
  no marginal cost. Major-version pinned (`<2`) so a zizmor release
  tightening rules doesn't flip CI red between PRs.
- CI: `.github/dependabot.yml` covering both `github-actions` and
  `cargo` ecosystems. Weekly cadence (Mondays), grouped minor+patch
  bumps per ecosystem, major bumps as separate PRs, 7-day cooldown
  to surface bad-release reports before automation pulls them.
  Commit prefix `ci(deps):` for github-actions, `chore(deps):` for
  cargo (per the workspace's commit convention).
- CI: workflow-level `permissions: contents: read` default in
  `ci.yml` (least-privilege). All 9 jobs are read-only; no per-job
  overrides needed today. Closes the `excessive-permissions` zizmor
  audit.
- CI: `persist-credentials: false` on every `actions/checkout` call
  (9 sites — 8 pre-existing jobs + 1 in the new zizmor job). Keeps
  the GH App checkout token out of the runner's `.git/config`.
  Closes the `artipacked` zizmor audit.
- CI: every external `uses:` ref in `ci.yml` SHA-pinned with trailing
  `# vX` (tagged releases: `actions/checkout`, `Swatinem/rust-cache`,
  `taiki-e/install-action`, `actions/upload-artifact`) or
  `# tracks @<branch> branch` (branch-pinned:
  `dtolnay/rust-toolchain@{stable,1.88}` — Dependabot can't bump
  these per AGENTS.md "Supply-chain hygiene" Rule 2; manual quarterly
  refresh cadence). `EmbarkStudios/cargo-deny-action` was already
  pinned.
- Docs: `AGENTS.md` adds `## Supply-chain hygiene` section (8 rules;
  mirrors crap4rs Rules 1–8). Rules 9 (cache-poisoning fix for
  release workflows) and 10 (gh CLI for release artifacts) deferred
  to v1.0 release-workflow work per `CLAUDE.md > v0.x → v1.0
  Transition`. Follow-up: scrap-rs#51 (extract `setup-rust` composite
  action — mirror crap4rs pattern).
- `domain::opt_outs::OptOut` (scrap-rs#12) — `#[non_exhaustive]`
  enum carrying the v0.1 per-test detector-suppression markers
  (`NoAsserts`, `Tautology`, `NoOp`), projected from
  `#[allow(scrap::*)]` attributes on the test fn. Derives `Ord` so
  `ParsedTest::opt_outs` is a `BTreeSet<OptOut>` (deterministic
  serialization order).
- `domain::assertion_sources::AssertionSource` (scrap-rs#12 — folds
  in scrap-rs#4) — `#[non_exhaustive]` enum naming the implicit-
  assertion sources the parser recognises (`Proptest`, `Quickcheck`,
  `Kani`, `Cucumber`, `Trybuild`, `Insta`, `PrettyAssertions`,
  `ShouldPanic`). The detectors (lands at scrap-rs#19/#30) read
  `ParsedTest::implicit_assertion_sources` and skip emission when
  non-empty — critical false-positive guard for mokumo CI integration.
- `domain::assertion_sources::recognise(&str) -> Option<AssertionSource>` —
  pure string-keyed lookup with first-match-wins precedence
  (exact-key → prefix → suffix). The parser composes the path string
  at the adapter boundary and passes a `&str`; the function has zero
  AST-library dependencies and stays inside `scrap-core`'s
  ast-purity envelope.
- `ParsedTest` gained `implicit_assertion_sources: Vec<AssertionSource>`
  and `opt_outs: BTreeSet<OptOut>` fields (scrap-rs#12). The canonical
  `ParsedTest::new(...)` constructor extends additively from 4 → 6
  args; D10 / Semantic Facts pattern.
- `ParsedAssertion` gained `raw_args: Option<String>` field (scrap-rs#12)
  carrying the verbatim macro-arg text for detector-side tautology
  classification. Conditional wire-key via
  `#[serde(skip_serializing_if = "Option::is_none")]` per envelope rules.
- New `span-purity` CI job — structural enforcement of the v0.1
  "no `Span` columns" decision; rejects `start_col` / `end_col` /
  `start_column` / `end_column` / `column:` fields in
  `crates/scrap-core/src/domain/types.rs`. Mirrors the `ast-purity`
  shape; the column-deferral exclusion is tracked by scrap-rs#17
  (SARIF reporter — the column-aware consumer).
- Property tests at `crates/scrap4rs/tests/parser_props.rs`
  (scrap-rs#12) — three invariants pinned over a hand-rolled
  `valid_test_source_strategy` (256 cases — pinned via
  `ProptestConfig::with_cases(256)` so future proptest default drift
  cannot silently weaken the invariant strength):
  - `proptest_no_panic_on_parse_file_able_source` — for any strategy
    output, the parser returns `Ok(_)` and never panics.
  - `proptest_idempotent_reparse` — parsing the same source twice
    yields equal `ParsedTestFile` values. Pins determinism for
    snapshot tests, baseline diffs (v0.4+), and reproducible CI.
  - `proptest_span_monotonicity` — every `ParsedTest::identity.span`
    and every `ParsedAssertion::span` obeys `start_line <= end_line`.
    Catches the A9 bug class where span construction could trip the
    `Span::new` debug_assert.
- 2 S3.1 error-recovery fixtures + 2 snapshots:
  `tests/fixtures/error_recovery/unclosed_brace.rs` and
  `tests/fixtures/error_recovery/malformed_attribute.rs`. Both
  surface `ParseError::Syntax { message, span: Some(_) }`;
  `insta::assert_debug_snapshot!` pins the Err shape (the
  `Serialize` derive lives in domain wire types, not on
  `ParseError`). The third planned synthetic-span fixture
  (`start_line == 0` sentinel) is covered via a unit test in
  `parser/mod.rs::tests::parse_error_from_syn_error_call_site_emits_localised_span`
  because proc-macro2 under `span-locations` doesn't emit
  `start_line == 0` from real source — the defensive branch in
  `parse_error_from_syn_error` is forward-compat guard, exercised
  by the unit test rather than a fixture.
- Coverage gate via `./scripts/coverage-parser.sh` — 98.36% workspace,
  97-100% on `crates/scrap4rs/src/parser/` (well over the 85% gate).
- `BodyVisitor::visit_expr_await` (scrap-rs#12 S2.4) — recognises the
  cucumber `.await` chain. `.await` desugars to `syn::Expr::Await`
  (NOT a method call, despite syntactic appearance); the override
  walks `ExprAwait::base` for the two canonical shapes
  (`World::cucumber().run(...).await` and
  `cucumber::Cucumber::run(...).await`) via `is_cucumber_chain` and
  fabricates the synthetic `"cucumber::run"` key for recognise().
  DOES recurse via `visit::visit_expr_await` (unlike `visit_macro`'s
  v0.1 no-recurse boundary) — `.await` chains can nest and
  recursion catches every cucumber chain in the body.
- `BodyVisitor::visit_expr_call` (scrap-rs#12 S2.4) — recognises
  function-call implicit sources (e.g. `quickcheck::quickcheck(prop)`,
  `trybuild::TestCases::new()`). For `Expr::Call` whose `.func` is
  `Expr::Path`, hand-rolls the path string via the existing
  `compose_macro_path_string` and passes it through recognise().
  DOES recurse via `visit::visit_expr_call`.
- `is_cucumber_chain` (scrap-rs#12 S2.4) — predicate that walks an
  `&Expr` (an `.await` receiver) looking for `World::cucumber()`
  method-call shape or any `cucumber::*` path-call shape. Pinned by
  4 unit tests (2 positive, 2 negative).
- `attributes::implicit_sources_from_attributes` (scrap-rs#12 S2.4 /
  N24) — attribute-channel implicit-source recognition. At v0.1, the
  only attribute-sourced variant is `AssertionSource::ShouldPanic`
  (from `#[should_panic]`); the function is shaped for additive
  v0.3+ extension. Called by `extract_parsed_test` alongside
  `extract_attributes`/`extract_opt_outs`; result merges into the
  body-walker's implicit-source vec before `ParsedTest::new`.
- 4 S2.4 fixtures + 4 snapshots under
  `crates/scrap4rs/tests/fixtures/runner_shells/`:
  `quickcheck_shell.rs` (→ `Quickcheck` via fn-call), `cucumber_shell.rs`
  (→ `Cucumber` via .await chain), `trybuild_shell.rs` (→ `Trybuild`
  via fn-call), `should_panic_shell.rs` (→ `ShouldPanic` via N24
  attribute path). S2.1's `attribute_variants.rs` snapshot
  regenerated as expected (its `should_panic_test` fn now correctly
  shows `implicit_assertion_sources: [should_panic]`).
- `.github/workflows/ci.yml` — extends both the `test` job (line 79
  area) and the `coverage` job (lines 116-120 area) with explicit
  `cargo test -p scrap4rs --test cucumber --locked` invocations
  (deferred from S1.1 per item 11 Option A; now landing because all
  parser.feature scenarios are green — CI logs stay clean). Per the
  Reusable Reference workflow-edit verification rule, both edits
  verified via `git diff` before commit.
- All 9 implicit-source Scenario Outline rows pass — `@wip` tag
  removed from the outline scenario. scrap4rs cucumber: 19/19
  scenarios pass (was 11/11 at S2.3). Walking-skeleton parser fully
  recognises every v0.1 AssertionSource variant.
- `BodyVisitor::implicit_assertion_sources` (scrap-rs#12 S2.3) — macro-form
  implicit-assertion source recognition via the
  `scrap_core::domain::assertion_sources::recognise()` contract. The
  `visit_macro` override now runs BOTH the explicit-assertion match
  AND the recognise() lookup; `pretty_assertions::assert_eq` is the
  dual-recognition case (one `ParsedAssertion("assert_eq")` AND one
  `AssertionSource::PrettyAssertions`). v0.1 boundary preserved: no
  token-stream recursion (per plan revision item 22).
- 5 S2.3 fixtures + 5 snapshots under
  `crates/scrap4rs/tests/fixtures/runner_shells/`:
  `proptest_shell.rs` (→ `Proptest`), `kani_shell.rs` (→ `Kani`),
  `insta_shell.rs` (→ `Insta`), `pretty_assertions_shell.rs`
  (→ `PrettyAssertions` + dual `ParsedAssertion`),
  `proptest_macro_suffix.rs` (→ `Proptest` via `*_proptest` suffix
  rule). All prior snapshots reviewed; none regenerated.
- Cucumber `When I parse the fixture <path>` matcher landed —
  deferred from S1.1 per the Reusable Reference convention; now
  active with the S2.3 fixture corpus. New
  `has the implicit assertion source X` Then matcher decodes
  `AssertionSource` variant names. The implicit-source Scenario
  Outline remains `@wip` until S2.4 lands the remaining 3 variants
  (Quickcheck / Cucumber / Trybuild / ShouldPanic).
- `scrap4rs::parser::body::BodyVisitor` (scrap-rs#12 S2.2) — per-test
  body walker via `syn::visit::Visit`. S2.2 ships `visit_macro` with
  the explicit-assertion-macro side: leaf-segment match against the
  v0.1 set (`assert` / `assert_eq` / `assert_ne` / `assert_matches` /
  `panic` / `unimplemented` / `todo`) → `ParsedAssertion::new(leaf,
  raw_args, span)`. Hand-rolled path stringification via
  `compose_macro_path_string` avoids the whitespace injection from
  `TokenStream::to_string()` (load-bearing for the S2.3 `recognise()`
  exact-string lookups). v0.1 boundary: NO `visit::visit_macro`
  recursion into token streams (per scrap-rs#12 plan revision item 22).
- `scrap4rs::parser::assertions::compose_macro_path_string` —
  hand-rolled `iter().map().join("::")` for `syn::Path` →
  whitespace-free key string. Critical: NOT `quote!(#path).to_string()`
  (which injects spaces around `::` and breaks recognise()'s exact
  match).
- `scrap4rs::parser::extract_parsed_test` now drives the BodyVisitor
  for each `#[test]` fn; `ParsedTest::assertions` populates with the
  recovered macros. The TODO(S2.2) stub from S2.1 is removed.
- 2 S2.2 fixtures: `tests/fixtures/true_positives/{zero_assertion,
  tautological}.rs`. The first has no assertions (parser-side baseline
  for the future zero-assertion detector at #30); the second has 2
  tautological assertions (`assert!(true)`, `assert_eq!(1, 1)`) for
  the future tautology detector at #24.
- 2 new insta snapshots (`snapshot_zero_assertion`, `snapshot_tautological`)
  accepted via `cargo insta review` per the Reusable Reference snapshot
  discipline. S2.1 snapshots reviewed for regenerations — none
  surfaced, confirming the body-walker change affects only fixtures
  with body-level assertions.
- 3 new cucumber step impls: `has N explicit assertions`,
  `assertion N has name "X"`, `has N implicit assertion sources`.
  Unlocks 2 more scenarios: "A bare #[test] fn yields one parsed
  test" (needed S2.2's count step) and "Explicit assertion macros
  populate parsed-assertion entries" (the S2.2 target). scrap4rs
  cucumber: 11/11 scenarios pass (was 9/9 at S2.1).
- `scrap4rs::parser` top-level walker (scrap-rs#12 S2.1) — wires
  `visit_item_mod` (path-stack push/recurse/pop) and `visit_item_fn`
  (is_test_fn check + extract_parsed_test orchestration); recovers
  module-qualified test names like `auth::login_tests::it_logs_in`.
  Body inspection (assertions + implicit sources) stays stubbed via
  `TODO(S2.2)` markers; lights up incrementally across S2.2 → S2.4.
- `scrap4rs::parser::attributes` module — `is_test_fn`,
  `extract_attributes` (with leaf-segment v0.1 whitelist:
  `test`/`rstest`/`should_panic`/`ignore`), `parsed_attribute_from_syn`
  (leaf-segment naming + `Meta::List`/`NameValue` raw-text projection
  via `quote::ToTokens`), `extract_opt_outs` + `match_opt_out_key`
  (the `#[allow(scrap::*)]` → `BTreeSet<OptOut>` projection).
- `scrap4rs::parser::spans::compute_body_line_count` — formula
  `close.line - open.line` (matches the documented v0.1 semantic;
  pinned in S2.1's docstring to discard the misleading "N-1 for
  N-line bodies" phrasing from the breadboard draft).
- `crates/scrap4rs/tests/fixtures/` — three S2.1 fixtures: `nested_mods.rs`
  (depth-2 mod nesting), `attribute_variants.rs` (all 7 v0.1 attribute
  variants), `opt_outs/allows.rs` (all 3 OptOut variants + multi-key
  allow + non-scrap allow exclusion).
- `crates/scrap4rs/tests/parser_snapshots.rs` — insta snapshot harness;
  3 per-fixture YAML snapshots committed under
  `crates/scrap4rs/tests/snapshots/`. First-creation seeded via
  `INSTA_UPDATE=auto` per the Reusable Reference snapshot discipline;
  Wave 2 sessions S2.2-S2.4 use `cargo insta review` interactively.
- `scrap4rs` workspace dep: `quote = "1"` — provides the `ToTokens`
  trait needed by `parsed_attribute_from_syn` to project
  `Meta::NameValue` expressions to verbatim source-byte strings. The
  `quote!(...)` macro is NOT used; path stringification stays
  hand-rolled. (S1.1 plan-revision item 7 was over-aggressive about
  dropping this; restored here in S2.1.)
- `workspace` dep update: `insta` features extended to include `yaml`
  (was `json` only) — `parser_snapshots.rs` uses YAML snapshots for
  compact, diff-friendly per-field output.
- `scrap4rs::parser` module (scrap-rs#12 S1.1) — syn-based parser
  skeleton implementing `TestParserPort`. `SynTestParser::new()`
  zero-sized adapter; `parse_test_source` opens
  `syn::parse_file`, drives an empty `TestVisitor<'ast>` (Wave 2
  fills in the overrides), drains to `ParsedTestFile`. Two failure
  paths: malformed source returns `ParseError::Syntax` with a
  localised `Span` when proc-macro2 surfaces one; otherwise
  `span: None`. All `syn` types confined to
  `crates/scrap4rs/src/parser/` per adr-hexagonal-layout.
- `scrap4rs` workspace deps: `syn = "2"` (features `parsing`,
  `full`, `visit`, `extra-traits`), `proc-macro2 = "1"` (feature
  `span-locations` — REQUIRED; without it `Span::start()` /
  `Span::end()` return line 0 for every node).
- `crates/scrap4rs/tests/cucumber.rs` — cucumber-rs 0.23 harness
  mirroring the file-walker pattern (`harness = false`, async
  tokio current-thread runtime, `World::cucumber()
  .filter_run_and_exit("tests/features", ...)`). Two `When`
  matchers per the impl-plan Reusable Reference:
  `I parse the source: <docstring>` (S1.1) and
  `I parse the fixture <path>` (S2.3 — added when fixtures land).
  S1.1 supports scenarios 1-2 (empty source / malformed source);
  scenarios 3-9 are tagged `@wip` and skipped until each Wave 2
  session removes the tag.
- `crates/scrap4rs/tests/parser_surface.rs` — compile-time
  invariant smoke tests via `static_assertions`: `TestParserPort`
  is obj-safe, `dyn TestParserPort` is NOT `Send + Sync`,
  `SynTestParser` IS `Send + Sync` (Send/Sync symmetry mirroring
  `crates/scrap-core/tests/source_walker.rs:21-24`).
- `scripts/coverage-parser.sh` — single execution path for
  coverage. Chains `cargo llvm-cov clean` →
  `--no-report nextest --workspace -E 'not binary(cucumber)'` →
  `--no-report test -p scrap-core --test cucumber` →
  `--no-report test -p scrap4rs --test cucumber` →
  `report --fail-under-lines 85`. Mirrors CI step-for-step;
  future detector PRs inherit the pattern.
- `domain::source` module — POD types for source discovery:
  `DiscoveryOutcome` (files + non-fatal mid-walk diagnostics),
  `SourceDiagnostic` (path + kind + message), `SourceDiagnosticKind`
  (`#[non_exhaustive]` enum: `permission_denied` / `midwalk_io` /
  `other`).
- `domain::config::AnalysisConfig` — caller-supplied workspace
  configuration: `src: SourceRoot`, `exclude: Vec<String>`,
  `extensions: Vec<String>`, `respect_gitignore: bool`. Infallible
  `::new`; glob validation lives in the adapter (per shaping Shape A
  — adapter-owns-validation).
- `adapters::source::fs::FsWalker` — disk-backed `SourcePort`
  implementation built on `ignore::WalkBuilder` + `globset`. Eagerly
  pre-validates user globs at `try_new` (fatal `SourceError::InvalidGlob`
  for malformed globs, `SourceError::EmptyExcludePattern` for
  empty/whitespace-only globs that `globset` would silently accept and
  `OverrideBuilder` would rewrite into a global whitelist); honours
  `.gitignore` / `.ignore` / `.git/info/exclude` when
  `respect_gitignore = true` (`require_git(false)` so VCS files are
  honoured outside a git repo, matching `rg`/`fd` ergonomics);
  mid-walk failures surface as `SourceDiagnostic` so the walk
  continues; symlinks emit a `SourceDiagnosticKind::Other` diagnostic
  rather than being silently dropped. Emits paths **relative to
  `AnalysisConfig::src`** so reports and snapshots are stable across
  machines. Sorts the collected paths byte-wise on the underlying
  OsStr (E1 from shaping — matches the
  `crap4rs::core::discover_rust_files` reference).
- `adapters::source::memory::MemorySource` — test-only `SourcePort`
  implementation that returns a fixed `(files, diagnostics)` pair
  without touching disk. Fields are private; access via
  `::files()` / `::diagnostics()` accessors. Two constructors:
  `::new(files, diagnostics)` (canonical, D10) and
  `::with_files(files)` (convenience for the diagnostics-empty test
  fixture path).
- Executable behavioral contract for the file walker at
  `crates/scrap-core/tests/features/file_walker.feature`
  (15 scenarios + 1 Scenario Outline w/ 2 Examples — 17 scenario rows
  total), driven by a cucumber-rs 0.23 harness at
  `crates/scrap-core/tests/cucumber.rs` (`harness = false`).
- Compile-time invariant smoke tests at
  `crates/scrap-core/tests/source_walker.rs` — `assert_obj_safe!`
  on `SourcePort`, `assert_not_impl_any!(dyn SourcePort: Send, Sync)`
  on the trait, `assert_impl_all!(_: Send, Sync)` on both shipped
  adapters.
- Hexagonal port traits in `scrap-core`: `SourcePort` (test-file
  discovery) and `TestParserPort` (source → `ParsedTestFile`), each with
  a `thiserror`-derived `#[non_exhaustive]` error enum (`SourceError`,
  `ParseError`). Object-safe (`Box<dyn ...>` works); `Send + Sync` is
  deliberately absent so parallelism bounds add at the `core::analyze`
  call site.
- `domain::parsed` module — language-agnostic structural facts every
  detector consumes: `ParsedTestFile`, `ParsedTest`, `ParsedAttribute`,
  `ParsedAssertion`, `ParseDiagnostic`, `ParseDiagnosticKind`. POD types
  for FFI portability; canonical `::new()` constructors are the
  documented entry point so detector follow-up PRs can add typed
  semantic-fact fields additively.
- `domain::SourceRoot` newtype — type-level boundary marker for the
  CLI/test entry into source discovery.
- `Display` impls on `FilePath`, `SourceRoot`, `QualifiedName` so
  operator-facing error and log strings render the wrapped path/name
  cleanly.
- Initial workspace bootstrap: `crates/scrap4rs` skeleton with hexagonal
  module layout (`domain/`, `ports/`, `adapters/`, `core/`, `cli/`).
- CI workflow with format / clippy / test matrix (Linux + macOS arm64 +
  macOS x86_64) / coverage jobs.
- Repo chrome: README, AGENTS.md, CLAUDE.md, CONTRIBUTING.md, dual
  MIT/Apache-2.0 license, default scrap4rs.toml stub.

### Changed

- `FsWalker::discover_test_files` (`crates/scrap-core/src/adapters/source/fs.rs`)
  decomposed into a `Decision` enum + `classify_entry`, `preflight_root`,
  and `FsWalker::build_walker` helpers (cognitive complexity 30 → 8 on
  the method; new helpers all ≤8). Pure refactor — no behaviour change;
  the existing 16-test unit suite + 217-line `file_walker.feature` BDD
  harness exercise every branch end-to-end. Unblocked the new
  `crap-self` gate at the calibrated cognitive-default threshold 15.
  Refs scrap-rs#20.
- Workspace lint policy lifted from per-crate
  `#![warn(clippy::pedantic, clippy::cargo)]` headers in
  `crates/{scrap-core,scrap4rs}/src/lib.rs` to
  `[workspace.lints.clippy]` in the root `Cargo.toml` (scrap-rs#12
  S1.1). Each crate's `Cargo.toml` opts in via `[lints]
  workspace = true`. The `[workspace.lints.rust]` block is reserved
  empty so future rust-lint policy lifts have a home. scrap4ts
  inherits the workspace lints cleanly at v0.6+ join.
- `lefthook.yml` pre-push `test:` command split per-crate cucumber
  invocations (scrap-rs#12 S1.1). The prior `cargo test --test
  cucumber` form was unambiguous when scrap-core was the only crate
  shipping a cucumber binary; once scrap4rs also ships one, cargo
  refuses the ambiguous `--test cucumber` invocation. The new
  pre-push runs `cargo test -p scrap-core --test cucumber` and
  `cargo test -p scrap4rs --test cucumber` sequentially.
- `crates/scrap-core/tests/cucumber.rs` and
  `crates/scrap4rs/tests/cucumber.rs` per-file `#![allow(...)]` blocks
  for the pedantic cucumber-step-fn nits now carry an inline
  `tracked: scrap-rs#50` reference per `~/.claude/rules/exclusions.md`.
  scrap-rs#50 owns the eventual lift (changing cucumber step-fn
  `String` params to `&str`, rewriting `match { _ => panic!() }`
  blocks as `let-else`).
- `parser.feature` — `@wip` tags removed from 4 scenarios as S2.1
  unlocks them: the attribute-variants outline, nested-mods,
  opt-out projection, and body_line_count. 3 wip scenarios remain
  (explicit assertions + implicit-source outline + the bare-#[test]
  scenario, which needs S2.2's "has 0 explicit assertions" step and
  S2.3's "has 0 implicit assertion sources" step before it goes
  green). scrap4rs cucumber: 9/9 scenarios pass (was 2/2 at S1.1).
- **Breaking** (scrap-rs#12, pre-v1.0 — no external consumers):
  `ParsedAssertion::kind: String` renamed to `ParsedAssertion::name: String`
  to align with `ParsedAttribute::name` and disambiguate from
  `ParseDiagnosticKind`. The canonical `ParsedAssertion::new(...)`
  constructor signature changes from `(kind, span)` → `(name, raw_args, span)`.
  No `schema_version` bump per
  [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)
  — `ParsedAssertion` is not part of the truthful-gate wire envelope
  (see `crates/scrap-core/tests/wire_envelope_snapshot.rs`).
- **Breaking**: `SourcePort::discover_test_files` signature changed
  from `fn(&self, root: &SourceRoot) -> Result<Vec<FilePath>, SourceError>`
  to `fn(&self) -> Result<DiscoveryOutcome, SourceError>`. The
  redundant `root` parameter has been dropped — adapters source the
  walked root from internal state (`AnalysisConfig::src` for
  `FsWalker`, owned files for `MemorySource`). Callers now receive
  the matching files and any non-fatal mid-walk diagnostics in one
  response. Existing `assert_obj_safe!` and
  `assert_not_impl_any!(dyn SourcePort: Send, Sync)` smoke checks
  continue to hold; the trait surface remains bound-free.
- `SourceError` gained two new variants (`#[non_exhaustive]`):
  `Ignore { source: ignore::Error }` for `OverrideBuilder::build()`
  setup failures (forward-compat hatch — exceedingly rare in
  practice; **NOT** fired by the infallible `WalkBuilder::build()`),
  and `EmptyExcludePattern { pattern: String }` for empty or
  whitespace-only exclude globs (caught eagerly at `FsWalker::try_new`
  so a config typo or empty env-var interpolation doesn't silently
  delete every walk result).
- Workspace MSRV bumped from `1.85` to `1.93`. An intermediate 1.88
  bump (file-walker pipeline) authorised cucumber 0.23 by lifting
  the prior 0.21 ceiling inside the harness. The 1.93 floor aligns
  with the sibling `crap-rs` workspace, where `oxc` 0.127+ drives
  the pin for the `crap4ts` adapter — the same constraint will apply
  to `scrap4ts` when it joins this workspace at v0.6+. An idiom-
  adoption audit (1.89–1.93 language-feature sweep across
  `crates/scrap-core/src/`) is tracked separately as a follow-up.

### Removed

- `PartialOrd + Ord` derives on `domain::types::FilePath` — never
  consumed; `PathBuf`'s natural component-wise ordering clashes
  with the byte-wise sort `FsWalker` uses for its post-collect
  ordering. Future call sites that need ordering must choose
  explicitly between component-wise and byte-wise (and document the
  choice).
