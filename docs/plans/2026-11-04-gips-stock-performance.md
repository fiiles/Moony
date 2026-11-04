# Stock performance by GIPS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The stocks analysis page computes TWR, MWR and contributions by GIPS: every position held in the period, trades at their own price plus an optional fee, gross dividends, IRR, annualisation.

**Architecture:** The daily engine in the stock performance service keeps its shape (per position and day: value, flow, base, gain; chain-linking; contribution weights). Its inputs change: a position universe that includes sold positions, flows from the transactions (price, fee, transaction currency), dividends from the stock dividends service, and an IRR in place of modified Dietz. A new `fee` column carries the fee from the forms, the import and MCP. The page shows the new figures and stops hiding sold positions.

**Tech Stack:** Rust (rusqlite, Tauri 2 commands), React 19 + TypeScript, Vitest, i18next (10 locales).

**Spec:** `docs/specs/2026-10-30-gips-stock-performance-design.md`

## Global Constraints

- The ten rules in `AGENTS.md` hold for every task (invoke only in `tauri-api.ts`; the 7-step type-contract procedure; per-field `serde(rename)` + `bindings.rs` + regenerated `generated-types.ts`; thin commands, `AppError`; `validate()`; append-only migrations; mutation invalidation; all 10 locales; money as TEXT, conversions only through `services/fx.rs`, never 1:1).
- Migration number: `028_transaction_fee` (027 is the stock dividends migration). ADR number: `0023`.
- Fee: nullable `fee` TEXT on `investment_transactions`, in the transaction currency, `NULL` = 0, never negative.
- Flows: buy `+(q × price + fee)`, sell `−(q × price − fee)`, in the transaction currency (= the position currency), converted at the trade day.
- Base: `V(s−1) + max(F(s), 0)`; gain: `V(s) − V(s−1) − F(s) + D(s)`.
- Dividends: gross, by ex-date, only through `stock_dividends::payouts` (never a second rule).
- Annualise only when the period is longer than 365 days; exactly 365 is not annualised.
- IRR bracket `(−99.99 %, +10 000 %)`, time = calendar days / 365, `t₀` = base day.
- Commit on the feature branch only; never in the shared checkout; no push.

## Review Focus

1. **A day trade** (buy and sell of one position on the same day, quantity 0 before and after): the position is in the universe, its profit is in the portfolio return, nothing is NaN or infinite. Test in Task 3.
2. **A fee larger than the proceeds of a small sell** (net flow turns positive): treated as money in, no panic, return finite. Test in Task 3.
3. **Editing a transaction from a client that does not send `fee`** (mobile, an old MCP caller): the stored fee stays. Test in Task 1.
4. **A conversion missing on a trade or ex-date day** for a USD position in a CZK report: that position drops out of that day only and the miss is counted in `fxQuality`; the other positions keep chaining. Test in Task 4.
5. **An IRR with no solution** (value wiped out, or a period with no money in it): `null` on the wire, "—" on the page, the mobile YTD figure does not crash. Tests in Tasks 5 and 6.

---

### Task 1: The fee column through the backend, wire and MCP

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (new `MIGRATION_028` after `MIGRATION_027`, entry `("028_transaction_fee", MIGRATION_028)` after the 027 entry, a migration test), `src-tauri/src/db/schema.snapshot` (regenerate)
- Modify: `src-tauri/src/models/investments.rs` (`InvestmentTransaction`, `InsertInvestmentTransaction`, `validate()`)
- Modify: `src-tauri/src/services/investments.rs` (`create_transaction_internal`, `update_transaction`, `add_transaction_to_investment`, `import_stock_row`, `BulkStockRow`, `validate_bulk_row`, `bulk_create_stock_transactions`, re-selects)
- Modify: `src-tauri/src/commands/investments.rs` (the three SELECT lists and `InvestmentTransaction` literals)
- Modify: `src-tauri/src/services/mcp/investments.rs` (`StockTransactionRow`, the read JSON of `investment_detail` and `stock_transactions`), `src-tauri/src/services/mcp/mod.rs` (tool description)
- Modify: `src-tauri/src/bin/seed_demo.rs` (literals), every hand-written `CREATE TABLE investment_transactions` in tests that a changed query reads (`grep -rn "CREATE TABLE investment_transactions" src-tauri/src`)
- Modify: `shared/schema.ts` (`InvestmentTransaction`, `insertInvestmentTransactionSchema`), `shared/generated-types.ts` (regenerate)
- Modify: `docs/architecture/database.md` (028 bullet, `investment_transactions` row)

