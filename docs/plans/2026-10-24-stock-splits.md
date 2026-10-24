# Stock splits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect stock splits from Yahoo, tell the user, and on confirmation rewrite the stored
pre-split transactions into current share units; offer the same conversion in the CSV import
wizard when a split falls inside the file's trade range.

**Architecture:** A new `stock_splits` service owns the split record (`stock_splits` table), the
exact-decimal rewrite and the "already adjusted?" classification. The three Yahoo fetch paths
(history rebuild, dividend refresh, a new daily check in the quote refresh) feed detected splits
into it; a milestone builder and a detail-page banner surface pending ones; six commands resolve
them. The import wizard gets a backend-parsed split check and a per-split convert/keep choice that
`simulate` and `import` honour.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher, `yahoo_finance_api` 5, `chrono`), React 19 +
TypeScript, TanStack Query, i18next, Vitest.

**Spec:** `docs/specs/2026-10-24-stock-splits-design.md`. Decisions (D1–D24) and sections (§5,
§6 …) below refer to it. The owner approved the design in chat on 2026-10-24.

## Global Constraints

- Work in the sibling worktree `../Moony-tauri-splits` on branch `feat/stock-splits`. Never
  commit in the shared checkout; check `git branch --show-current` before every commit.
- The ten rules of `AGENTS.md`. Read `docs/standards/rust-backend.md`,
  `docs/standards/type-contract.md`, `docs/standards/i18n.md`, `docs/standards/testing.md`,
  `docs/DEPRECATED.md` before coding.
- Money and quantities are TEXT; timestamps unix seconds; PKs TEXT UUIDs; no `.unwrap()`
  outside tests; services take `&Connection`; commands are thin; errors are `AppError`.
- Every new user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`,
  `pl`, `sv`, `ja`, `zh`) in the same commit; `npm test` runs `src/i18n/locales.test.ts`.
- After any change to a type registered in `bindings.rs`: `cd src-tauri && cargo test
  generate_bindings -- --ignored`, then hand-mirror into `shared/schema.ts`.
- Gate per task before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run typecheck && npm test
```

