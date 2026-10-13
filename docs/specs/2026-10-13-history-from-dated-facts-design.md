# History from dated facts and manual loan balances — design

Date: 2026-10-13. Approved by the owner in chat (decisions D1–D10 below). Worktree branch
`fix/oct13-batch` (`../Moony-tauri-oct13`). This spec is also the hand-off brief for any agent that
continues the work: every rule an implementer needs is stated here.

## 1. Problem

The owner reported, on a fresh database:

1. Stock transactions imported → the stocks trend chart shows a single point.
2. A bank account balance recorded as of a year ago → the history chart does not change.
3. Crypto must be recomputed after a CoinGecko key is entered, and also when a historical
   transaction is entered after the key.

Root causes (verified in code and by an audit):

- **Missing days.** Ticker recalculation (`aggregate_ticker_history`) and the asset recalculation
  (`update_classes`) only UPDATE existing `portfolio_metrics_history` rows. A fresh profile has only
  today's live row, so nothing is written. The startup backfill (`backfill_missing_snapshots`) only
  fills gaps after the oldest existing row, so it never extends history backwards. *Interim fix
  already committed (`9e9a90e`): `portfolio_history::rebuild_ticker_class` creates missing days for
  stocks and crypto; saving a new CoinGecko key rebuilds crypto history.*
- **Static classes are never derived.** Savings, real estate, loans and bonds on past days are
  copied from the nearest live row (`carried_statics_from_nearest_live`). Nothing reads
  `bank_balance_records`, `real_estate_valuations`, `real_estate.purchase_date` or loan
  amortization at a past day when building history. `database.md` documents this as intended
  ("static classes carried from the nearest live row").
- **Write paths without a trigger.** Recording or deleting a bank balance, adding or deleting a
  real-estate valuation, loan events, creating an other asset with an initial transaction (UI and
  MCP), bonds: none of them recalculates history. MCP follow-up (`commands/mcp_refresh.rs`) has
  no branch for bank accounts, real estate, loans or bonds.
- **Other gaps.**
  - `recalculate_asset_history_from_date` writes nothing on interior gaps.
  - The full-recalculation path aborts on a network error even for other-asset-only changes, and
    UI callers swallow that with `.ok()`.
  - The startup backfill values past days with **today's** holdings
    (`breakdowns_for_backfill_day` → `positions::held_quantities`).

## 2. Principles

- **P1. History is derived from dated facts.** The value of a class on day D is computed from the
  facts dated on or before D, using the same rule the domain's own page uses. Nothing is copied
  from another day when a dated fact exists.