**Interfaces:**
- Produces: `investment_transactions.fee TEXT NULL`; Rust `InvestmentTransaction.fee: Option<String>` (`#[serde(rename = "fee")]`); `InsertInvestmentTransaction.fee: Option<String>` (`#[serde(default, rename = "fee")]`); `BulkStockRow.fee: Option<String>`; `create_transaction_internal(..., fee: Option<&str>, origin)`; TS `InvestmentTransaction.fee: string | null`, insert schema `fee: z.string().optional()`.

- [ ] **Step 1: Write the failing tests**
  - Migration test: after the chain up to 028, `INSERT` into `investment_transactions` with `fee = '1.50'` and read it back; a row without fee reads `NULL`.
  - `validate()` tests in `models/investments.rs`: `fee: Some("2.5")` passes and is kept; `Some("")` becomes `None`; `Some("-1")` → `AppError` with key `validation.feeNegative`; `Some("abc")` → `validation.feeInvalid`.
  - `update_transaction_keeps_the_fee_when_the_update_omits_it`: create with `fee: Some("3")`, update with `fee: None` → stored fee still `"3"`; update with `fee: Some("")` → `NULL`.
  - `bulk_rows_store_their_fee`: one `BulkStockRow` with `fee: Some("0.99")` → the stored row has `fee = '0.99'`.
  - `mcp_stock_transactions_create_stores_the_fee` in the MCP bulk tests (extend its in-memory schema with the columns the shared path writes).

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test fee`
Expected: compile errors / FAIL (no `fee` field, no column).

- [ ] **Step 3: Implement**
  - `MIGRATION_028`: `ALTER TABLE investment_transactions ADD COLUMN fee TEXT;`
  - `validate()` normalises the fee with `dedup::canonical_amount` like the price; empty → `None`.
  - `update_transaction` writes `fee` only when `data.fee` is `Some` (`Some("")` clears it).
  - The fee is **not** part of a transaction's duplicate identity (dedup by external id and by ticker/date/type/quantity/price/currency stays as is).
  - MCP `StockTransactionRow.fee: Option<String>` with a `schemars` description "Trading fee in the transaction currency, optional"; read tools add `"fee"` to each transaction object.

- [ ] **Step 4: Regenerate and run**

Run: `cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored && cargo test generate_bindings -- --ignored && cargo test && cargo clippy -- -D warnings`
Expected: PASS; `git diff shared/generated-types.ts` shows only the `fee` fields.

Run: `npm run typecheck && npm test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A src-tauri shared docs/architecture/database.md
git commit -m "feat(stocks): optional fee on a stock transaction (migration 028)"
```

---

### Task 2: The fee in the forms

**Files:**
- Modify: `src/components/stocks/BuyInvestmentModal.tsx`, `SellInvestmentModal.tsx`, `AddInvestmentModal.tsx`, `EditTransactionModal.tsx`
- Modify: `src/hooks/use-investment-mutations.ts` (export the key list or a refresh helper if needed)
- Modify: `src/i18n/locales/{en,cs,de,es,it,nb,pl,sv,ja,zh}/stocks.json`

**Interfaces:**
- Consumes: Task 1 `InsertInvestmentTransaction.fee`, `InvestmentTransaction.fee`.

- [ ] **Step 1: Add an optional "Poplatek" field** to the four forms, under the price, in the transaction currency (same locked currency adornment as the price). Empty submits no fee (`undefined`); a value must be a number ≥ 0 (zod: optional, `validation.feeInvalid` / `validation.feeNegative` messages in the existing validation namespace). The edit form pre-fills the stored fee and sends `""` when the user clears it.
- [ ] **Step 2: Fix the invalidation of the buy, sell and add modals** to the rule-7 set used by `useInvestmentTransactionMutations` (`STOCK_TRANSACTION_KEYS` incl. `['stock-performance']` and `['stocks-analysis']`, `portfolio-metrics`, `cashflow-report`, `recordSnapshot()`, `portfolio-history`): reuse the hook's helper rather than a fourth hand-written list.
- [ ] **Step 3: Copy** — new keys `modal.fee` ("Fee (optional)" / "Poplatek (nepovinné)") and the two validation keys in all 10 locales (`docs/standards/i18n.md`; remove the dead `modals.buy.fee` / `modals.sell.fee` keys from every locale).
- [ ] **Step 4: Run**

Run: `npm run lint && npm run typecheck && npm test`
Expected: PASS (including `src/i18n/locales.test.ts`).

- [ ] **Step 5: Commit** — `feat(stocks): fee field in the buy, sell, add and edit forms`

---

### Task 3: The engine — universe, transaction-priced flows, fee, base rule, price fallback

**Files:**
- Modify: `src-tauri/src/services/stock_performance.rs` (engine + tests + module doc)
- Modify: `src-tauri/src/models/stock_performance.rs` (`StockPerformance.unpricedTickers`), `src-tauri/src/bindings.rs` (unchanged registrations), `shared/schema.ts`, `shared/generated-types.ts`

**Interfaces:**
- Consumes: Task 1 `investment_transactions.fee`, `price_per_unit`, `currency`, `investment_id`.
- Produces (used by Tasks 4–5):
  - `struct Position { id: String, ticker: String }` and `fn all_positions(conn) -> Result<Vec<Position>>` (every `stock_investments` row, ordered by ticker) replacing `held_positions`.
  - `struct Trades` loaded once: per ticker, ascending `(day, quantity_delta, cash_native, currency)` where `cash_native` is `+(q × price + fee)` / `−(q × price − fee)`; `Quantities` derives from it.
  - `struct Inputs<'a> { history: &'a HashMap<String, Vec<DayHolding>>, quantities: &'a Quantities, trades: &'a Trades, dividends: &'a Dividends }` (`Dividends` is an empty placeholder type in this task, filled in Task 4).
  - `fn build_daily(inputs: &Inputs, tickers: &[&str], days: &[i64], fx: &ReportFx, quality: &mut Quality) -> Daily` with `Daily.dividend: Vec<Vec<Option<f64>>>` (all `Some(0.0)` in this task).
  - `fn in_universe(daily: &Daily, p: usize) -> bool`: quantity > 0 at the end of any day `k ≥ 0`, or a non-zero flow or dividend on any `k ≥ 1`.
  - Wire: `StockPerformance.unpricedTickers: Vec<String>` (`#[serde(rename = "unpricedTickers")]`), TS `unpricedTickers: string[]`.