- One Conventional Commit per task, ending with
  `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Never type a password into any app window that does not greet "Alex" (demo profile).
  Demo runs from the worktree use port 1422 and a worktree copy of `demo-home`.

## Review Focus

1. A reverse split (`1:10`) on a history with sells: after D3 rounding the stored history must
   still validate (test in Task 2 and Task 4).
2. A ticker whose file rows are already adjusted while Moony holds a pending split: `keep`
   leaves the stored split pending and imports the rows unchanged (test in Task 11).
3. Transactions in pence-quoted tickers (`BARC.L`): the close used by D7 is in pounds, the
   stored price too; a mismatch currency gives `unknown`, never a wrong verdict (Task 3, 12).
4. A split detected for a ticker whose only transactions are dated on or after the ex-date is
   never stored or shown (Task 4, 6).
5. Apply with `stock_price_overrides.updated_at` after the ex-date leaves the override alone
   (Task 4).

---

## Phase A — backend core

### Task 1: Migration 023 `stock_splits` + `stock_data.splits_checked_at`

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (append `MIGRATION_023`, Vec entry, test module)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (table catalog: `stock_splits`, the new column)

**Interfaces:**
- Produces: the table of §5 exactly as written there (columns, CHECKs, UNIQUE, index) and
  `ALTER TABLE stock_data ADD COLUMN splits_checked_at INTEGER`.

- [ ] **Step 1: Write the failing migration test** in `migrations.rs` next to
  `stock_import_batch_tests`, module `stock_splits_tests`:

```rust
#[test]
fn migration_023_creates_stock_splits_and_the_check_column() {
    let conn = database_before("023_stock_splits"); // same helper style as database_before_012
    run_migrations(&conn).expect("migrate");
    let unique: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND tbl_name='stock_splits' AND sql LIKE '%UNIQUE%' OR name='sqlite_autoindex_stock_splits_1'",
        [], |r| r.get(0)).unwrap();
    assert!(unique >= 1);
    conn.execute("INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, status) VALUES ('a','NVDA',1718000000,'10','1','yahoo','pending')", []).unwrap();
    assert!(conn.execute("INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, status) VALUES ('b','NVDA',1718000000,'10','1','yahoo','pending')", []).is_err(), "UNIQUE(ticker, effective_day)");
    assert!(conn.execute("INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, status) VALUES ('c','X',1,'2','1','guess','pending')", []).is_err(), "source CHECK");
    conn.execute("UPDATE stock_data SET splits_checked_at = 1", []).unwrap();
}
```

- [ ] **Step 2: Run it** — `cd src-tauri && cargo test migration_023 -- --nocapture`. Expected: FAIL
  (unknown migration / no such table).
- [ ] **Step 3: Add `pub(crate) const MIGRATION_023: &str`** with the §5 SQL and the Vec entry
  `("023_stock_splits", MIGRATION_023)`.
- [ ] **Step 4: Run** `cargo test migration_023` → PASS; `cargo test regenerate_schema_snapshot
  -- --ignored`; `cargo test db::` → PASS.
- [ ] **Step 5: Document** the table and column in `docs/architecture/database.md` (stock rows).
- [ ] **Step 6: Commit** `feat(db): stock_splits table and stock_data.splits_checked_at (migration 023)`.

### Task 2: Exact decimal scaling (`stock_splits/scale.rs`)

**Files:**
- Create: `src-tauri/src/services/stock_splits/mod.rs` (module declarations only for now:
  `pub mod scale; pub mod classify; pub mod types;` — the empty modules are created by their tasks)
- Create: `src-tauri/src/services/stock_splits/scale.rs`
- Modify: `src-tauri/src/services/mod.rs` (`pub mod stock_splits;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Rounding { HalfUp, Down }
  /// `text × mul / div`, exact base-ten digits on the canonical text; at most 8 fractional
  /// digits, rounded per `rounding` when the quotient does not terminate; result canonical.
  /// Errors: `validation.amountInvalid` for non-numeric or negative text, `div == 0`, `mul == 0`.
  pub fn scale_amount(text: &str, mul: u64, div: u64, rounding: Rounding) -> Result<String>;
  ```

- [ ] **Step 1: Write the failing tests** in `scale.rs`:

```rust
#[test] fn multiplies_exactly() { assert_eq!(scale_amount("15", 4, 1, Rounding::HalfUp).unwrap(), "60"); assert_eq!(scale_amount("2.5", 4, 1, Rounding::HalfUp).unwrap(), "10"); }
#[test] fn divides_exactly() { assert_eq!(scale_amount("950", 1, 10, Rounding::HalfUp).unwrap(), "95"); assert_eq!(scale_amount("185.5", 1, 4, Rounding::HalfUp).unwrap(), "46.375"); }
#[test] fn reverse_split_quantity() { assert_eq!(scale_amount("30", 1, 10, Rounding::HalfUp).unwrap(), "3"); }
#[test] fn non_terminating_half_up_and_down() {
    assert_eq!(scale_amount("1", 1, 3, Rounding::HalfUp).unwrap(), "0.33333333");
    assert_eq!(scale_amount("2", 1, 3, Rounding::HalfUp).unwrap(), "0.66666667");
    assert_eq!(scale_amount("2", 1, 3, Rounding::Down).unwrap(), "0.66666666");
    assert_eq!(scale_amount("0.5", 1, 3, Rounding::HalfUp).unwrap(), "0.16666667");
}
#[test] fn the_1_3_oversold_example_stays_valid() {
    // buys 1 + 1 (half-up) ≥ sell 2 (down): 0.33333333 + 0.33333333 ≥ 0.66666666
    let b = scale_amount("1", 1, 3, Rounding::HalfUp).unwrap(); let s = scale_amount("2", 1, 3, Rounding::Down).unwrap();
    assert!(b.parse::<f64>().unwrap() * 2.0 >= s.parse::<f64>().unwrap());
}
#[test] fn canonical_output_and_large_values() {
    assert_eq!(scale_amount("0.10", 2, 1, Rounding::HalfUp).unwrap(), "0.2");
    assert_eq!(scale_amount("123456789012.5", 3, 2, Rounding::HalfUp).unwrap(), "185185183518.75");
}
#[test] fn rejects_bad_input() { for t in ["abc", "-1", ""] { assert!(scale_amount(t, 2, 1, Rounding::HalfUp).is_err()); } assert!(scale_amount("1", 0, 1, Rounding::HalfUp).is_err()); assert!(scale_amount("1", 1, 0, Rounding::HalfUp).is_err()); }
```

- [ ] **Step 2: Run** `cargo test stock_splits::scale` → FAIL (module missing).
- [ ] **Step 3: Implement** `scale_amount`: run the text through `dedup::canonical_amount`,
  split integer/fraction digits, multiply the digit string by `mul` as `u128` long
  multiplication (or digit-by-digit to avoid overflow on very long inputs), then long-divide by
  `div` producing up to 8 fractional digits plus one guard digit for `HalfUp` (and a
  "remainder non-zero" flag for `Down`), strip trailing zeros, `canonical_amount` again.
- [ ] **Step 4: Run** `cargo test stock_splits::scale` → PASS.
- [ ] **Step 5: Commit** `feat(stocks): exact decimal scaling for stock splits`.

### Task 3: Classification (`stock_splits/classify.rs`)

**Files:**
- Create: `src-tauri/src/services/stock_splits/classify.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
  #[serde(rename_all = "lowercase")]
  pub enum Verdict { Adjusted, Unadjusted, Unknown }
  /// D7 for one transaction. `close` is `(price, currency)` of the trade day when known.
  pub fn classify(price: f64, currency: &str, close: Option<(f64, &str)>, n: u64, d: u64) -> Verdict;
  /// D7 for the split: unadjusted / adjusted only when no transaction says the opposite.
  pub fn verdict_of(verdicts: &[Verdict]) -> Verdict;
  pub(crate) const BAND_LOW: f64 = 0.7; pub(crate) const BAND_HIGH: f64 = 1.4;
  ```
  `Verdict` lives here and is re-exported from `types.rs` as `StockSplitVerdict` (same type).

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn pre_split_price_near_factor_times_close_is_unadjusted() { assert_eq!(classify(950.0, "USD", Some((96.0, "USD")), 10, 1), Verdict::Unadjusted); }
#[test] fn price_near_close_is_adjusted() { assert_eq!(classify(96.0, "USD", Some((95.0, "USD")), 10, 1), Verdict::Adjusted); }
#[test] fn out_of_both_bands_is_unknown() { assert_eq!(classify(300.0, "USD", Some((95.0, "USD")), 10, 1), Verdict::Unknown); }
#[test] fn reverse_split_bands() { assert_eq!(classify(9.5, "USD", Some((95.0, "USD")), 1, 10), Verdict::Unadjusted); assert_eq!(classify(96.0, "USD", Some((95.0, "USD")), 1, 10), Verdict::Adjusted); }
#[test] fn overlapping_bands_are_unknown() { assert_eq!(classify(150.0, "USD", Some((100.0, "USD")), 3, 2), Verdict::Unknown); assert_eq!(classify(100.0, "USD", Some((100.0, "USD")), 3, 2), Verdict::Unknown); }
#[test] fn missing_close_or_other_currency_is_unknown() { assert_eq!(classify(950.0, "USD", None, 10, 1), Verdict::Unknown); assert_eq!(classify(950.0, "USD", Some((96.0, "GBP")), 10, 1), Verdict::Unknown); }
#[test] fn split_verdict() {
    use Verdict::*;
    assert_eq!(verdict_of(&[Unadjusted, Unknown]), Unadjusted);
    assert_eq!(verdict_of(&[Adjusted, Unknown]), Adjusted);
    assert_eq!(verdict_of(&[Adjusted, Unadjusted]), Unknown);
    assert_eq!(verdict_of(&[Unknown]), Unknown);
    assert_eq!(verdict_of(&[]), Unknown);
}
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** (bands overlap when `0.5 < n/d < 2`, which
  includes `n == d`; then everything is `Unknown`). **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(stocks): classify pre-split transactions as adjusted or not`.

