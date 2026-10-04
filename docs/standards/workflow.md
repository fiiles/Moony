# Workflow Standards

How work moves from idea to `main` in this repo. The tier decision is recorded in
ADR 0003 (`docs/architecture/decisions/0003-tiered-ai-workflow.md`); this file is the
operational procedure.

## 1. Tiers (ADR 0003)

| Tier | Trigger | Required stages |
|------|---------|-----------------|
| **Feature** | new command, table, page, or domain — or change >~150 lines | design → spec (`docs/specs/YYYY-MM-DD-<topic>-design.md`) → plan (`docs/plans/YYYY-MM-DD-<topic>.md`) → TDD → code review → verification-before-completion |
| **Small fix** | bug fix, copy change, config tweak | TDD + gates + verification (no spec/plan) |

When unsure which tier applies, **ask the maintainer** — don't guess.

## 2. Gates

| Gate | Runs | What |
|------|------|------|
| **pre-commit** (<10s) | every commit | lint-staged (staged-file ESLint/Prettier) + `npm run typecheck` + `cargo fmt --check` |
| **pre-push** | every push | `npm test` + `cargo clippy -- -D warnings` + `cargo test` |
| **CI** (authoritative) | every push/PR | everything above + `shared/generated-types.ts` drift check + `cargo audit` |

`--no-verify` is an emergency hatch, never a habit. Anything skipped locally must
still pass CI — skipping defers the gate, it does not waive it.

## 3. Commits

- **Conventional Commits:** `feat:` / `fix:` / `docs:` / `chore:` / `build:` / `ci:` /
  `test:` / `refactor:`.
- Small and frequent — one logical change per commit.
- Trunk-based development on `main`; no long-lived branches.

## 4. Definition of done

A task is done when all of the following hold:

1. **Gates green:**
   ```bash
   npm run lint && npm run typecheck && npm test
   cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
   ```
2. **Tests** written per `docs/standards/testing.md` (regression test for bug fixes,
   co-located tests for new pure functions, service tests for new Rust logic).
3. **Both locales updated** for any user-facing string (`docs/standards/i18n.md`).
4. **Docs updated** if behavior changed: README for features,
   `docs/architecture/database.md` for tables, an ADR for architectural decisions.

## 5. Doc freshness rules

- **Specs and plans are frozen snapshots** — they describe the design/codebase as of
  their date and are never updated afterward (see `docs/specs/README.md` and
  `docs/plans/README.md`). Never follow an old plan for new work.
- **Standards, architecture docs, and ADRs are living docs** — update them in the same
  PR/commit that invalidates them. ADRs themselves are immutable; superseding one means
  writing a new ADR that links back.
- **Never state file sizes or line numbers in living docs** — they rot immediately.
  Name files by role instead ("the budgeting service", not "budgeting.rs, 686 lines").