- [ ] **Step 1: Extend the test helpers.** `setup_db` gets the real columns the engine reads (`investment_transactions(id, investment_id, type, ticker, quantity, price_per_unit, currency, transaction_date, fee)`); `seed` writes each quantity change as a trade at that day's row price and currency (so existing tests keep their meaning); `insert_tx` gains `price: f64, currency: &str, fee: Option<f64>`.
- [ ] **Step 2: Write the failing tests**
  - `a_position_sold_out_inside_the_range_still_contributes`: two positions, one sold on day 3 of 6; contributions of both sum to the portfolio TWR (1e-9) and the sold one is in `positions` with end value 0.
  - `a_position_sold_before_the_range_is_not_in_it`: sold on day 1, range from day 3 → not in `positions`.
  - Rework `closed_positions_and_unknown_ids_are_ignored` into the two above (unknown ids still ignored).
  - `a_buy_above_the_close_is_that_days_loss`: buy 10 at 105, close 100, no other holding → day return −4.7619 % (`100·10/1050 − 1`).
  - `a_sell_below_the_close_is_that_days_loss_and_a_sane_return`: hold 10 at 100, sell all at 95 → day return −5 % (base = previous value 1000, gain = 0 − 1000 + 950).
  - `a_fee_lowers_the_return`: buy 10 at 100 with fee 10, close 100 → day return `1000/1010 − 1`; sell 10 at 100 with fee 10 → day return −1 %.
  - `a_day_trade_counts_in_the_portfolio` (Review Focus 1): buy 10 at 100 and sell 10 at 110 on day 2 next to a held position; portfolio day gain includes +100, all figures finite.
  - `a_fee_larger_than_the_proceeds_is_money_in` (Review Focus 2): sell 1 at 1 with fee 5 → finite return, no panic.
  - `a_ticker_without_prices_is_valued_at_its_trade_prices`: no `stock_value_history` rows, buys at 50 then 60 → value carried at 50 then 60, ticker listed in `unpricedTickers`.
  - `a_split_between_two_trades_changes_nothing`: buy 10 at 100, a 2:1 split rewritten into current units (the stored buy is 20 at 50, rows at half price), buy 20 at 55 → the same flows and TWR as the unsplit history at 110.
  - `the_boeing_case_keeps_its_full_contribution`: position A alone from day 0 falls 55 %, position B (10× larger) bought on day 5 flat → A's contribution ≈ −55 pp, portfolio TWR ≈ −55 %.