### Task 4: Split records service (`stock_splits/types.rs`, `mod.rs`)

**Files:**
- Create: `src-tauri/src/services/stock_splits/types.rs`
- Modify: `src-tauri/src/services/stock_splits/mod.rs`
- Modify: `src-tauri/src/bindings.rs` (register `StockSplit`, `InsertStockSplit`,
  `StockSplitVerdict`, `StockSplitApplyResult`, `StockSplitStatus`, `StockSplitSource`)
- Modify: `shared/generated-types.ts` (regenerated), `shared/schema.ts` (§6 types + a zod
  `insertStockSplitSchema` beside `insertInvestmentTransactionSchema`)
- Modify: `src/i18n/locales/*/common.json` (the six `validation.stockSplit*` keys of §8)

**Interfaces:**
- Consumes: `scale_amount`, `Verdict`, `verdict_of`, `classify` (Tasks 2–3);
  `investments::{ensure_valid_holdings_history, recalculate_investment_metrics}`;
  `dedup::canonical_amount`.
- Produces (all `pub`, in `stock_splits::`):
  ```rust
  // types.rs — per-field #[serde(rename)], specta::Type
  pub struct StockSplit { id, ticker, effective_day, numerator: String, denominator: String, source: StockSplitSource, status: StockSplitStatus, detected_at, resolved_at: Option<i64>, applied_count: Option<i64>, pre_split_count: i64, verdict: Verdict }
  pub enum StockSplitStatus { Pending, Applied, Dismissed }   // serde lowercase
  pub enum StockSplitSource { Yahoo, Manual, Import }         // serde lowercase
  pub struct InsertStockSplit { ticker, effective_day: i64, numerator: String, denominator: String }
  impl InsertStockSplit { pub fn validate(&self) -> Result<()> }   // D11; ticker uppercased+trimmed by the caller
  pub struct StockSplitApplyResult { split: StockSplit, rewritten: i64, earliest_day: Option<i64> }
  /// One split as a Yahoo response reports it, already reduced to integers and the UTC day.
  pub struct DetectedSplit { pub effective_day: i64, pub numerator: u64, pub denominator: u64 }
  pub fn ratio_parts(numerator: &str, denominator: &str) -> Result<(u64, u64)>; // validation.stockSplitRatioInvalid

  // mod.rs
  pub fn list_for_ticker(conn: &Connection, ticker: &str) -> Result<Vec<StockSplit>>;      // D6 filter, verdict from stock_value_history (D7, D18), newest first
  pub fn get(conn: &Connection, id: &str) -> Result<StockSplit>;                            // AppError::NotFound
  pub fn record_detected(conn: &Connection, ticker: &str, splits: &[DetectedSplit], source: StockSplitSource) -> Result<Vec<StockSplit>>; // D5/D6: inserts pending rows that are new and relevant; returns ONLY the inserted rows
  pub fn apply(conn: &Connection, id: &str) -> Result<StockSplitApplyResult>;              // D10, pending → applied
  pub fn revert(conn: &Connection, id: &str) -> Result<StockSplitApplyResult>;             // D10 inverse, applied → pending
  pub fn dismiss(conn: &Connection, id: &str) -> Result<StockSplit>;                        // pending → dismissed
  pub fn record_manual(conn: &Connection, data: &InsertStockSplit) -> Result<StockSplitApplyResult>; // D11: upsert-or-reuse then apply
  pub fn delete_manual(conn: &Connection, id: &str) -> Result<()>;                          // manual + not applied only
  pub fn applied_before(conn: &Connection, ticker: &str, day: i64) -> Result<Vec<StockSplit>>; // D20: applied splits with effective_day > day
  pub(crate) fn rewrite_pre_split_rows(conn: &Connection, ticker: &str, effective_day: i64, mul: u64, div: u64) -> Result<i64>; // the D1/D3 rewrite + side data (D10), shared with Task 11
  ```
  The transaction boundary: the commands call these inside `db.with_conn`; `apply`/`revert`/
  `record_manual` open a SAVEPOINT (`conn.execute_batch("SAVEPOINT split_apply")` … RELEASE /
  ROLLBACK TO) so a failing `ensure_valid_holdings_history` leaves nothing behind.

- [ ] **Step 1: Write the failing service tests** in `mod.rs` with `setup_test_db()` creating
  `stock_investments`, `investment_transactions` (with `created_at`, `import_batch_id`,
  `external_id`), `stock_splits` (§5 SQL), `stock_value_history`, `stock_price_overrides`,
  `watched_stocks` (with `target_price`, `updated_at`), `dividend_overrides`, `dividend_data`
  and a helper `tx(conn, ticker, kind, qty, price, day)`:

