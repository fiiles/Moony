# History from dated facts and manual loan balances — plan

Spec: `docs/specs/2026-10-13-history-from-dated-facts-design.md` (section ids B1–B7 and A1–A8
below refer to it). Work happens in the worktree `../Moony-tauri-oct13` on the branch
`fix/oct13-batch`. Never commit in the shared checkout
`/Users/filipkral/Programování/FinanceApp/Moony-tauri`.

Read `AGENTS.md` (the ten rules), `docs/standards/rust-backend.md`, `docs/standards/type-contract.md`,
`docs/standards/i18n.md` and `docs/DEPRECATED.md` before coding. Use TDD: write the failing test,
then the code. Use one Conventional Commit per task, ending with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. The pre-commit hook runs
lint-staged, typecheck and `cargo fmt --check`.

Gate per task, before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run typecheck && npm test
```

## Status

| Task | Status | Commit |
|---|---|---|
| 0. Interim: stock/crypto rebuilds create missing days, CoinGecko key rebuilds crypto | done | 9e9a90e |
| 1. Loan ledger maths (Rust + TS mirror + fixture) | done | 65da31d |
| 2. Loan data: migration 019, services, MCP, milestone, wire contract | done | f20c62b |
| 3. Loans UI and copy (10 locales) | done | b55c215 |
| 4. History derivation (`history_days.rs`, `rebuild_days`) | done | ebd7b27 |
| 5. Queue, triggers, MCP domains, startup, one-time rebuild, dead-code removal | todo | |
| 6. Docs: ADR 0016, database.md, overview.md, DEPRECATED.md | todo | |
| 7. Verification: all gates, live demo check, review | todo | |

## Task 1 — Loan ledger maths (B2, B3)

**Rust, in `src-tauri/src/services/loan_amortization.rs`:**

- Add the ledger types: `LedgerEvent { day, created_at, kind, amount, rate, monthly_payment,
  previous_rate, previous_monthly_payment }`, and `LoanLedger { terms: LoanTerms, events:
  Vec<LedgerEvent> }`.
- Add `pub fn loan_balance_at(ledger: &LoanLedger, day: i64) -> LedgerState { balance, mode,
  segment_anchor_day, monthly_rate, monthly_payment }`, implemented exactly per B2. `LoanTerms`'
  anchor fields are ignored by the ledger; the events are the source.
- Add `pub fn plan_from(ledger, today) -> LoanTerms`, per B3: the terms whose `anchor()` starts
  the forward schedule, so `schedule()` and `payments_due_through` stay reusable.
- Keep `elapsed_to` and `amortization_step` as the inner steps.

**TS mirror, in `shared/calculations/loan-amortization.ts`:** `loanBalanceAt` and `planFrom`, with
the same semantics.

**Fixture, in `shared/calculations/loan-amortization.fixture.json`:** add cases, read by both the
Rust and the TS tests:

- estimated only;
- a manual record holds over 6 months;
- extra payment in estimated mode, and in manual mode (manual − payment, D7);
- rate change in estimated mode (re-anchor), and in manual mode (balance unchanged, plan changes);
- two manual records: the first holds until the second; deleting the second restores the first;
- before start = 0;
- a legacy `rate_change` without `previous_*`;
- the plan ahead in manual mode.

Find how the existing fixture is loaded in both test suites and extend it the same way.

**Done when:** both test suites pass on the shared cases. Commit
`feat(loans): ledger walk with manual balances that hold`.

## Task 2 — Loan data, services, MCP, contract (B1, B4, B5, B7)

- **Migration 019** per B7. Add it as the next entry in `migrations.rs`, use the UUID SQL recipe
  of migration 013, and add a migration test. Regenerate `schema.snapshot` with
  `cargo test regenerate_schema_snapshot -- --ignored`.
- **`services/loans.rs`:** `outstanding_by_currency`, `get_loan`, `list_loans` and `loan_schedule`
  load the events and use `loan_balance_at` / `plan_from`. `Loan` gains `events` and `balance_mode`.
  The form semantics (switch on → upsert the manual record of that day; switch off → delete all
  manual records) live in `create_loan` / `update_loan`.
- **`services/loan_events.rs`:**
  - `add_event` stores `extra_payment` / `rate_change` without touching the anchor columns;
    `rate_change` writes `previous_*`.
  - The new `add_manual_balance(conn, loan_id, day, amount)` replaces the same day.
  - `delete_event` re-mirrors the anchor columns from the newest `balance_check`.
  - Keep `validation.loanEventBeforeStart`.
- **Commands:** expose `add_loan_manual_balance` (or reuse `add_loan_event` with kind
  `balance_check`, which is simpler and does not change the contract). Each command calls
  `validate()` first; there is no business logic in commands.
- **MCP** (`services/mcp/loans.rs`): `balanceMode` in `loans_list`; `loan_create` with an anchor
  creates a manual record. Update the tool descriptions.
- **Milestones** (`services/milestones/loans.rs`): use the "last checked" day per B4.
- **Wire contract** per B5, following the 7-step procedure in `type-contract.md`: update
  `shared/schema.ts`, `bindings.rs` and `generated-types.ts`
  (`cargo test generate_bindings -- --ignored`).

**Tests:** service tests for each rule, the migration test, and the MCP list output.

**Commit:** `feat(loans): manual balances are records that hold; migration 019`.

## Task 3 — Loans UI and copy (B6)

- **Step 0 (maths fix, decided by the lead):** the loan form edits the loan's current rate and
  payment, so the LAST terms segment of the ledger walk (after the last event that carries a rate
  or payment, or from the start when none does) runs on the loan's current columns; earlier
  segments keep the values their events carry. Rust `loan_balance_at` and TS `loanBalanceAt`, two
  new fixture cases, and `event_terms_at` (the walk without that rule) for
  `loan_events::sync_current_terms`, which reads the event chain back to move the columns.
- **Components:** `LoanEventDialog`, `LoanEventsCard`, `LoanFormDialog`, `LoanValuationNote`
  (and the `use-loan-valuation` hook), `DebtTrajectoryCard`, `LoanScheduleTable`, `LoanWhatIfCard`.
- **Utils:** `src/utils/loans.ts` and `src/utils/loan-trajectory.ts` use `loanBalanceAt` /
  `planFrom` with `loan.events`. Every manual record is a step on the past line. Update their
  tests.
- **Copy:** the balance-check action becomes "Zadat zůstatek k datu". Rewrite the help, note and
  status copy and the delete confirmation per B6, plus the milestone copy in `milestones.json`.
  Every string goes into all ten locales (`en cs de es it nb pl sv ja zh`);
  `src/i18n/locales.test.ts` must pass.
- **Mutations:** follow AGENTS rule 7 — invalidate the loan keys, `["portfolio-metrics"]` and
  `["cashflow-report"]`, then `recordSnapshot()` and invalidate `["portfolio-history"]`.

**Commit:** `feat(loans): one "Zadat zůstatek k datu" action; manual balances hold in the UI`.

## Task 4 — History derivation (A1–A4, A8)

**New `src-tauri/src/services/history_days.rs`, registered in `services/mod.rs`:**

- `struct Facts` loads, once: accounts with their records (ascending), properties with their
  valuations and purchase data, loans as `LoanLedger`, other assets with transactions and
  valuations, bonds, and the ticker series (reuse `portfolio_history::load_ticker_series`; make it
  `pub(crate)` if needed).
- `fn day_values(&mut self, day) -> DayValues { breakdowns: ClassBreakdowns, known: per-class
  bool }`, exactly per the A1 table, using cursors and no per-day queries.
- `pub fn earliest_fact_day(conn) -> Result<Option<i64>>` per A2.
- `pub fn rebuild_days(conn, from_day, until_exclusive) -> Result<RebuildStats>`, per A2–A4:
  1. Start at `max(from, E)`, chunked by `CHUNK_DAYS` with one transaction per chunk. Expose
     `rebuild_chunk` so the async wrapper in `history_recalc` can yield between chunks.
  2. Backfill and new rows get `upsert_snapshot(Backfill)`. Live rows get `update_classes` for
     the known classes only. Stocks and crypto are always updated.
  3. Delete `source='backfill'` rows before E.

**Tests:** in the module, in-memory DB with a hand-written schema; see AGENTS.md:

- each class rule;
- fresh profile + a balance a year back;
- the live guard;
- deleting the earliest fact;
- idempotence;
- chunked output equals one pass;
- A8 consistency against `net_worth::compute_breakdowns` (switch its loans to the ledger first).

**Commit:** `feat(history): derive every class of a past day from dated facts`.

## Task 5 — Queue, triggers, startup (A5–A7)

- **Queue:** generalise `services/history_recalc.rs` to the job kinds of A5, and move the crypto
  ticker rebuild into the worker. The worker ends each batch with `rebuild_days` over the earliest
  date in the batch. Keep the queue unit tests and add tests for the coalescing of the kinds.
- **Entry point:** `commands/portfolio.rs` gets `schedule_history_rebuild(app, db, recalc, jobs)`.
- **Triggers:** wire every write in the A6 table, in the commands (`bank_accounts`, `investments`,
  `stock_import`, `crypto`, `other_assets`, `real_estate`, `loans`, `bonds`, `price_api` set_api_keys)
  and in `commands/mcp_refresh.rs` (the new domains; check which domains the MCP write tools emit
  in `services/mcp/` and `services/mcp_refresh.rs`).
- **Startup:** `start_snapshot_backfill` becomes "plan and queue" per A7 (the
  `app_config.history_rebuild_version` key, the gap detection). `recalculate_all_portfolio_history`
  queues `Days(E)` plus every ticker.
- **Dead code:** remove the functions A4 replaces, the interim `rebuild_ticker_class` /
  `spawn_crypto_history_rebuild`, and the then-unused `backfill` helpers. `cargo clippy` must show
  no dead code. Add the removed patterns to `docs/DEPRECATED.md`.
- **Frontend:** make sure every history consumer refetches on `recalculation-complete`.

**Commit:** `feat(history): one rebuild queue for every dated write; startup fills and rebuilds once`.

## Task 6 — Docs

- **ADR 0016**, `docs/architecture/decisions/0016-history-from-dated-facts.md`: context, decision
  (P1–P3, D1–D10 condensed), consequences. Add it to the index. The next free number after it is
  0017.
- `docs/architecture/database.md`: the `portfolio_metrics_history` row no longer says "carried";
  `loans` / `loan_events` (manual records, `previous_*`, the mirror); the migration 019 bullet.
- `docs/architecture/overview.md`: rewrite "History recalculation" (the queue and its job kinds,
  `rebuild_days`, triggers, startup) and drop "Crypto history still rebuilds synchronously".

**Commit:** `docs: ADR 0016 history from dated facts`.

## Task 7 — Verification

- Run all gates, plus the bindings drift check.
- Request a code review (Sonnet) against the spec and fix the findings.
- **Live check:** run the demo app from the worktree on port 1421 and work through the
  scenarios in spec §6. Use a test profile; never type the owner's real password.
- Leave the release notes to the owner and mention the loan behaviour change.
- **Merge:** merge locally into `main` through a temporary worktree. Check that the feature tip
  is an ancestor of `main` before deleting the branch. No push.
