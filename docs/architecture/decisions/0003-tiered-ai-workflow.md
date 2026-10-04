# 0003. Tiered AI workflow

Date: 2026-07-07

## Status

Accepted

## Context

Moony is developed mostly through AI-agent sessions. A single one-size
process either buries a two-line bug fix in design ceremony or lets a whole
feature skip design review. Separately, quality gates only work if they run:
slow pre-commit hooks incentivize `--no-verify`, and the spec-and-plan flow
produces many small commits per feature, multiplying that cost.

## Decision

Work is tiered:

| Tier | Trigger | Required stages |
|------|---------|-----------------|
| **Feature** | new command, table, page, or domain — or change >~150 lines | design → spec in `docs/specs/` → plan in `docs/plans/` → TDD → code review → verification-before-completion |
| **Small fix** | bug fix, copy change, config tweak | TDD + gates + verification (no spec/plan) |

Quality gates are tiered by cost so none of them get skipped:

- **Fast pre-commit** (target <10 s): lint-staged (staged-file
  ESLint/Prettier), incremental `tsc --noEmit`, `cargo fmt --check` — runs on
  every commit.
- **Heavy pre-push:** `npm test`, `cargo clippy`, `cargo test` — runs once
  per push instead of once per commit.
- **CI is authoritative:** everything above plus the generated-types drift
  check and `cargo audit`. Anything skipped locally must still pass CI.

## Consequences

- The tier is judged at task start; when unsure, ask the maintainer which tier
  applies rather than guessing.
- Commits stay cheap (seconds), pushes cost minutes — matching how often each
  happens.
- `--no-verify` remains an emergency hatch, never a habit; it defers, not
  waives, the gates (CI still blocks).
- Operational detail lives in `docs/standards/workflow.md`; this ADR records
  the decision, not the procedure.