```rust
#[test] fn record_detected_stores_only_relevant_new_splits_and_reports_them() {
    // NVDA buys on day 100 and day 400; splits on day 200 (relevant) and day 50 (before the first buy)
    let new = record_detected(&conn, "NVDA", &[split(200, 10, 1), split(50, 4, 1)], StockSplitSource::Yahoo).unwrap();
    assert_eq!(new.len(), 1); assert_eq!(new[0].status, StockSplitStatus::Pending); assert_eq!(new[0].pre_split_count, 1);
    let again = record_detected(&conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo).unwrap();
    assert!(again.is_empty(), "an existing row is not reported again");
}
#[test] fn record_detected_never_changes_an_existing_status() { /* dismiss, then record_detected again → still dismissed */ }
#[test] fn list_hides_splits_without_pre_split_rows() { /* delete the day-100 buy → list_for_ticker is empty, row still in table */ }
#[test] fn verdict_from_value_history() { /* close 96 on day 100 (adjusted series), buy at 950 → Unadjusted; buy at 96 → Adjusted; no history row → Unknown */ }
#[test] fn apply_rewrites_only_pre_split_rows_and_keeps_cost_basis() {
    // buys: day 100 qty 3 @ 950 (pre), day 400 qty 20 @ 120 (post); split 10:1 on day 200
    let r = apply(&conn, &id).unwrap();
    assert_eq!(r.rewritten, 1);
    assert_eq!(row(&conn, day 100), ("30", "95")); assert_eq!(row(&conn, day 400), ("20", "120"));
    assert_eq!(r.split.status, StockSplitStatus::Applied); assert_eq!(r.split.applied_count, Some(1));
    assert_eq!(r.earliest_day, Some(day(100)));
    assert_eq!(quantity_of_position(&conn, "NVDA"), "50"); // recalculate_investment_metrics ran
}
#[test] fn apply_refuses_a_non_pending_split() { /* apply twice → Err validation.stockSplitNotPending */ }
#[test] fn apply_scales_side_data_older_than_the_ex_date_only() {
    // override 1000 updated_at < effective_day → 100 (updated_at unchanged); watchlist target 1200 updated_at > effective_day → untouched; dividend_overrides 20 (old) → 2; dividend_data row deleted
}
#[test] fn apply_with_sells_after_a_reverse_split_stays_valid() { /* buys 1,1 then sell 2, split 1:3 → Ok, quantities 0.33333333 ×2, sell 0.66666666 */ }
#[test] fn apply_aborts_when_the_history_is_oversold() {
    // Insert an already-oversold history directly (buy 1 on day 100, sell 2 on day 150), split 4:1 on day 200:
    // ensure_valid_holdings_history fails → Err("validation.stockSplitOversold"), both rows and the split row unchanged.
}
#[test] fn revert_restores_terminating_ratios_exactly() { /* apply 4:1 on "15"/"165.3" → "60"/"41.325" → revert → "15"/"165.3", status Pending */ }
#[test] fn dismiss_and_delete_rules() { /* dismiss pending → Dismissed; delete_manual on yahoo row → Err stockSplitNotDeletable; manual pending → deleted */ }
#[test] fn record_manual_reuses_a_detected_row() { /* yahoo pending on (NVDA, day 200) + record_manual same day → same id, status Applied, source stays Yahoo */ }
#[test] fn record_manual_validation() { /* n == d, zero, > 1_000_000, non-integer text, future day → Err keys */ }
#[test] fn applied_before_lists_applied_splits_after_the_day() { /* applied split day 200: applied_before(day 100) → 1, applied_before(day 300) → 0 */ }
```

- [ ] **Step 2: Run** `cargo test stock_splits::` → FAIL.
- [ ] **Step 3: Implement** `types.rs` and `mod.rs`. Notes that the tests do not pin:
  `effective_day` of a `DetectedSplit` is already a UTC midnight (the caller floors it);
  `pre_split_count` is `COUNT(*) FROM investment_transactions WHERE ticker=? AND
  transaction_date < effective_day`; the verdict query joins `stock_value_history` on
  `recorded_at = transaction_date` and `ticker`, compares currencies, and is computed only for
  `pending` rows (others `Unknown`); `record_detected` uses `INSERT … ON CONFLICT(ticker,
  effective_day) DO NOTHING` and reads back `changes()`; the rewrite updates rows one by one
  (`SELECT id, type, quantity, price_per_unit` → `scale_amount` with `HalfUp` for buys and
  prices, `Down` for sell quantities → `UPDATE`); side-data scaling uses `HalfUp` and
  `canonical_amount`; `earliest_day` is the ticker's first transaction day (`MIN`).
- [ ] **Step 4: Run** `cargo test stock_splits::` → PASS; `cargo test generate_bindings --
  --ignored`; mirror §6 into `shared/schema.ts`; add the `validation.stockSplit*` strings to
  all ten `common.json`; `npm test` → PASS.
- [ ] **Step 5: Commit** `feat(stocks): stock split records, apply/revert/dismiss service`.

### Task 5: Yahoo detection in the three fetch paths + `REBUILD_VERSION` 4