- [ ] **Step 3: Run them** — `cd src-tauri && cargo test stock_performance` → FAIL.
- [ ] **Step 4: Implement**
  - Universe: compute the daily grid for every position, then keep only `in_universe` ones for `positions`, `members_of` and the weights; the range clamp (`history_span`) covers every position's rows.
  - Flow on day `k ≥ 1`: the sum of that day's `Trades` cash, converted from the trade currency at the day; `None` if any conversion fails (the position drops out of the day, as today).
  - Base/gain per Global Constraints; a buy and a sell of one position on one day net out before `max(F, 0)`.
  - Price fallback: a ticker with no `stock_value_history` row at all gets synthetic `DayHolding`s from its trades (day, price, trade currency) before `build_daily`; collect those tickers into `unpricedTickers` (only universe positions).
  - Rewrite the module doc to the new method (no line numbers).
- [ ] **Step 5: Run** — `cargo test && cargo clippy -- -D warnings`, `cargo test generate_bindings -- --ignored`, `npm run typecheck && npm test` → PASS.
- [ ] **Step 6: Commit** — `feat(stocks): performance counts sold positions and trades at their own price and fee`

---

### Task 4: The engine — gross dividends and the coverage note

**Files:**
- Modify: `src-tauri/src/services/stock_performance.rs`, `src-tauri/src/models/stock_performance.rs`, `shared/schema.ts`, `shared/generated-types.ts`
- Maybe modify: `src-tauri/src/services/stock_dividends.rs` (only to add a range-limited or batched variant of `payouts` that keeps its rule; with a test that it matches `payouts`)

**Interfaces:**
- Consumes: `stock_dividends::payouts(conn, today) -> Result<Vec<Payout>>` (`Payout { investment_id, ticker, ex_date, gross, currency, source }`), `stock_dividend_coverage(ticker, covered_from, fetched_at)`, Task 3 `Inputs`, `Daily.dividend`.
- Produces: `struct Dividends` (per ticker: ascending `(ex_date, gross, currency)`), filled in `stock_performance`; wire `StockPerformance.dividends: Vec<String>` (gross dividends cumulated since the first date, decimal TEXT, same dates as `values`) and `StockPerformance.dividendGapTickers: Vec<String>` (universe tickers whose coverage row is missing or whose `covered_from` is later than `max(range start, the ticker's first trade day)`).

- [ ] **Step 1: Extend `setup_db`** with `stock_dividend_events`, `stock_dividend_coverage` and `investment_dividends` as migration 027 defines them; a helper `insert_dividend_event(conn, ticker, at, per_share, currency)` + `cover(conn, ticker, from)`.
- [ ] **Step 2: Write the failing tests**
  - `a_dividend_is_gross_gain_on_its_ex_date`: 10 shares held, flat price 100, event 2.0 USD per share on day 3 (USD→CZK rate set) → day 3 return +2 %, cumulative `dividends` = 20 USD in CZK from day 3 on, `netFlows` unchanged.
  - `a_dividend_is_paid_on_the_shares_held_the_day_before`: buy 10 more on the ex-date → payout on the old 10 only.
  - `a_manual_dividend_counts_where_yahoo_has_none`: no events for the ticker, one `investment_dividends` row of 15 CZK on day 3 → day 3 gain +15; once an event exists for the ticker the manual row no longer counts.
  - `a_failed_dividend_conversion_drops_the_day` (Review Focus 4): no rate on the ex-date → the position drops out of that day only, `fxQuality` counts the miss, the other position keeps chaining.
  - `a_ticker_without_coverage_is_named`: no coverage row → in `dividendGapTickers`; coverage from before the range → not named.
