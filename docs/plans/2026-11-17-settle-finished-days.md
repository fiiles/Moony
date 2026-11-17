# Settle Finished Days Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Once a UTC day is over, every device rewrites stocks and crypto on it to Yahoo's daily close, so past days read the same on the desktop and the phone.

**Architecture:** A Local `app_config` key holds the last settled day. `history_plan::plan_startup` (pure planning) starts the existing ticker and days rebuild at the day after the mark; `start_snapshot_backfill` stores the mark through a batch completion. A sync snapshot replace deletes the mark.

**Tech Stack:** Rust (rusqlite, the existing `history_recalc` queue). No frontend, no migration, no wire type.

**Spec:** `docs/specs/2026-11-17-settle-finished-days-design.md`

## Global Constraints

- Key name, verbatim: `history_settled_through`. Value: the UTC day start in unix seconds, as a decimal string. It must NOT be added to `APP_CONFIG_SYNCED_KEYS`.
- Earned rule, verbatim: `days_ok && (ticker_jobs == 0 || failed_tickers.len() < ticker_jobs)`.
- `REBUILD_VERSION` stays 4.
- AGENTS.md rules 4 and 5: logic in `services/`, `AppError`/`Result`, no `.unwrap()` outside tests.
- Tests: `Connection::open_in_memory()` with the module's hand-written schema (`history_plan.rs` already has `setup_test_db`). Never the `Database` struct.
- Another session is editing `history_plan.rs` (the trigger helpers after `held_ticker_jobs` and the END of the tests module). Keep edits to the startup section and put new tests inside the `// --- startup (A7)` block, never at the end of the file.
- Commits: Conventional Commits, explicit paths, never `--no-verify`, check `git branch --show-current` is `feat/settle-finished-days` first. End every commit message with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. Offline at the first start after midnight: every ticker fails, the mark must stay (Task 1, `settle_earned` table).
2. Several unlocks on one day: the second plan queues nothing (Task 1, test 3).
3. A mark at or after yesterday (clock set back, mark already current): nothing queued, mark untouched (Task 1, test 8).
4. An unreadable mark value: treated as no mark, never a panic (Task 1, test 1).
5. A device that joins sync gets another device's history rows: its mark must go (Task 2, snapshot test).

---

### Task 1: The mark, the plan and the earned rule

**Files:**
- Modify: `src-tauri/src/services/history_plan.rs` (startup section and its tests)

**Interfaces:**
- Produces:
  - `pub fn stored_settled_through(conn: &Connection) -> Result<Option<i64>>`
  - `pub fn store_settled_through(conn: &Connection, day: i64) -> Result<()>` (upsert, like `store_rebuild_version`)
  - `pub fn forget_settled_through(conn: &Connection) -> Result<()>` (deletes the key)
  - `pub fn settle_earned(outcome: &BatchOutcome, ticker_jobs: usize) -> bool`
  - `StartupPlan.settles_through: Option<i64>`: yesterday when the mark has to move, else `None`. `plan_everything` sets `None`.

- [ ] **Step 1: Write the failing tests** in the `// --- startup (A7)` block. `d(n)` is the existing day helper, today is `d(0)`, `outcome(days_ok, failed)` is the existing helper.

  1. `the_settled_mark_is_stored_read_back_and_forgotten`: fresh db → `None`; `store_settled_through(&conn, d(-1))` → `Some(d(-1))`; value overwritten with `'garbage'` → `None`; stored again then `forget_settled_through` → `None`.
  2. `a_day_recorded_live_is_settled_at_the_next_start`: version stored, mark `d(-2)`, `balance_record(d(-10))`, AAPL buy 2 at `d(-9)`, a history row for every day `-10..=0`. Plan: `!one_time`, `from_day == Some(d(-1))`, `jobs == [StockTicker("AAPL", d(-1)), Days(d(-1))]`, `settles_through == Some(d(-1))`.
  3. `a_settled_profile_queues_nothing_more_that_day`: same profile with mark `d(-1)` → `jobs.is_empty()`, `from_day == None`, `settles_through == None`.
  4. `a_profile_without_a_mark_settles_from_the_first_fact`: version stored, no mark, `balance_record(d(-30))`, AAPL buy at `d(-20)`, every row present → `from_day == Some(d(-30))`, `jobs == [StockTicker("AAPL", d(-20)), Days(d(-30))]`, `settles_through == Some(d(-1))`.
  5. `the_earlier_of_the_gap_and_the_settle_start_wins`: facts from `d(-10)`; (a) mark `d(-2)` and the row of `d(-5)` missing → `from_day == Some(d(-5))`; (b) mark `d(-6)` and the row of `d(-3)` missing → `from_day == Some(d(-5))`. Both `settles_through == Some(d(-1))`.
  6. `a_profile_without_past_facts_only_moves_the_mark`: version stored, no mark, (a) no facts, (b) a first fact dated `d(0)` → `jobs.is_empty()`, `settles_through == Some(d(-1))`.
  7. `the_one_time_rebuild_settles_too`: version below `REBUILD_VERSION`, facts present, no mark → `one_time`, `settles_through == Some(d(-1))`; with mark `d(-1)` → `one_time`, `settles_through == None`.
  8. `a_mark_at_or_after_yesterday_is_left_alone`: marks `d(-1)`, `d(0)`, `d(3)` → no jobs, `settles_through == None`.
  9. `the_mark_is_earned_unless_the_days_failed_or_every_ticker_did`: `settle_earned` for `(days_ok, failed, ticker_jobs)`: `(false, [], 2)` → false; `(true, [], 0)` → true; `(true, [], 3)` → true; `(true, ["A"], 3)` → true; `(true, ["A","B","C"], 3)` → false; `(true, ["A","B","C","D"], 3)` → false; `(true, ["A"], 1)` → false.
  10. `settling_rewrites_the_stocks_of_a_day_recorded_live`: AAPL buy 2 at `d(-9)`, `balance_record(d(-9))`, a `stock_value_history` row AAPL `d(-1)` quantity 2 price 100 USD, a `source = 'live'` history row at `d(-1)` with `investments_by_currency = '{"USD":999}'`, mark `d(-2)`, rows for the other days present. `rebuild_days(&conn, plan.from_day.expect("settle"), d(0))` → the `d(-1)` row reads USD 200 for investments and is still `source = 'live'`.