**Files:**
- Modify: `src-tauri/src/services/price_api.rs`
- Modify: `src-tauri/src/services/history_recalc.rs`
- Modify: `src-tauri/src/services/history_plan.rs` (`REBUILD_VERSION = 4`)
- Modify: `src-tauri/src/commands/price_api.rs`
- Create: `src-tauri/src/services/fixtures/yahoo_chart_aapl_split.json` (a real captured
  `v8/finance/chart` payload for AAPL covering 2020-08-31 with its `events.splits` block; if the
  implementer cannot capture one, build it from the known shape: `"splits": {"1598880600":
  {"date": 1598880600, "numerator": 4, "denominator": 1, "splitRatio": "4:1"}}` and say so in
  the file's sibling `README`)

**Interfaces:**
- Consumes: `stock_splits::{record_detected, DetectedSplit, StockSplitSource}`;
  `history_recalc::{rebuild_jobs, schedule}`; `portfolio::schedule_history_rebuild`.
- Produces in `price_api.rs`:
  ```rust
  pub struct HistoricalSeries { pub prices: Vec<HistoricalPrice>, pub splits: Vec<DetectedSplit> }
  pub(crate) fn chart_splits(response: &YResponse) -> Vec<DetectedSplit>;   // D2: integers, UTC day, split_ratio cross-check, drops + logs disagreements
  pub async fn get_historical_stock_series_yahoo(tickers: &[String], start: i64, end: i64, strict: bool) -> Result<HashMap<String, HistoricalSeries>>;
  // get_historical_stock_prices_yahoo / _strict keep their signatures (map the series to prices)
  pub struct StockPriceRefreshResult { …, #[serde(skip)] pub new_splits: Vec<String> }  // tickers with a NEW pending split (D19)
  pub struct DividendRefresh { pub results: Vec<DividendResult>, pub new_splits: Vec<String> } // refresh_dividends returns this
  ```
  In `history_recalc.rs`: `rebuild_stock_ticker_with_policy` calls `record_detected(conn, ticker,
  &series.splits, Yahoo)` after a successful fetch; when it returns new rows **and** the
  fetch's `from_day` is later than the ticker's first transaction day, it calls
  `schedule(state, db, …, vec![Job::StockTicker(ticker, first_day)], None)` (D18). The
  commands `refresh_stock_prices` and `refresh_dividends` queue `rebuild_jobs(conn,
  &new_splits)` like `rebuild_history_for_unit_changes` (extend that helper to take both lists).

- [ ] **Step 1: Write the failing tests** in `price_api.rs`'s test module:

```rust
#[test] fn chart_splits_reads_the_events_block() {
    let response: YResponse = serde_json::from_str(include_str!("fixtures/yahoo_chart_aapl_split.json")).unwrap();
    let splits = chart_splits(&response);
    assert_eq!(splits, vec![DetectedSplit { effective_day: day(2020, 8, 31), numerator: 4, denominator: 1 }]);
}
#[test] fn chart_splits_drops_a_disagreeing_ratio_text() { /* mutate splitRatio to "1:4" → empty */ }
#[test] fn chart_splits_drops_non_integer_parts() { /* numerator 1.5 → empty */ }
#[test] fn series_carries_prices_and_splits() { /* historical_stock_series(&response, "AAPL", false) → prices non-empty, splits len 1 */ }
```
  and in `history_recalc.rs`: a unit test for the new pure helper
  `needs_full_rebuild(new_rows: usize, from_day: i64, first_day: Option<i64>) -> Option<i64>`
  (`Some(first_day)` only when `new_rows > 0 && first_day < from_day`).

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** The D19 check inside the quote loop:
  after a stored quote, read `splits_checked_at` (query in the same `with_conn`); when `None`
  or older than `now - 86_400`, `provider.get_quote_range(ticker, "1d", "1mo")`, `chart_splits`,
  `record_detected(…, Yahoo)`, `UPDATE stock_data SET splits_checked_at = now`; a failure of
  that extra request is logged and does not fail the ticker. `refresh_dividends` calls
  `chart_splits` on the response it already has.
- [ ] **Step 4: Run** `cargo test price_api` and `cargo test history_recalc` → PASS; `cargo
  clippy` → clean. Bump `REBUILD_VERSION` to 4 and fix its test if one pins the value.
- [ ] **Step 5: Commit** `feat(stocks): detect stock splits from Yahoo responses and a daily check`.

### Task 6: Milestone kind `stock_split`

**Files:**
- Create: `src-tauri/src/services/milestones/stock_splits.rs`
- Modify: `src-tauri/src/services/milestones/mod.rs` (builder list), `test_support.rs`
  (add `stock_splits`, `investment_transactions`, `stock_investments` to the schema if absent)
- Modify: `src-tauri/src/models/milestones.rs` (`MILESTONE_KINDS` → 16 entries, doc comment)
- Modify: `shared/schema.ts` (`MilestoneKind` union), `src/utils/milestones.ts` (`GROUP_OF:
  stock_split: 'targets'`, `INFO_KINDS`, `milestoneHref` → `/stocks/${id}`),
  `src/hooks/use-milestone-sub-line.ts`, `src/mobile/components/AgendaRow.tsx` (treat
  `stock_split` like `watch_target` for the chip: "today"),
  `src/i18n/locales/*/milestones.json` (`sub.stock_split`, the `targets` group hint mentions
  splits), settings card label if kinds are listed there.

**Interfaces:**
- Consumes: `stock_splits::list_for_ticker` (or a direct query with the D6 filter).
- Produces: `pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>>` →
  one row per pending relevant split: `kind "stock_split"`, `key "stock_split:<id>"`,
  `source_id = stock_investments.id` of the ticker (skip when no position), `title = ticker`,
  `tone = info`, `stage = now`, `count = pre_split_count`, `direction = "n:d"`,
  `action_day = effective_day`.

- [ ] **Step 1: Failing tests** in `stock_splits.rs`: `pending_split_with_pre_split_rows_is_listed`
  (asserts key, kind, count, direction, source_id), `applied_or_dismissed_or_irrelevant_are_not`,
  and in `mod.rs` the existing kind-count test (`MILESTONE_KINDS`) updated.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** builder + registrations + frontend plumbing +
  locales (`sub.stock_split`: cs "Split {{ratio}} k {{date}} · {{count}} transakcí před
  splitem" with `_one/_few/_other` forms where the locale uses them).
- [ ] **Step 4: Run** `cargo test milestones`, `npm run typecheck`, `npm test` → PASS.
- [ ] **Step 5: Commit** `feat(milestones): stock split notice`.

### Task 7: Commands, API wrapper, hooks

**Files:**
- Create: `src-tauri/src/commands/stock_splits.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!`)
- Modify: `src/lib/tauri-api.ts` (`stockSplitsApi`)
- Create: `src/hooks/use-stock-splits.ts`, `src/hooks/use-stock-split-mutations.ts`
- Modify: `src/hooks/use-investment-mutations.ts` (export `STOCK_TRANSACTION_KEYS`, D23)

**Interfaces:**
- Produces commands (all `#[tauri::command] pub async fn`):
  `list_stock_splits(db, ticker: String) -> Vec<StockSplit>`,
  `apply_stock_split(db, recalc, app, id: String) -> StockSplitApplyResult`,
  `revert_stock_split(db, recalc, app, id) -> StockSplitApplyResult`,
  `dismiss_stock_split(db, id) -> StockSplit`,
  `record_stock_split(db, recalc, app, data: InsertStockSplit) -> StockSplitApplyResult`,
  `delete_stock_split(db, id) -> ()`.
  Write commands: `db.with_conn(|conn| service)` → `portfolio::update_todays_snapshot(&db).await.ok()`
  → `schedule_history_rebuild(&app, &db, &recalc, vec![Job::StockTicker(ticker, earliest_day)])`
  when `earliest_day` is `Some` → `app.emit("recalculation-complete", ())`.
  TS: `stockSplitsApi = { list(ticker), apply(id), revert(id), dismiss(id), record(data), remove(id) }`;
  `useStockSplits(ticker)` → query `['stock-splits', ticker]`;
  `useStockSplitMutations()` → `{ apply, revert, dismiss, record, remove }` with the D21 refresh
  (`priceApi.refreshStockPrices(true)` then `invalidateAfterPriceRefresh`) after apply/revert/
  record, the toast with "Vrátit" (undo → revert) after apply, invalidations of §4.2.

- [ ] **Step 1:** add the commands, register, run `cargo clippy --all-targets -- -D warnings`
  (commands are untested by policy).
- [ ] **Step 2:** add the TS wrapper and hooks; `npm run typecheck && npm run lint` → PASS.
  Toast strings in all ten `stocks.json` (`splits.toast.applied`, `splits.toast.reverted`,
  `splits.toast.dismissed`, `splits.toast.recorded`, `splits.toast.deleted`, `splits.undo`).
- [ ] **Step 3: Commit** `feat(stocks): stock split commands and hooks`.

## Phase B — detail page

### Task 8: Banner, dialog, hints on the stock detail page

**Files:**
- Create: `src/components/stocks/StockSplitBanner.tsx`, `src/components/stocks/StockSplitsDialog.tsx`
- Modify: `src/pages/StockDetail.tsx` (banner under `ConversionQualityNote`; menu item
  "Zaznamenat split" opening the dialog; an "applied splits" line above the ledger when any)
- Modify: `src/components/stocks/BuyInvestmentModal.tsx`, `SellInvestmentModal.tsx`,
  `EditTransactionModal.tsx` (D20 hint from `useStockSplits(ticker)`:
  `applied.find(s => s.status === 'applied' && dateDay < s.effectiveDay)`)
- Modify: `src/i18n/locales/*/stocks.json` (`splits.*` namespace: banner title/body per
  verdict, buttons, dialog title, columns, statuses, form labels, `splits.hint`, menu item)
- Modify: `src/utils/stock-splits.ts` (create): `ratioText(n, d)`, `preSplitHint(splits, day)`
  pure helpers + `src/utils/stock-splits.test.ts`.

**Interfaces:**
- Consumes: Task 7's hooks and `StockSplit`.
- Banner (D9): one per pending split; title "Split {ratio} k {date}"; body by verdict
  (`unadjusted`: "{count} transakcí před splitem vypadá v původních jednotkách."; `adjusted`:
  "… vypadá už přepočtených."; `unknown`: "Moony nedokáže poznat, zda jsou transakce před
  splitem přepočtené."); primary button per D9; secondary the other; `unknown` → both
  `variant="outline"`.
- Dialog: table (date, ratio, source, status, count, actions per status: pending → apply /
  dismiss; applied → revert; dismissed → apply; manual & not applied → delete) + form (date
  input, numerator, denominator numeric, submit → `record`). Design per
  `docs/design-system/README.md` and the existing modals in `src/components/stocks/`.

- [ ] **Step 1: Failing tests** for the pure helpers (`ratioText("10","1") === "10:1"`,
  `preSplitHint` picks the applied split whose day is after the entered day, none otherwise).
- [ ] **Step 2:** build the components and wiring; **Step 3:** `npm run lint && npm run
  typecheck && npm test` → PASS.
- [ ] **Step 4: Verify in the app** (worktree demo run on port 1422, see Task 14 for the
  recipe): open the NVDA detail, see the banner, apply, see 30 + 20 shares, toast, revert.
- [ ] **Step 5: Commit** `feat(stocks): split banner, splits dialog and pre-split hints`.

## Phase C — import wizard

### Task 9: The split row in the file (D15)

**Files:**
- Modify: `src-tauri/src/services/stock_import/columns.rs` (`SPLIT_WORDS`, `TypeValueAction::SplitRow`
  or a sibling `fn is_split_value(value: &str) -> bool`), `parse.rs` (`ROW_SPLIT =
  "importWizard.row.splitRow"`, the existing test's expectation for `split` changes to it)
- Modify: `src/components/stocks/import/row-messages.ts` (+ test), `import-config.ts`
  (`SKIPPED_KEYS` gains the key), `src/i18n/locales/*/stocks.json` (`importWizard.row.splitRow`:
  cs "Řádek splitu — Moony ho nepřevádí, zkontrolujte množství před splitem")

- [ ] **Step 1: Failing tests:** `columns.rs` `split_values_are_recognised` for `"Stock split
  open"`, `"split"`, `"Aktiensplit"`, `"podział"`; `parse.rs` expectation updated;
  `row-messages.test.ts` renders the key.
- [ ] **Step 2–4:** implement, run `cargo test stock_import`, `npm test` → PASS.
- [ ] **Step 5: Commit** `feat(stock-import): name split rows in the skipped list`.

### Task 10: Override `splits` + scaling in `simulate` (D13)

**Files:**
- Modify: `src-tauri/src/services/stock_import/types.rs` (`StockSplitOverride { effective_day,
  numerator, denominator, action: StockSplitAction }`, `StockSplitAction { Convert, Keep }`,
  `StockInstrumentOverride.splits: Vec<StockSplitOverride>` with `#[serde(default)]`,
  `StockImportCounts.converted`, `StockImportResult.converted`; `config.validate()` checks the
  ratio parts with `stock_splits::ratio_parts`)
- Modify: `simulate.rs` (`SimulatedTrade.converted: bool`; scaling of file trades; a scaled copy
  of `TickerState.stored` when a `Convert` split is `pending` for the ticker — see D14),
  `preview.rs` (`counts.converted`)
- Modify: `bindings.rs`, `generated-types.ts`, `shared/schema.ts`

**Interfaces:**
- Produces: `simulate` scales `quantity *= n/d`, `price *= d/n` (f64) for every file trade of
  an instrument dated `< effective_day` of each `Convert` split, before pricing/dedup/sell
  checks; `SimulatedTrade.converted = true` for those rows. A pure helper
  `pub(crate) fn convert_factor(splits: &[StockSplitOverride], day: i64) -> f64` (product of
  `n/d` over convert splits with `effective_day > day`).
  Stored-state scaling: `TickerState::load` takes the instrument's splits; for each `Convert`
  split whose `stock_splits` row is `pending` (or absent) it scales the stored rows dated before
  it the same way (f64) — the import (Task 11) then really applies it.

- [ ] **Step 1: Failing tests** in `simulate.rs`'s test module (helpers `buy/sell/parsed/db`
  exist): `convert_scales_pre_split_file_rows` (buy day 100 qty 3 @ 950 with convert 10:1 on
  day 200 → simulated qty 30, price 95, `converted`; buy day 400 untouched);
  `converted_rows_deduplicate_against_stored_current_units` (stored row 30 @ 95 on day 100 →
  the file's 3 @ 950 is `Duplicate`); `sell_check_uses_converted_units` (stored buy 30 (already
  applied), file: sell 25 on day 300 → ok; file in old units with convert: buy 3 day 100, sell
  25 day 300 → ok because 3→30); `pending_stored_rows_are_scaled_for_the_check` (stored buy 3 @
  950 day 100, `stock_splits` pending 10:1 day 200, file sell 25 day 300 with convert → New).
- [ ] **Step 2–4:** implement; `cargo test stock_import` → PASS; regenerate bindings; mirror TS.
- [ ] **Step 5: Commit** `feat(stock-import): convert pre-split rows in the preview`.

### Task 11: Recording and applying splits in `import` (D14)

**Files:**
- Modify: `src-tauri/src/services/stock_import/import.rs`

**Interfaces:**
- Consumes: `stock_splits::{record_detected-like insert, rewrite_pre_split_rows, apply}`; add
  to `stock_splits/mod.rs` a `pub fn record_from_import(conn, ticker, split: &StockSplitOverride)
  -> Result<Option<StockSplit>>` that inserts `source import`, status `applied`/`dismissed` by
  action when no row exists, and returns the row when it existed (unchanged).
- Produces: after `bulk_create_stock_transactions`, for each instrument with splits: `Convert`
  on a `pending` existing row → `stock_splits::apply(conn, &row.id)` (rewrites the stored rows
  that were there before the import — the new rows are already in current units; implementer:
  apply must run on the rows that existed before the bulk insert, so run it **before**
  `bulk_create_stock_transactions` inside the same transaction); `result.converted` = count of
  imported rows with `converted`; `earliest_day` = min over imported rows and applied splits'
  first transaction day.

- [ ] **Step 1: Failing tests** in `import.rs`'s test module: `convert_records_an_applied_split`
  (new ticker → row `import/applied`), `keep_records_a_dismissed_split`,
  `convert_applies_a_pending_split_to_stored_rows` (stored 3 @ 950 day 100 + pending row; file
  sell 25 day 300 convert → stored becomes 30 @ 95, split applied, imported sell 25, holdings
  5), `keep_leaves_a_pending_split_pending` (stored rows unchanged, row still pending, file rows
  imported as given), `existing_applied_row_is_left_alone`, `converted_count_in_result`.
- [ ] **Step 2–4:** implement; `cargo test stock_import::import` → PASS.
- [ ] **Step 5: Commit** `feat(stock-import): record and apply splits on import`.

### Task 12: `check_stock_splits` (D12, D22)

**Files:**
- Create: `src-tauri/src/services/stock_import/splits.rs`
- Modify: `src-tauri/src/services/stock_import/types.rs` (`StockSplitCheck`,
  `StockSplitCheckSplit`, `StockSplitCheckTrade { line, verdict }` of §6),
  `commands/stock_import.rs` (`check_stock_splits(db, file_path, config) -> Vec<StockSplitCheck>`),
  `lib.rs`, `bindings.rs`, `generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts`
  (`stockImportApi.checkSplits(filePath, config)`)

**Interfaces:**
- Produces:
  ```rust
  /// Per instrument: the splits after its first will-import trade with the D7 verdicts.
  pub async fn check_splits(db: &Database, parsed: &ParsedFile, config: &StockImportConfig) -> Result<Vec<StockSplitCheck>>;
  pub(crate) async fn check_with<F, Fut>(conn_series: F, groups: Vec<TickerGroup>, today: i64) -> Vec<StockSplitCheck>
      where F: Fn(String, i64, i64) -> Fut, Fut: Future<Output = Result<HistoricalSeries>>;  // injectable fetch
  pub(crate) struct TickerGroup { key: String, ticker: String, trades: Vec<(usize /*line*/, i64 /*day*/, f64 /*price*/, String /*currency*/)>, stored_status: HashMap<i64, StockSplitStatus>, stored_pre_split_count_by_day: HashMap<i64, i64> }
  ```
  Flow in the command: `config` with every override's `splits` cleared → `db.with_conn(|conn|
  simulate + group + stored statuses)` (DB lock released) → `check_with(real fetch, …)` →
  result. The real fetch is `price_api::get_historical_stock_series_yahoo(&[ticker], first_day,
  today, false)` mapped to the single series; an empty/missing series → `checked: false`.

- [ ] **Step 1: Failing tests** in `splits.rs`: `classifies_trades_against_the_series` (stub
  series: closes 96 on day 100, split 10:1 day 200; trades (line 2, day 100, 950, USD) → split
  verdict `Unadjusted`, trade verdict `Unadjusted`), `splits_before_the_first_trade_are_not_reported`,
  `fetch_failure_marks_unchecked`, `currency_mismatch_is_unknown`, `stored_status_and_count_are_passed_through`.
- [ ] **Step 2–4:** implement; `cargo test stock_import::splits` → PASS; bindings; TS mirror.
- [ ] **Step 5: Commit** `feat(stock-import): check Yahoo splits for the file's trades`.