- [ ] **Step 3: Run** → FAIL.
- [ ] **Step 4: Implement** — load payouts once, bucket by position and ex-date, convert at the ex-date into `Daily.dividend`; the native-currency series converts them into its own currency like values.
- [ ] **Step 5: Run** all gates as in Task 3 → PASS.
- [ ] **Step 6: Commit** — `feat(stocks): gross dividends in the stock return, with a note for tickers not yet loaded`

---

### Task 5: The engine — IRR money-weighted return

**Files:**
- Modify: `src-tauri/src/services/stock_performance.rs`, `src-tauri/src/models/stock_performance.rs`, `shared/schema.ts` (incl. the `mwr` doc comment), `shared/generated-types.ts`
- Modify: `src/mobile/hooks/use-stock-positions.ts` (`useTwrThisYear` handles a `null` MWR)
- Create: `docs/architecture/decisions/0023-stock-performance-follows-gips.md`; Modify: `docs/architecture/decisions/README.md` (index row, next free number `0024`)

**Interfaces:**
- Produces: `fn irr(cash_flows: &[(f64, f64)]) -> Option<f64>` — `(years_from_t0, amount)` pairs (investor view: day 0 `−V₀`, day `k` `−F_k + D_k`, the end `+V_end`), returns the annual rate as a fraction; wire `StockPerformance.mwr: Vec<Option<f64>>` (period rate in percent: `(1 + r)^{t} − 1`) and `StockPerformance.mwrAnnual: Vec<Option<f64>>` (annual `r` in percent), both per emitted date; index 0 is `Some(0.0)`.

- [ ] **Step 1: Write the failing tests**
  - `irr_of_a_single_deposit_that_grows_ten_percent_in_a_year` → `Some(0.10)` (1e-9).
  - `irr_matches_a_textbook_case`: −1000 at 0, −500 at 0.5, +1700 at 1.0 → the rate that zeroes NPV (assert NPV(r) ≈ 0 within 1e-6).
  - `irr_without_a_sign_change_is_none` (Review Focus 5): only negative amounts → `None`; a wiped-out portfolio (−1000 at 0, end value 0) → `None` (no sign change in the bracket).
  - `mwr_is_the_period_rate_and_mwr_annual_the_yearly_one`: deposit 1000 grows to 1210 over 730 days → `mwr` 21 %, `mwrAnnual` 10 %.
  - Rework `mwr_falls_below_twr_when_money_arrives_before_a_drop` and `a_deposit_is_not_return` to the IRR (`mwr` last = 0 for a deposit only).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** — Newton from 0.1 with bisection fallback on the bracket; evaluate only at emitted indices; skip zero flows. Remove `money_weighted` (modified Dietz). Update the `mwr` doc comment in `schema.ts`. In the mobile hook treat `null` as "no figure".
- [ ] **Step 4: ADR 0023** — one page: position universe incl. sold positions, transaction-price flows, fee, money in at the start / out at the end of the day, gross dividends on the ex-date via the dividends service (links ADR 0022), IRR, annualisation rule, sleeve without cash, price fallback.
- [ ] **Step 5: Run** all gates → PASS.
- [ ] **Step 6: Commit** — `feat(stocks): money-weighted return as IRR; ADR 0023`

---

### Task 6: The page

**Files:**
- Modify: `src/pages/StocksAnalysis.tsx`, `src/components/stocks-analysis/performance-model.ts` (+ `performance-model.test.ts`), `PerformanceCard.tsx`, `src/utils/stock-analysis.ts` (if `heldPositions` loses its only caller, delete it)
- Modify: `src/i18n/locales/*/reports.json` (`stocksAnalysis.performance.*` and the notes)

**Interfaces:**
- Consumes: wire fields from Tasks 3–5 (`positions` universe, `unpricedTickers`, `dividends`, `dividendGapTickers`, `mwr`, `mwrAnnual`).
- Produces (TS, pure, tested): `periodDays(from: string, to: string): number`; `annualizes(days: number): boolean` (`days > 365`); `annualized(cumulativePct: number, days: number): number` (`((1 + c/100)^(365/days) − 1)·100`); `Figures` gains `annual: boolean`, `twrAnnual: number | null`, `mwr: number | null` (annual when `annual`, else period), `mwrGap: number | null` (same basis), `dividends: number`.