- [ ] **Step 2: Update the existing startup tests** that store the version and expect "nothing queued" or test the gap rule alone (`a_profile_that_ran_version_three_rebuilds_once_more` second plan, `a_rebuilt_profile_with_every_day_present_queues_nothing`, `the_first_missing_day_starts_the_repair_with_the_tickers_held_from_it`, `yesterday_is_the_last_day_that_counts_and_today_is_not_a_gap`, `a_fresh_profile_balance_a_year_back_...`): store the mark at yesterday (`store_settled_through(&conn, d(-1))`, or `today - DAY`) next to the version so each keeps testing its own rule. Add `settles_through` to every `StartupPlan` literal.

- [ ] **Step 3: Run** `cd src-tauri && cargo test --lib history_plan` — expected: compile errors for the missing items.

- [ ] **Step 4: Implement.** `SETTLED_KEY` next to `VERSION_KEY`; the three store functions beside `store_rebuild_version`; `settle_earned` beside `rebuild_version_earned` with a doc comment giving the reason for each branch (spec, "Storing the mark"). In `plan_startup`:
  - `yesterday = today - DAY`; `settles_through = Some(yesterday)` when the stored mark is absent or `< yesterday`, else `None`.
  - One-time branch: unchanged jobs, plus `settles_through`.
  - Otherwise `settle_from` = (when `settles_through` is some) the first fact when there is no mark, else `max(first fact, mark + DAY)`; dropped when there is no first fact or it is after `yesterday`. Start = the earlier of the first missing day and `settle_from`; jobs as today (`held_ticker_jobs(start)` + `Days(start)`).
  - Update the module doc and the `plan_startup` doc for the new rule.

- [ ] **Step 5: Run** `cd src-tauri && cargo test --lib history_plan && cargo fmt --check && cargo clippy -- -D warnings` — expected: all pass, no warnings. (`commands/portfolio.rs` builds a `StartupPlan`? If the compiler asks, the only literals are in `history_plan.rs`.)

- [ ] **Step 6: Commit** `feat(history): plan the settling of finished days from a settled-through mark` (path: `src-tauri/src/services/history_plan.rs`).

### Task 2: Store the mark after the batch; forget it on a snapshot replace

**Files:**
- Modify: `src-tauri/src/commands/portfolio.rs` (`start_snapshot_backfill`, `store_version_when_done`)
- Modify: `src-tauri/src/services/sync/snapshot/mod.rs` (`replace`) and its tests

**Interfaces:**
- Consumes: everything Task 1 produces.

- [ ] **Step 1: Write the failing snapshot test** beside the existing `restore_replace` tests of `services/sync/snapshot` (reuse their harness): `a_replace_forgets_the_settled_mark` — before the replace insert `app_config` rows `history_settled_through = '1700000000'` and `history_rebuild_version = '4'`; after `restore_replace` the first is gone and the second is still there.

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib sync::snapshot` — expected: the new test fails.

- [ ] **Step 3: Implement the forget.** In `replace`, right after `clear(conn)?`, call `crate::services::history_plan::forget_settled_through(conn)?` with a comment: the Seed tables now hold another device's rows, whose finished days this device has not settled.

- [ ] **Step 4: Wire the command.** In `start_snapshot_backfill`:
  - `ticker_jobs` = the plan's jobs that are not `Job::Days`.
  - Label for `queued_result`: one-time → the existing text; else `settles_through.is_some()` → `"the settling of the finished days"`; else `"a repair of the missing days"`.
  - One completion when `plan.one_time || plan.settles_through.is_some()` (else `None`, as today): the existing version logic when `one_time`; then, when `settles_through` is `Some(day)`, `settle_earned(outcome, ticker_jobs)` → `store_settled_through(conn, day)` and `log::info!("[RECALC] Finished days settled to closing prices through {day}")`; not earned → `log::warn!` that the next start tries again. A store failure is logged, never panics. Keep `store_version_when_done`'s behaviour and log lines for the one-time part (fold it into the new completion or call it from there).

- [ ] **Step 5: Run** `cd src-tauri && cargo test --lib sync::snapshot history_plan && cargo fmt --check && cargo clippy -- -D warnings` — expected: pass. Then `cargo test` (whole crate) — expected: pass.

- [ ] **Step 6: Commit** `feat(history): settle finished days to closing prices at the start after unlock` (paths: the two files).

### Task 3: Docs (the controller writes these)

**Files:**
- Create: `docs/architecture/decisions/0033-finished-days-settle-to-closing-prices.md`; add it to `docs/architecture/decisions/README.md`
- Modify: `docs/architecture/overview.md` (the "Startup" bullet), `docs/architecture/database.md` (the `app_config` row)

- [ ] **Step 1:** ADR 0033: context (D5 + ADR 0016, the mid-session price), decision (mark, plan, earned rule, replace forgets), consequences and the accepted limits from the spec. Links back to ADR 0016 and ADR 0024.
- [ ] **Step 2:** Update the two living docs in the same commit: `docs(history): finished days settle to closing prices (ADR 0033)`.