### Task 13: Wizard UI for splits

**Files:**
- Modify: `src/components/stocks/import/StockImportDialog.tsx` (split check state: `splitChecks:
  Record<key, StockSplitCheck>`, `isCheckingSplits`; runs `checkSplits` when `step === 'review'`
  and `!resolution.isResolving` and the config changed in ticker overrides — key the effect on a
  memo of `instrumentOverrides.map(o => o.key + ':' + o.ticker)` — and merges the default
  overrides `splits` into `overrides` via `mergeSplitOverrides` unless the user already chose)
- Modify: `import-config.ts` (`defaultSplitAction(verdict): StockSplitAction`,
  `splitOverridesFor(check): StockSplitOverride[]`, `mergeSplitOverrides(current, key, splits)`;
  `summarizePreview` counts `converted`; `hasOverrideContent` includes `splits`) + `import-config.test.ts`
- Modify: `InstrumentsPanel.tsx` (a split line under the instrument row: ratio, date, verdict
  text, "přepočítá i N uložených transakcí" when `storedPreSplitCount > 0 && storedStatus ===
  'pending'`, a `Switch` convert/keep; `splitChecks` + `onSplitAction(instrument, effectiveDay,
  action)` props; a "Splity se nepodařilo ověřit" line for `checked: false`), `ReviewStep.tsx`
  (pass-through + summary strip "Přepočteno: N"), `ResultStep.tsx` (converted tile)