- **P2. A manual value is authoritative.** When the user overrides a value by hand (a bank balance
  as of a date, a manual loan balance), it holds exactly from its day until the user changes or
  removes it. Nothing is estimated over it. (Owner: "když to uživatel přetíží a zadá to manuálně,
  tak se nic neodhaduje a spoléhá se na to, že to uživatel aktualizuje.")
- **P3. One rule, one place.** Each class has one Rust function for "value on day D". Today's
  snapshot, the history, the MCP tools and (through the TypeScript mirror pinned by a shared
  fixture) the loans page all use it.

## 3. Owner decisions (2026-10-13)

| # | Decision |
|---|---|
| D1 | Facts win over past **live** rows too, with a guard: on a live row a class is rewritten only when the facts cover every item of that class on that day (§A3). |
| D2 | The first start of the new version rebuilds the whole history once, from the earliest dated fact. Ticker histories are rebuilt from each ticker's first transaction. |
| D3 | Bonds have no purchase date: they keep today's value on every past day, as now. |
| D4 | An account counts nothing on days before its first dated balance (same as `valueAt` in `src/utils/balance-records.ts`). |
| D5 | A loan before its first manual balance follows the estimated path, which is the rule of the loans chart (`src/utils/loan-trajectory.ts`). |
| D6 | A manual loan balance holds its value: no amortization after it, until the user enters another one or removes it. |
| D7 | An extra payment entered while a manual balance applies gives a new manual value from its day: the manual balance minus the payment. |
| D8 | Switching the loan form back to the estimate deletes every manual balance of the loan. The loan, its history included, is then estimated again. |
| D9 | "Ověřit zůstatek výpisem" is removed as a separate action. There is one concept, "Zadat zůstatek k datu" (a manual balance record), on the loan detail and in the loan form. Existing balance checks become manual records. |
| D10 | Crypto history is rebuilt after a CoinGecko key is entered and after every historical crypto write (works through §A5). |

## Part B — Loans: a ledger with manual balances

### B1. Facts

- `loans`: `principal`, `start_date`, current `interest_rate` and `monthly_payment`, `end_date`.
- `loan_events` (ordered by `event_day`, then `created_at`):
  - `extra_payment`: `amount`, optional new `monthly_payment` ("Snížení splátky").
  - `rate_change`: new `rate`, optional new `monthly_payment`, **new columns `previous_rate` and
    `previous_monthly_payment`** (written from now on; NULL on legacy rows).
  - `balance_check`: the **manual balance record** (`amount`). The stored kind stays
    `balance_check`, so the CHECK constraint and wire enum do not change. The UI calls it
    "Ruční zůstatek".
- `loans.balance_anchor_amount` / `balance_anchor_date` become a **mirror of the newest manual
  record** (NULL when there is none), written only by the manual-record service, like
  `bank_accounts.balance` mirrors the newest `bank_balance_records` row. `extra_payment` and
  `rate_change` no longer write them.

### B2. Balance on a day: the ledger walk

A pure function, `loan_balance_at(ledger, day)`, in `services/loan_amortization.rs`, mirrored in
`shared/calculations/loan-amortization.ts` and pinned by new cases in
`shared/calculations/loan-amortization.fixture.json`.

1. `day < start_day` → **0**. This applies to history, and to today for a loan that starts in the
   future.
2. The initial state is mode `Estimated`, balance `principal`, and the segment anchor day is
   `start_day`. Initial terms are the first `rate_change`'s `previous_*` values when present, else
   the loan's current rate and payment (the legacy behaviour).
3. Walk the events dated `<= day` in order. Between events, in `Estimated` mode, apply every
   monthly payment due on or before the next event or `day`. Payments use `amortization_step`;
   due days fall on the segment anchor's day of month (`due_day`). In `Manual` mode nothing is
   applied.
4. `extra_payment`:
   - `Estimated` mode: `balance -= amount`, the event day becomes the segment anchor, and the new
     payment applies when given. This is today's behaviour.
   - `Manual` mode: `balance -= amount` and the mode stays `Manual` (D7).
5. `rate_change`: the terms change.
   - `Estimated` mode: the event day becomes the segment anchor.
   - `Manual` mode: the balance is unchanged.
6. `balance_check` (manual record): `balance = amount`, mode becomes `Manual` (D6).
7. Clamp the balance to `>= 0`. The result is `{ balance, mode, segment_anchor_day, terms }`.

`outstanding_by_currency(conn, today)` and `loan_schedule` use the walk. `amortization::elapsed_to`
remains the inner step for estimated segments.

### B3. Plan ahead (schedule, payoff, what-if)

Start from the walk's state at today:

- `Estimated`: the schedule continues from the segment anchor, as today.
- `Manual`: the schedule starts from `(balance_today, today)` with the current terms. The first
  payment is due on the segment anchor's day of month after today. The future is always an
  estimate; only the actual balance holds.

### B4. Services and commands

`services/loan_events.rs` and `services/loans.rs`:

- **`add_manual_balance(loan_id, day, amount)`**: one record per day, so the same day replaces.
  Re-mirrors the anchor columns.
- **`delete_event`**: deleting any event re-derives the walk; nothing stays from the deleted row.
  Deleting a manual record re-mirrors, and the previous record or the estimate applies again.
  Remove the "Zůstatek, který událost nastavila, zůstane v platnosti" semantics and copy.
- **`add_event(extra_payment | rate_change)`**: store the event only. `rate_change` also writes
  `previous_rate` and `previous_monthly_payment` (the terms in force before it) and updates the
  loan's current terms, as now. Neither writes the anchor columns.
- **The loan form** (`create_loan` / `update_loan` with `balance_anchor_*`):
  - Switch on with `(amount, day)` → upsert the manual record of that day (older records stay).
  - Switch off (both NULL) while manual records exist → delete every manual record of the loan
    (D8).
- **MCP:**
  - `loan_create` with an anchor creates a manual record.
  - `loans_list` keeps `balanceAnchorAmount` and `balanceAnchorDate` (the mirror) and adds
    `balanceMode: "estimated" | "manual"`.
- **Milestone `loan_balance_check`:** its "last checked" day becomes the newest manual record day,
  else the latest of the start day and the event days. The kind id is unchanged; the copy changes
  to "Zadat aktuální zůstatek".

### B5. Wire contract (follow the 7-step procedure in `docs/standards/type-contract.md`)

- `LoanEvent` gains `previousRate: string | null` and `previousMonthlyPayment: string | null`.
- `Loan` gains `events: LoanEvent[]` (oldest first), filled by `list_loans` and `get_loan`, so the TS
  mirror can walk every loan on the list page; plus `balanceMode: 'estimated' | 'manual'`.
- Register the types in `bindings.rs` and regenerate `generated-types.ts`.

### B6. UI (every string in all ten locales)

- **Loan detail:** the action "Ověřit zůstatek výpisem" becomes **"Zadat zůstatek k datu"**. The
  dialog stays the same and keeps the useful line "Odhad počítal s X, rozdíl Y".
  - The events list and the chart show "Ruční zůstatek X"; every manual record can be deleted.
  - The extra-payment dialog in `Manual` mode says "Ruční zůstatek klesne na X".
  - The rate-change dialog in `Manual` mode says the balance stays and only the plan changes.
  - The delete confirmation drops "zůstatek … zůstane v platnosti".
- **Loan form:** the help text becomes: "Moony odhaduje zůstatek podle splátek. Když zadáte
  skutečný zůstatek k datu, platí přesně, dokud ho nezměníte — nic se od něj neodhaduje."
  Switching off shows the warning "Smaže N ručních zůstatků, úvěr se bude znovu odhadovat."
- **Valuation note and status:** the `manual` text reads "Ručně zadaný zůstatek {{amount}} k
  {{date}} — platí, dokud ho nezměníte." The `estimated` text is unchanged.
- **TS:**
  - `src/utils/loan-trajectory.ts`: the past path comes from the walk, and every manual record is
    a step.
  - `src/utils/loans.ts`: `balanceToday` and the payoff day come from B2 and B3.
  - The `LoanFormDialog` estimate uses the walk.

### B7. Migration 019 (append-only in `migrations.rs`; regenerate `schema.snapshot`)

```sql
ALTER TABLE loan_events ADD COLUMN previous_rate TEXT;
ALTER TABLE loan_events ADD COLUMN previous_monthly_payment TEXT;
-- A form-entered anchor (no extra_payment / rate_change / balance_check on its day) becomes a manual record.
INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at)
SELECT <uuid recipe of migration 013>, l.id, 'balance_check', l.balance_anchor_date, l.balance_anchor_amount, l.updated_at
FROM loans l
WHERE l.balance_anchor_amount IS NOT NULL AND l.balance_anchor_date IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM loan_events e WHERE e.loan_id = l.id AND e.event_day = l.balance_anchor_date);
-- The anchor columns mirror the newest manual record.
UPDATE loans SET
  balance_anchor_amount = (SELECT amount FROM loan_events e WHERE e.loan_id = loans.id AND e.kind = 'balance_check' ORDER BY event_day DESC, created_at DESC LIMIT 1),
  balance_anchor_date   = (SELECT event_day FROM loan_events e WHERE e.loan_id = loans.id AND e.kind = 'balance_check' ORDER BY event_day DESC, created_at DESC LIMIT 1);
```

Migration test: a form anchor becomes a record, an extra-payment re-anchor does not, and the
mirror is correct.

**Behaviour change, put it in the release notes:** a loan with a manual balance stops amortizing
after it (D6), so today's debt can be higher than before.

## Part A — History from dated facts

### A1. Value of each class on day D (new `services/history_days.rs`)

Load the facts once per rebuild, then walk the days with per-item cursors (the pattern of
`portfolio_history::load_ticker_series`). Every item reports `(native amount, currency, known)`.

| Class | Rule on day D | `known` |
|---|---|---|
| Savings | Accounts with `exclude_from_balance = 0`. The newest `bank_balance_records.as_of <= D` gives its amount and currency. Before the first record the account is not counted (D4). An account with **no** records (undated balance) carries its current balance. | yes with a record `<= D`; no before the first record; no for undated |
| Loans | `loan_balance_at` (B2) in the loan currency; 0 before the start. | yes, unless the loan has manual records and D is before the first one (the estimated past of an overridden loan, D5) |
| Real estate | Personal or investment by the current `type`. `purchase_date` set and D before it → 0. Otherwise the newest valuation with `valued_at <= D` gives the value; of several on one day the last created wins, as in `dailyValuations`. Without a valuation yet and D on or after `purchase_date`, the purchase price in its currency applies (the `valuationTrace` rule). Without a purchase date and D before the first valuation → not counted. | yes, except "not counted" |
| Other assets | Quantity from `other_asset_transactions` dated `<= D` (buy +, sell −) × `valuations::value_at_day`, falling back to `market_price`. Currency: the valuation's, else the asset's. | yes |
| Bonds | Today's `coupon_value × quantity`, carried (D3). | no |
| Stocks, crypto | `stock_value_history` / `crypto_value_history` through `load_ticker_series` / `value_before` (unchanged). | yes |

A class is `known` on D when every item in it is.

### A2. Coverage

- **Earliest fact day E** is the minimum over: the first transaction of stocks, crypto and other
  assets; the first `bank_balance_records.as_of`; each property's `purchase_date`, else its first
  valuation; each loan's `start_date`. Clamp to `>= 2000-01-01` (`RECALC_FLOOR_TS`). Bonds and
  undated accounts do not define E.
- Every day in `[E, today)` has exactly one row. Backfill rows dated before E are deleted. Live
  rows are never deleted.

### A3. Write rule for a day D < today

- **Backfill or new row:** write all eight breakdowns from A1. For items that are not known,
  undated accounts and bonds carry their current values, and the rest are not counted.
- **Live row:** rewrite only the classes whose `known` flag is true (D1). Stocks and crypto are
  always rewritten, as today. The others keep their recorded values.
- **Today's row** is never touched by a rebuild; `update_todays_snapshot` owns it.

All writes go through `services/portfolio_history.rs` (the single writer): `upsert_snapshot` for
new and backfill rows, `update_classes` for live rows.

### A4. Engine: `rebuild_days(conn, from_day, until_exclusive)`

- Starts at `max(from_day, E)`.
- Writes in `CHUNK_DAYS` chunks, one transaction each, yielding between chunks (the
  `history_recalc` pattern).
- Deletes backfill rows before E.
- Idempotent.
- Replaces:
  - `rebuild_ticker_class`, `create_missing_days` and `aggregate_stock_chunk` / `aggregate_stocks`
    (the interim fix);
  - `recalculate_history_from_date`, `recalculate_asset_history_from_date`,
    `write_recalculated_day`, `historical_transaction_breakdowns`,
    `calculate_asset_breakdown_for_day`;
  - `trigger_historical_recalculation*` and `update_portfolio_*_from_ticker_table`;
  - `breakdowns_for_backfill_day` and the snapshot part of `backfill_missing_snapshots`.

Delete the dead code and list it in `docs/DEPRECATED.md`.

### A5. One background queue (generalise `services/history_recalc.rs`)

- **Jobs:** `StockTicker(ticker, from)`, `CryptoTicker(ticker, from)` and `Days(from)`. The
  earliest date per key is kept.
- **The worker:**
  1. Rebuilds stock tickers (Yahoo, existing code).
  2. Rebuilds crypto tickers. This is the body of `recalculate_crypto_ticker_history_with_policy`;
     the coingecko id is resolved from `crypto_investments`; the CoinGecko window is 365 days.
  3. Runs `rebuild_days(min(from over the batch), today)`.
- **Events:** `history-recalculation` `{ticker, status}` per ticker as now, then
  `recalculation-complete` after each batch.
- **Entry point:** a single helper, `portfolio::schedule_history_rebuild(app, db, recalc, jobs)`.

### A6. Triggers (UI command and MCP path; date = earliest affected day; an edit uses min(old, new))

| Domain | Writes and the date they queue |
|---|---|
| Bank accounts | Create account: opening record day, or E for an undated balance. Update: E when `exclude_from_balance` or the currency changes. Delete account: E. Record or delete balance: record day. CSV import with a statement balance: record day. MCP `bank_account_create`. |
| Stocks | Existing `schedule_stock_history_rebuild` calls become `StockTicker` jobs; stock import; MCP. |
| Crypto | Create, update and delete transaction, and import: `CryptoTicker` (replaces the synchronous `trigger_historical_recalculation_for_crypto_ticker` calls and their `.ok()`). CoinGecko key change: every crypto ticker from its first transaction (replaces the interim `spawn_crypto_history_rebuild`). MCP crypto. |
| Other assets | Create with an initial transaction: transaction day. Transaction create and delete: transaction day. Valuation add and delete: valuation day. Asset delete: its first transaction day, else E. MCP `other_asset_create` and transaction create. |
| Real estate | Create: purchase date, else the first valuation day. Update: min(old, new purchase date), or E on a type or currency change. Delete: E. Valuation add and delete: valuation day. MCP `real_estate_create`. |
| Loans | Create and update: min(old, new start). Delete: start. Event and manual-record add and delete: event day. Switch off: first manual day. MCP `loan_create`. |
| Bonds | Create, update, delete: E. |

- `commands/mcp_refresh.rs` gains the domains `bank-accounts`, `real-estate`, `loans` and `bonds`,
  each queuing `Days`. MCP write tools must emit their domain with the earliest date, so check
  `emit_data_changed` callers in `services/mcp/`.
- Commands take `AppHandle` and `State<HistoryRecalc>` (Tauri injects them; the frontend is
  unchanged).
- `trigger_import_recalculation` maps onto the queue.

### A7. Startup and the one-time rebuild

`start_snapshot_backfill`, called by `SyncProvider` after unlock, becomes "plan and queue" and
returns at once:

1. If `app_config.history_rebuild_version < 1`: queue every stock and crypto ticker from its first
   transaction, plus `Days(E)`. Store version `1` once the days rebuild of that batch succeeded,
   so a failed days rebuild retries at the next start. A ticker the provider keeps rejecting (a
   delisted symbol, a rate limit) is reported `failed`, logged and retried by later writes and the
   gap fill; it does not block the flag.
2. Otherwise, take the first missing day G in `[E, yesterday]`: queue the held tickers from G and
   `Days(G)`.

Check that every history consumer refetches on `recalculation-complete`: `PortfolioTrendCard`
does; the dashboard chart and `use-portfolio` must too.

### A8. Today

`net_worth::compute_breakdowns` keeps valuing today, with loans switched to the ledger (B2).

Consistency test: on a fixture, A1's savings, loans, real estate and other assets for today's day
equal `compute_breakdowns`.

## 4. Known limits and out of scope

- Bonds have no purchase date (D3).
- CoinGecko free and demo tiers return 365 days. Older crypto days use the last known price, else
  no value.
- Legacy `rate_change` rows without `previous_*`: the estimated path before them uses the current
  rate (as today).
- The bank-account detail line interpolates between records; history uses steps (`valueAt`).
- `src/utils/estimated-series.ts` and `EstimatedLegend.tsx` are unused (dead code). Flag them;
  don't touch them.

## 5. Documentation

- **New ADR 0016**, "History days derive from dated facts; manual values hold". It supersedes the
  carried-statics rule.
- `docs/architecture/database.md`: `portfolio_metrics_history`, `loans`, `loan_events`, migration
  019.
- `docs/architecture/overview.md`: the "History recalculation" section.
- `docs/DEPRECATED.md`: the removed functions.

## 6. Testing and verification

- **Rust, A1:** one service test per class rule (in-memory DB, hand-written schema; see AGENTS.md).
- **Rust, `rebuild_days` scenarios:**
  - a fresh profile (today's live row only) plus a balance record a year back: past rows hold it;
  - the live-row guard: an account without a record on a live day keeps the recorded savings;
  - deleting the earliest fact deletes the earlier backfill rows;
  - idempotence;
  - chunked output equals one pass;
  - the today-consistency check (A8).
- **Ledger fixture cases, Rust and TS:**
  - estimated only; manual hold; extra payment in both modes; rate change in both modes;
  - deleting a manual record restores the previous one; switch off; before start = 0;
  - the manual-mode plan ahead.
- **Other tests:** migration 019; the queue (the job kinds coalesce).
- **Gates:**
  ```bash
  npm run lint && npm run typecheck && npm test
  cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
  cd src-tauri && cargo test generate_bindings -- --ignored
  ```
- **Live check:** run the demo app from the worktree on port 1421; see the memory notes on dev
  ports and the demo profile.
  - A bank balance a year back shows in the history.
  - A stock import draws history from the first trade.
  - Crypto after the key, and a historical crypto transaction after the key.
  - A loan with a manual balance holds it.
- **Before release:** a dry run on a copy of the owner's real profile, with history before and
  after compared.
