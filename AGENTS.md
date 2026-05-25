# scrap-rs Agent Notes

Cross-provider agent operating guide. Both Claude Code and Codex
should read this before touching code.

## Repo Identity

- `scrap-rs` is a Rust **workspace** in `breezy-bays-labs` org. Public
  visibility from day one; **no crates.io publish, no GitHub Release
  tarballs, no `cargo install` path until v1.0**. Tags exist for git
  pinning only — internal versions like `v0.1.0` do not trigger any
  workflow.
- The v1.0 release gate is publish: workspace already has the right
  shape (`crates/scrap-core/` + `crates/scrap4rs/`, with
  `crates/scrap4ts/` joining at v0.6+). Until v1.0, mokumo is the
  sole consumer via composite GitHub Action.

## Architecture

Hexagonal (ports & adapters), strict dependency direction enforced by
Cargo crate boundaries. See
[`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-hexagonal-layout.md)
for the full layering invariant.

```
scrap-core (no AST libs)
    ↑
scrap4rs (depends on scrap-core; adds syn, proc-macro2, quote)
    ↑
scrap4ts (depends on scrap-core; adds swc_ecma_parser or oxc, napi-rs)  [v0.6+]
```

| Crate | Purpose | Allowed deps |
|-------|---------|--------------|
| `scrap-core` | Domain types, port traits, generic orchestration, detector logic, CLI surface, language-agnostic adapters (file walker, reporters) | `serde` (derive), `serde_json`, `walkdir`, `ignore`, `globset`, `comfy-table`, `clap` (derive), `thiserror` |
| `scrap4rs` | Rust-source parser adapter + binary | `scrap-core`, `syn`, `proc-macro2`, `quote` |
| `scrap4ts` | TypeScript-source parser adapter + binary | `scrap-core`, `swc_ecma_parser` *or* `oxc_parser`, `napi-rs` |

**Never import inward.** `scrap-core` must stay free of AST libraries
(`syn`, `swc_*`, `oxc_*`, `tree-sitter*`, `proc-macro2`, `quote`).
Enforcement: structural (`scrap-core/Cargo.toml` does not list them)
+ source-level (`ast-purity` CI job rejects matching `use` lines in
`crates/scrap-core/src/`).

## Working Rules

- **TDD** — tests before implementation for all domain and adapter code.
- **Domain purity** — `crates/scrap-core/src/domain/` must never import
  external crates (other than `serde` derive) or perform I/O.
- **Self-referential test** — once detectors land, scrap4rs must
  analyze its own source as an integration test (the `self-check`
  CI job).
- **Symmetric dogfood** — scrap4rs's CI also runs `crap4rs` against its
  own production code (`crap-self` job, gated on the production-code
  CC ladder).
- **No release workflow during v0.x** — `release.yml` arrives at v1.0
  prep. Tags are git-pinning markers only.
- **No `tools.toml` Warden pin** in mokumo during v0.x — mokumo
  consumes scrap-rs via composite action ref (`@v0.x.0`); the action
  self-builds scrap4rs from the ref.
- **No direct push to main** — branch + PR for all work after the
  initial bootstrap commit.
- **Worktrees** for parallel work: `git worktree add ../scrap-rs-issue-N -b feat/topic-name`.
- **Property tests required** for the smell scoring formula and the
  parser's assertion-recognition list (see `domain/assertion_sources.rs`).
- **Regression files committed** — any `proptest-regressions/` dirs
  go into git, never gitignored. Commit the regression file + fix in
  the same PR.

## Test Framework Recognition

scrap4rs's `zero-assertion` detector treats the following as
**implicit-assertion sources** and suppresses the smell:

- Macros: `proptest!`, `quickcheck!`, `kani::*`, any `*_proptest!`
- Method-call chains ending in `.await` on `World::cucumber()` or
  `cucumber::run` / `cucumber::Cucumber::run`
- Function calls to `quickcheck::quickcheck`, `trybuild::TestCases::*`
- Macros: `insta::assert_*!`, `pretty_assertions::*`
- `#[should_panic]` attribute on the enclosing fn

The list lives in `domain/assertion_sources.rs` (data-driven). New
idioms join via PR with a fixture under
`crates/scrap4rs/tests/fixtures/runner_shells/` that MUST NOT trigger
any detector.

## Idiomatic Rust Test Mapping

For v0.3+ block-level reports (Speclj `describe`/`context` analog):

- `block` = `mod tests { ... }` under `#[cfg(test)]`, with arbitrary
  nesting (`mod auth { mod login { ... } }`).
- `describe-path[]` = the fully-qualified module path
  (`crate::auth::tests::login`).
- `rstest::rstest` parameterization is **orthogonal** — table-drive
  within one test, captured under `table-driven?` flag.
- Free-function `#[test]` siblings without an enclosing `mod` are a
  single anonymous block per file.
- Doc-tests are out of scope at v0.1 (separate parser pipeline,
  v0.3+ follow-up).

## Cross-References

- **Pipeline plan**: `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`
  (in the private ops vault — full architecture, detector phasing, CI
  templates, sub-issue tree)
- **Adoption tracker**: [mokumo#649](https://github.com/breezy-bays-labs/mokumo/issues/649)
- **Parent epic (mokumo)**: [mokumo#370](https://github.com/breezy-bays-labs/mokumo/issues/370)
- **Quality manifest Q1 slot**: `ops/standards/quality-manifest.md:48`
- **Sibling — production-code complexity**: [crap4rs](https://github.com/breezy-bays-labs/crap4rs)
- **Modeled on**: [unclebob/scrap](https://github.com/unclebob/scrap) (Clojure, Speclj)

## Supply-chain hygiene

Every GitHub Actions `uses:` reference in the repo — across
`.github/workflows/*.yml` AND `.github/actions/*/action.yml` — is
SHA-pinned with a trailing `# vX` (or `# tracks @<branch>`) comment,
and the freshness loop is closed by Dependabot + zizmor. The combined
pin + autobump + audit policy guards against tag-poisoning and
ref-shadowing attacks (a published `@v4` tag is mutable; a 40-char
commit SHA isn't).

**Deviation from crap4rs**: this section imports Rules 1–8 from the
crap4rs "Supply-chain hygiene" convention. Rules 9 (cache-poisoning
fix for release workflows) and 10 (prefer `gh` CLI to a third-party
release action) reference `release-plz.yml` and the `setup-rust`
composite action with `enable-cache: "false"` — both deferred to v1.0
per `CLAUDE.md > v0.x → v1.0 Transition`. Both rules re-apply when
scrap-rs gains a release workflow.

### Rules

1. **SHA-pin every `uses:` reference.** Format:

   ```yaml
   - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
   ```

   The trailing comment names the human-readable ref the SHA
   resolves to so reviewers can recognize the action without a `gh
   api` call. New workflows + composite actions follow the same
   pattern; the `zizmor` CI job fails the build on any
   `unpinned-uses` finding (mechanical enforcement — "documentation
   rots; CI doesn't").

2. **Floating-branch actions get pinned to a branch-HEAD SHA — but
   Dependabot can't bump them, so manual refresh is required.** Some
   actions (e.g. `dtolnay/rust-toolchain@stable`,
   `dtolnay/rust-toolchain@1.88`) publish version-channel branches
   rather than tagged releases — `@stable` is a branch that bakes in
   "whatever Rust release is current" and advances every ~6 weeks.
   These get pinned to the current branch-HEAD SHA with a comment
   naming the branch:

   ```yaml
   - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # tracks @stable branch
   ```

   **Dependabot's `github-actions` ecosystem tracks updates on an
   action's default branch (and on tagged releases) — it does NOT
   track HEAD advances on non-default branches.** For
   `dtolnay/rust-toolchain` the default branch is `master` (which
   has different `action.yml` content per version-channel branch),
   so a Dependabot-proposed SHA from `master` would silently break
   the action. The pin therefore needs a manual refresh on a
   quarterly cadence (or before any major Rust release the project
   depends on). The conscious tradeoff: reproducible CI (SHA-locked)
   at the cost of manual upkeep on the toolchain-action pin.

3. **Resolve SHAs via `gh api repos/foo/bar/commits/<tag>`.** This
   form returns the underlying **commit SHA** for any tag —
   lightweight or annotated — without the caller having to know the
   difference:

   ```bash
   gh api repos/foo/bar/commits/vX --jq '.sha'
   ```

   The alternative `gh api repos/foo/bar/git/ref/tags/<tag> --jq
   '.object.sha'` returns the *tag object* SHA for annotated tags
   (only the commit SHA for lightweight ones), which mixes
   representations across actions and risks pin drift. Use the
   `commits/<tag>` form unconditionally — both `@v4`-style
   floating-major and `@v1.18.1`-style pinned-release tags work.
   For branch-pinned actions:

   ```bash
   gh api repos/foo/bar/branches/<branch> --jq '.commit.sha'
   ```

4. **Local composite actions (`./.github/actions/<name>`) don't get
   pinned.** They're paths within this repo, not external
   references; the implicit version is "whatever's on the same
   branch". Their content (the `action.yml` inside) is SHA-pinned
   internally per rule 1. scrap-rs has no local composite actions
   today; the first one (a `setup-rust` extraction mirroring
   crap4rs) is tracked under [scrap-rs#51](https://github.com/breezy-bays-labs/scrap-rs/issues/51).

5. **Dependabot for `github-actions` AND `cargo` is enabled.** See
   `.github/dependabot.yml`. Weekly cadence (Mondays); bumps land as
   PRs with `type:chore` + `priority:soon` labels. Minor/patch bumps
   are grouped into one weekly PR per ecosystem (smaller review
   surface); major bumps land as separate PRs (breaking-change
   review). Commit prefix `ci(deps):` for github-actions,
   `chore(deps):` for cargo (per the workspace's commit convention
   — `ci:` for CI-only changes, `chore:` for non-source maintenance).

6. **Per-audit `zizmor` ignores get a `tracked:` comment.** New
   audits surfaced by zizmor that we scope-defer (e.g. a workflow-wide
   convention change too large for the current PR) land in
   `.github/zizmor.yml` with a `# tracked: scrap-rs#N` comment naming
   the follow-up issue. When the follow-up lands, the ignore is
   removed and the audit fires unconditionally — same accountability
   pattern as the rest of the repo's exclusions (mirrors
   `~/.claude/rules/exclusions.md`). The file is intentionally absent
   in steady state: it appears only when an audit is in flight to a
   tracked issue. Inline ignores in composite actions or in workflow
   spans use `# zizmor: ignore[<audit>]` on the identified line; if
   the suppression is permanent (intentional + bounded), no
   `tracked:` comment is needed.

7. **`persist-credentials: false` on every `actions/checkout`.**
   Workflows that never push (every job in this repo's `ci.yml`)
   keep the GH App checkout token out of the runner's `.git/config`.
   Already wired on every checkout repo-wide; the `artipacked` audit
   fails any new checkout that omits it.

8. **Scoped per-job `permissions:` blocks (least privilege).** Each
   workflow declares a top-level `permissions: contents: read`
   default; jobs that need more elevate explicitly (when a release
   workflow lands at v1.0, the publish path needs `contents: write` +
   `id-token: write` for OIDC; today all 9 `ci.yml` jobs are
   read-only so no overrides are wired). Never let a job inherit
   the runner's default workflow permissions silently; the
   `excessive-permissions` audit fails any job missing a
   `permissions:` block (or workflow-level default).

### When the rules bite

- Adding a new workflow / composite action → SHA-pin every external
  `uses:` reference from day one, add `persist-credentials: false`
  to every checkout, declare a scoped `permissions:` block per job
  (or rely on the workflow-level default if `contents: read` is
  enough). The `zizmor` CI gate fails the PR otherwise.
- Adding a new step that triggers a new zizmor audit → fix the
  finding directly OR file a follow-up issue and add a tracked
  entry to `.github/zizmor.yml`. Never silently expand the ignore
  list without a tracking issue.
- Reviewing a Dependabot bump PR → eyeball the upstream release
  notes for the bumped action / cargo crate; merge if benign.
  Multi-action group PRs may need staging if one bump is more
  contentious than the others.
