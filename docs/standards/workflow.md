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
   npm run lint && npm run typecheck && npm run format:check && npm test
   cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
   ```
2. **Tests** written per `docs/standards/testing.md` (regression test for bug fixes,
   co-located tests for new pure functions, service tests for new Rust logic).
3. **All ten locales updated** for any user-facing string (`docs/standards/i18n.md`).
4. **Docs updated** if behavior changed: README for features,
   `docs/architecture/database.md` for tables, an ADR for architectural decisions.
5. **Worktree removed** if the work ran in its own git worktree and its branch is now
   merged into `main` (section 6).

## 5. Doc freshness rules

- **Specs and plans are frozen snapshots** — they describe the design/codebase as of
  their date and are never updated afterward (see `docs/specs/README.md` and
  `docs/plans/README.md`). Never follow an old plan for new work.
- **Standards, architecture docs, and ADRs are living docs** — update them in the same
  PR/commit that invalidates them. ADRs themselves are immutable; superseding one means
  writing a new ADR that links back.
- **Never state file sizes or line numbers in living docs** — they rot immediately.
  Name files by role instead ("the budgeting service", not "budgeting.rs, 686 lines").

## 6. Worktrees and disk space

A worktree with Rust builds takes tens of gigabytes, so it lives only as long as its
branch is unmerged.

- **Merged means removed.** As soon as a worktree's branch is merged into `main`, remove
  the worktree in the same session, without asking. The merge is not finished until the
  worktree is gone.
- **Check before removing**, in this order:
  1. `git merge-base --is-ancestor <branch> main` succeeds (the whole branch is in `main`).
  2. `git -C <worktree> status --porcelain` prints nothing (no uncommitted or untracked
     work).
  3. No process runs from the worktree (a dev server, `cargo`, a test binary).
- **Remove it with git**, from the parent folder and with a relative path:
  `git worktree remove ../Moony-tauri-<topic>`. This deletes the folder with its
  `src-tauri/target`, `node_modules` and `dist`. For a worktree that built the iOS app,
  delete its `~/Library/Developer/Xcode/DerivedData/moony-*` folder as well.
- **Never force.** If a check fails (unmerged commits, uncommitted files, a running
  process, another session at work in it), leave the worktree alone and report it. Never
  use `git worktree remove --force` or `rm -rf` on a worktree with unmerged work.
- **Sweep at the start of work.** Before creating a new worktree, run `git worktree list`
  and remove every worktree that passes the three checks, whoever created it. Judge a
  worktree by its branch, never by its folder name.
- The main checkout is not a worktree to remove; its build folder is cleaned only when the
  owner asks.