- Modify: `src/i18n/locales/*/stocks.json` (`importWizard.splits.*`)

- [ ] **Step 1: Failing tests** in `import-config.test.ts`: `defaultSplitAction('unadjusted')
  === 'convert'`, `'adjusted' → 'keep'`, `'unknown' → 'keep'`; `mergeSplitOverrides` keeps a
  user choice and adds new splits; `summarizePreview` exposes `converted`.
- [ ] **Step 2–3:** implement, `npm run lint && npm run typecheck && npm test` → PASS.
- [ ] **Step 4: Verify in the app** (demo, port 1422): import a CSV with an NVDA buy dated
  2024-05-01 at 950 USD and a sell dated 2024-09-01 of 25 shares — the review shows the 10:1
  split line with "convert" on, the preview rows show 30 @ 95, the result tile says 1
  converted, the position holds the right quantity.
- [ ] **Step 5: Commit** `feat(stock-import): split line and convert/keep choice in the review`.

## Phase D — demo, docs, wrap-up

### Task 14: Demo seed, anonymiser, docs, final gates

**Files:**
- Modify: `src-tauri/src/bin/seed_demo.rs` (D24: NVDA `&[(3.0, 950.0, 900), (20.0, 120.0, 200)]`)
- Check: `src-tauri/src/bin/local_anonymize_profile.rs` — if tickers are rewritten or rows
  deleted per ticker there, treat `stock_splits` the same way (add the table to the per-ticker
  delete list near the `stock_price_overrides` one if that is what it does).