- [ ] **Step 1: Write the failing TS tests** in `performance-model.test.ts`: `annualizes(365) === false`, `annualizes(366) === true`; `annualized(21, 730) ≈ 10`; `figuresAt` over a 730-day fixture uses `mwrAnnual` and compares it with the annualised TWR; over a 200-day fixture uses `mwr` and the cumulative TWR; a `null` MWR gives `mwr: null`, `mwrGap: null`.
- [ ] **Step 2: Run** — `npx vitest run src/components/stocks-analysis/performance-model.test.ts` → FAIL.
- [ ] **Step 3: Implement the model**, then the card: TWR figure as now plus "ročně X %" when `annual`; MWR label suffix "ročně" / "za období", "—" when `null`; the change figure gains a "vyplacené dividendy" line when `dividends ≠ 0`; under the hero, one note per non-empty `unpricedTickers` ("Bez tržní ceny, oceněno cenami obchodů: …") and `dividendGapTickers` ("Dividendy se ještě nenačetly: …"), in the style of `ConversionQualityNote`.
- [ ] **Step 4: Sold positions on the page.** Build `lite` and the series specs from every position of the analysis (not only held), and show in the scope summary, board, contribution card and holdings table the positions that are held now or appear in `performance.positions`; drop the "sold-out positions are not in the engine" comment. Today's value of a sold position is 0.
- [ ] **Step 5: Copy** in all 10 locales.
- [ ] **Step 6: Run** — `npm run lint && npm run typecheck && npm run format:check && npm test` → PASS.
- [ ] **Step 7: Commit** — `feat(stocks): annualised figures, dividends and sold positions on the analysis page`

---

### Task 7: Fee in the import

**Files:**
- Modify: `src-tauri/src/services/stock_import/{types.rs,columns.rs,parse.rs,simulate.rs,import.rs,adapters/mod.rs,adapters/degiro.rs,adapters/trading212.rs}`, `src-tauri/src/bindings.rs` (unchanged registrations), `shared/schema.ts`, `shared/generated-types.ts`
- Modify: the column-mapping and review steps under `src/components/stocks/import/`, `src/i18n/locales/*/stocks.json` (`importWizard.*`)

**Interfaces:**
- Consumes: Task 1 `BulkStockRow.fee`.
- Produces: `StockImportConfig.feeColumn: Option<usize>` and `feeCurrencyColumn: Option<usize>` (`#[serde(default)]`, so saved formats keep loading); `ParsedTrade.fee: Option<f64>` (positive, in the trade currency).

- [ ] **Step 1: Write the failing tests**
  - `a_fee_column_is_imported_as_a_positive_fee`: a CSV row with fee `-1.25` → stored fee `1.25`.
  - `a_fee_in_another_currency_is_converted_at_the_trade_date`: trade in USD, fee 2 EUR, EUR→USD rate on that day → stored fee in USD.
  - `a_fee_without_its_own_currency_is_in_the_trade_currency`.
  - `a_fee_in_pence_becomes_pounds`.
  - `degiro_maps_its_fee_and_fee_currency_columns` and `trading212_maps_the_conversion_fee`.
  - `a_saved_format_without_fee_fields_still_loads`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** — the column suggestion pre-fills `feeColumn` / `feeCurrencyColumn`; the parser reads them; conversion through `services/fx.rs` at the trade day (a missing rate is a row error in the preview, not a silent 1:1); pence via `quote_unit::pence_to_pounds`; the review step's "Fees are not imported" note is replaced by the mapped fee column; the wizard's mapping step offers the two columns like the other optional ones.
- [ ] **Step 4: Run** all gates → PASS.
- [ ] **Step 5: Commit** — `feat(stock-import): import the trading fee`

---

### Task 8: Verification on the demo profile

- [ ] **Step 1:** Copy the demo profile into the worktree (`cp -R demo-home <worktree>/demo-home`) and run the worktree app on a free port with its own `HOME` or an identifier override (memory: dev-server port collision). Confirm the lock screen greets Alex before typing the demo password.
- [ ] **Step 2:** On the stocks analysis page check: a sold position appears for a period it was held in and disappears for a later period; the hero shows "ročně" figures for "3 roky" and none for "Letos"; MWR is a number; dividends line appears; adding a buy with a fee changes the TWR by about the fee over the value; the existing price-range drag, index comparison and grouping still work. Screenshot the hero for the report.
- [ ] **Step 3:** Full gates on the branch tip:

Run: `npm run lint && npm run typecheck && npm run format:check && npm test && cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: PASS.
