#!/usr/bin/env bash
set -euo pipefail

# Coverage execution for the parser tree (scrap-rs#12).
#
# Mirrors the CI workflow's coverage job step-for-step so local + CI
# produce identical lcov.info. Run from worktree root:
#
#   ./scripts/coverage-parser.sh
#
# Two-step accumulation: nextest covers the lib + standard integration
# binaries excluding cucumber (which needs harness=false), then both
# cucumber binaries run via `cargo test --test cucumber` also with
# --no-report, accumulating into the same llvm-cov data, then
# `report` finalizes the merged lcov.info and enforces the 85%
# line-coverage threshold via `--fail-under-lines 85`.
#
# Why this script exists:
#
# - Single execution path for coverage. S3.1's gate runs this; CI runs
#   the same chain inline. Future detector PRs (#24/#25/#26 etc.)
#   inherit the pattern via `scripts/coverage-<concern>.sh`.
# - Future developers running on a fresh worktree don't have to
#   re-derive the chained llvm-cov invocations.
# - Replaces the inline llvm-cov chain that lived in S3.1 step 3 of
#   the impl-plan.

cargo llvm-cov clean --workspace
cargo llvm-cov --no-report nextest \
    --workspace --locked -E 'not binary(cucumber)'
cargo llvm-cov --no-report test \
    -p scrap-core --test cucumber --locked
cargo llvm-cov --no-report test \
    -p scrap4rs --test cucumber --locked
cargo llvm-cov report \
    --lcov --output-path lcov.info --fail-under-lines 85