- Modify: `README.md` feature list (one line: splits), `CHANGELOG.md` (Unreleased).
- Modify: `docs/architecture/overview.md` only if it lists the stock services.

- [ ] **Step 1:** seed change + `cargo test` + `cargo run --bin seed_demo -- --help` sanity.
- [ ] **Step 2: Demo run recipe** (verification of Tasks 8 and 13 if not done yet): copy
  `demo-home` into the worktree (`cp -R ../Moony-tauri/demo-home ./demo-home`), ensure
  `demo-home/Library/Application Support/com.moonyfinance.desktop` exists, re-seed with the demo
  password `demo1234`, add a temporary launch entry to the shared checkout's `.claude/launch.json`
  that `cd`s into the worktree, sets `HOME` to the worktree's `demo-home` and runs
  `npm run tauri dev -- --config '{"build":{"devUrl":"http://localhost:1422","beforeDevCommand":"npm run dev -- --port 1422 --strictPort"},"app":{"windows":[{"backgroundThrottling":"disabled"}]}}'`;
  open the page, confirm it greets Alex, unlock, wait for the startup rebuild, open the agenda
  card and the NVDA detail. Remove the launch entry afterwards.
- [ ] **Step 3: Full gates** (Global Constraints) + `cargo test generate_bindings -- --ignored`
  shows no diff.
- [ ] **Step 4: Commit** `chore(demo): NVDA bought before its split; docs for stock splits`.
- [ ] **Step 5:** whole-branch review (subagent-driven-development's final review), fix rounds,
  then merge: `git -C ../Moony-tauri worktree add <scratch>/main-merge main`, `git merge
  --no-ff feat/stock-splits`, verify `git merge-base --is-ancestor feat/stock-splits main`,
  remove the scratch worktree. Never push.
