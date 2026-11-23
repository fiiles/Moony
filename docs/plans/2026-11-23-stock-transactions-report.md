# Stock Transactions Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A desktop-only report "Akciové transakce" that shows, for a period, what was sold, what it cost, fees, result and dividends, with an optional holding-period test and a switchable cost method.

**Architecture:** A new Rust service replays `investment_transactions` into FIFO lots, values every match in the report currency through `services/fx.rs` and returns one typed response; a thin command exposes it. The React page only filters and totals what it was given. Three preferences live in `app_config`.

**Tech Stack:** Rust (rusqlite, serde, specta), Tauri 2 commands, React 19 + TypeScript, TanStack Query, Tailwind tokens, Radix (shadcn), Recharts wrapper `MoonyBarChart`, i18next, Vitest.

**Spec:** `docs/specs/2026-11-23-stock-transactions-report-design.md` (binding; D-numbers below refer to it). Visual reference: `docs/specs/2026-11-23-stock-transactions-report-prototype.html` (open it in a browser; where it differs from the spec, the spec wins).

## Global Constraints

- The ten rules of `AGENTS.md` apply to every task. In particular: `invoke()` only in `src/lib/tauri-api.ts`; wire types follow the 7-step procedure of `docs/standards/type-contract.md` (per-field `#[serde(rename = "camelCase")]`, register in `bindings.rs`, regenerate `shared/generated-types.ts`, hand-mirror into `shared/schema.ts`); thin commands, logic in `src-tauri/src/services/` taking `&Connection`, `AppError` only, no `.unwrap()` outside tests; money is decimal TEXT on the wire, days are unix seconds at UTC midnight.
- Conversion only through `services/fx.rs` (`docs/standards/rust-backend.md` §10): `fx::History::load` once per request, `convert_on_tracked` per amount at its own day, `fx::current()` once for today's values; converted wire amounts are `Option<String>` rounded with `reporting::money_in_currency`, `None` when unconvertible, never 0 or 1:1.
- No migration, no new table. No change to `services/cost_basis.rs` semantics or to any figure shown elsewhere.
- Desktop only: nothing under `src/mobile/`.
- Every user-facing string in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`); `src/i18n/locales.test.ts` must pass. Czech copy is in the prototype and the spec; translate the rest.
- Copy rules (`docs/design-system/README.md` §10): sentence case, no exclamation marks, signed amounts "+ 60 110 Kč" / "− 15 200 Kč" through `useFormat()`, buttons are verbs.
- Design: tokens and existing components only (no raw colours); green and red only for gain and loss, with the ▲/▼ glyph or a sign; nothing may overflow at 1080 px window width; both themes.
- The page never states a tax, a tax rate or advice (spec §2 anti-goals). The holding-period test is off by default.
- Tests: Rust service tests use `Connection::open_in_memory()` with a hand-written minimal schema (never `Database`); frontend tests only for pure functions in `src/utils/`.
- Commits: Conventional Commits, explicit paths in `git add`, never `--no-verify`, check `git branch --show-current` prints `feat/stock-transactions-report` before committing, end every message with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-stock-tx`. Never touch the sibling checkout `Moony-tauri`.

## Wire contract (all tasks)

Hand-mirrored in `shared/schema.ts`; the Rust structs in `src-tauri/src/models/stock_transactions_report.rs` have the same fields in snake_case with per-field renames. Amounts are decimal TEXT in the report currency unless a comment says otherwise.

```ts
export type StockReportCostMethod = 'fifo' | 'average';

export interface StockTransactionsReportRequest {
  fromDay: number; // inclusive, UTC midnight
  toDay: number; // inclusive
  reportCurrency: string;
  costMethod: StockReportCostMethod;
  holdingTestMonths: number | null; // null = test off, otherwise 1..=600
}
export interface StockReportAmounts {
  proceeds: string;
  cost: string;
  result: string; // proceeds - cost
  fees: string; // buy + sale fees of the sold pieces
  resultAfterFees: string;
}
export interface StockReportSummary {
  total: StockReportAmounts;
  taxable: StockReportAmounts | null; // null when the test is off
  exempt: StockReportAmounts | null;
  salesCount: number; // counted sales
  salesInProfit: number;
  salesInLoss: number;
}
export interface StockReportStats {
  proceeds: string; // every sale of the period, uncovered parts included
  salesCount: number;
  soldTickers: number;
  bought: string;
  buysCount: number;
  boughtTickers: number;
  feesPaid: string; // fees of every trade in the period
  tradedVolume: string; // bought + proceeds
  dividends: string;
  dividendsCount: number;
}
export interface StockReportBucket {
  key: string; // 'YYYY-MM', or 'YYYY' when the period spans more than 24 months
  fromDay: number;
  toDay: number;
  buys: string;
  sells: string;
  result: string; // result after fees of the counted sales
}
export interface StockReportTickerResult {
  investmentId: string;
  ticker: string;
  companyName: string;
  quantity: string; // sold and counted
  resultAfterFees: string;
  taxableResult: string | null; // null when the test is off
  exemptResult: string | null;
}
export interface StockReportMatch {
  buyDay: number;
  quantity: string;
  buyPrice: string; // trade currency
  buyCurrency: string;
  buyRate: string | null; // report currency per 1 unit of buyCurrency at buyDay
  averageCost: string | null; // per share, report currency; only with costMethod 'average'
  proceeds: string | null;
  cost: string | null;
  fees: string | null;
  resultAfterFees: string | null;
  exempt: boolean | null; // null when the test is off
}
export interface StockReportSale {
  id: string; // transaction id
  investmentId: string;
  ticker: string;
  companyName: string;
  day: number;
  quantity: string; // whole sale
  price: string; // trade currency
  currency: string;
  rate: string | null; // at the sale day
  uncoveredQuantity: string; // '0' when every share has a lot
  counted: boolean; // false when an amount could not be converted (D15)
  matches: StockReportMatch[];
}
export interface StockReportLedgerRow {
  id: string;
  kind: 'buy' | 'sell' | 'dividend';
  day: number; // ex-date for a dividend
  investmentId: string;
  ticker: string;
  companyName: string;
  quantity: string | null; // dividend: shares held, null for a manual one
  price: string | null; // per share, trade currency; null for a manual dividend
  currency: string;
  rate: string | null;
  amount: string | null; // report currency
  fee: string | null; // trade currency, null when none
}
export interface StockReportOpenLot {
  investmentId: string;
  ticker: string;
  companyName: string;
  buyDay: number;
  quantity: string; // still open
  lotQuantity: string; // as bought
  buyPrice: string;
  buyCurrency: string;
  passDay: number | null; // first day a sale is exempt; null when the test is off
  value: string | null; // today
  unrealized: string | null;
}
export interface StockTransactionsReport {
  currency: string;
  fxQuality: ConversionQuality;
  today: number; // the UTC day the service used
  firstTransactionDay: number | null;
  summary: StockReportSummary;
  stats: StockReportStats;
  buckets: StockReportBucket[];
  tickers: StockReportTickerResult[]; // sorted by resultAfterFees, largest first
  sales: StockReportSale[]; // newest first
  ledger: StockReportLedgerRow[]; // newest first
  openLots: StockReportOpenLot[]; // oldest buy first
  uncoveredTickers: string[];
}
export interface StockTransactionsReportSettings {
  costMethod: StockReportCostMethod;
  holdingTestEnabled: boolean;
  holdingTestMonths: number;
}
```

## Shared test fixture (Rust tasks)

Report currency CZK. `exchange_rate_history(date, currency, rate)` holds units per 1 EUR: on every day `CZK` 25.0; `USD` 1.25 on 2023-01-10 (1 USD = 20 CZK) and 1.0 from 2024-01-01 on (1 USD = 25 CZK).

Position **AAA** (USD): buy 10 @ 100, fee 2, on 2023-01-10; buy 10 @ 200, fee 4, on 2024-01-10; sell 15 @ 300, fee 3, on 2026-02-10.

| | proceeds | cost | result | fees | after fees |
|---|---|---|---|---|---|
| fifo, match 1 (10 from lot 1) | 75 000 | 20 000 | 55 000 | 40 + 50 = 90 | 54 910 |
| fifo, match 2 (5 from lot 2) | 37 500 | 25 000 | 12 500 | 50 + 25 = 75 | 12 425 |
| fifo, total | 112 500 | 45 000 | 67 500 | 165 | 67 335 |
| average, match 1 | 75 000 | 35 000 | 40 000 | 70 + 50 = 120 | 39 880 |
| average, match 2 | 37 500 | 17 500 | 20 000 | 35 + 25 = 60 | 19 940 |
| average, total | 112 500 | 52 500 | 60 000 | 180 | 59 820 |

(Average before the sale: 70 000 CZK over 20 shares = 3 500 per share, fees 140 = 7 per share.) With `holdingTestMonths = 36`: match 1 is exempt (2023-01-10 + 36 months = 2026-01-10 < sale day), match 2 taxable. Open afterwards: 5 shares of lot 2, `passDay` = 2027-01-11.

Assert amounts as `reporting::money_in_currency(<number>, "CZK")` so the test does not depend on the text format.

## Review Focus

- Fractional quantities (buy 0.1 + 0.2, sell 0.3): no dust lot stays open and no phantom uncovered quantity appears (Task 1 test).
- Same-day buy and sale stored in the reverse order: the sale consumes that day's buy (Task 1 test).
- A sale on `toDay` and a buy on `fromDay` are inside the period; the day after and before are not (Task 2 test).
- A trade in a currency with no rate on its day but a current quote: the amount is produced, `fxQuality.estimated` is true; with no quote at all it is `null` and the currency is named (Task 2 test).
- The "Vlastní" months field while empty, 0, 601 or "1.5": the last valid length stays in effect and the field shows the invalid state; the request never carries an invalid value (Task 5 util test, Task 6 wiring).

---

### Task 1: Wire types and the FIFO replay

**Files:**
- Create: `src-tauri/src/models/stock_transactions_report.rs`, `src-tauri/src/services/stock_transactions_report/mod.rs`, `src-tauri/src/services/stock_transactions_report/lots.rs`
- Modify: `src-tauri/src/models/mod.rs`, `src-tauri/src/services/mod.rs`, `src-tauri/src/bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts` (regenerated)

**Interfaces:**
- Produces (Rust, `crate::models`): every type of the wire contract above; `StockTransactionsReportRequest::validate(&self) -> Result<()>` rejecting `from_day > to_day` (`validation.periodInvalid`), a `holding_test_months` outside 1..=600 (`validation.holdingTestMonthsInvalid`), an unsupported `report_currency` (as `StockPerformanceRequest::validate` does), and normalising nothing.
- Produces (`services::stock_transactions_report::lots`):

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeKind { Buy, Sell }
#[derive(Debug, Clone)]
pub struct Trade {
    pub id: String, pub investment_id: String, pub ticker: String, pub company_name: String,
    pub kind: TradeKind, pub quantity: f64, pub price: f64, pub currency: String,
    pub day: i64,   // UTC midnight
    pub fee: f64,   // trade currency, 0.0 when none
}
/// `quantity` shares of the sale `sale` come from the buy `buy` (indices into `Replay::trades`).
#[derive(Debug, Clone, PartialEq)]
pub struct Match { pub sale: usize, pub buy: usize, pub quantity: f64 }
#[derive(Debug)]
pub struct Replay {
    pub trades: Vec<Trade>,            // replay order
    pub matches: Vec<Match>,           // in sale order, oldest lot first within a sale
    pub open: Vec<(usize, f64)>,       // (buy index, open quantity > 0), oldest first
    pub uncovered: Vec<(usize, f64)>,  // (sale index, quantity with no lot)
}
pub fn load_trades(conn: &Connection) -> Result<Vec<Trade>>;  // every stock trade, ORDER BY transaction_date, created_at, rowid
pub fn replay(trades: Vec<Trade>) -> Replay;
```

- [ ] **Step 1: Model the wire types.** Rust structs with `Debug, Clone, Serialize, Deserialize, specta::Type` and per-field renames (copy the style of `models/stock_tags.rs` `StocksAnalysis`); `StockReportCostMethod` as an enum serialised `"fifo"` / `"average"`; `StockReportLedgerRow.kind` as an enum serialised `"buy"` / `"sell"` / `"dividend"`. Re-export from `models/mod.rs`, register every type in `bindings.rs::collect_types()`, mirror into `shared/schema.ts` exactly as the contract block (keep its comments), run `cd src-tauri && cargo test generate_bindings -- --ignored`. The new validation key `validation.holdingTestMonthsInvalid` (cs "Zadejte dobu držení 1 až 600 měsíců.") goes into every locale next to the existing `validation.periodInvalid`.
- [ ] **Step 2: Write the failing tests** in `lots.rs` (`#[cfg(test)]`, trades built in code, no database except for `load_trades`):
  - `a_sale_consumes_the_oldest_lots_first`: fixture AAA → matches `[(sale, lot1, 10.0), (sale, lot2, 5.0)]`, `open == [(lot2, 5.0)]`, `uncovered` empty.
  - `a_lot_can_feed_several_sales`: buy 10, sell 4, sell 4 → two matches from the same buy, open 2.0.
  - `same_day_buy_comes_before_the_sale_whatever_the_stored_order`: the sale row listed before a buy of the same day → one match, nothing uncovered.
  - `selling_more_than_held_reports_the_uncovered_part`: buy 5, sell 8 → one match of 5.0, `uncovered == [(sale, 3.0)]`.
  - `fractional_quantities_leave_no_dust`: buy 0.1, buy 0.2, sell 0.3 → `open` empty, `uncovered` empty (treat |x| < 1e-9 as zero).
  - `positions_do_not_share_lots`: two tickers, a sale of one never consumes the other's buy.
  - `the_replay_agrees_with_the_cost_basis_quantity`: for a mixed history, the sum of `open` quantities equals `cost_basis::replayed_quantity` over the same rows.
  - `load_trades_reads_fee_and_day`: in-memory `stock_investments` + `investment_transactions` (with the `fee` column), a fee of `"1.5"` and a `NULL` fee → 1.5 and 0.0; `day` is the UTC midnight of `transaction_date`.
- [ ] **Step 3: Run** `cd src-tauri && cargo test --lib stock_transactions_report` → the new tests fail to compile or fail.
- [ ] **Step 4: Implement** `load_trades` and `replay`. Order key `(day, kind == Sell)` with a stable sort, as `cost_basis::chronological`; lots per `investment_id`.
- [ ] **Step 5: Run** the same command → all pass; `cargo clippy -- -D warnings`, `cargo fmt --check`, `npm run typecheck`, and `git diff --exit-code shared/generated-types.ts` after staging shows the regenerated file is the committed one.
- [ ] **Step 6: Commit** `feat(stock-report): wire types and FIFO lot replay`.

### Task 2: Valuing sales — cost methods, holding test, summary

**Files:**
- Create: `src-tauri/src/services/stock_transactions_report/sales.rs`
- Modify: `src-tauri/src/services/stock_transactions_report/mod.rs`

**Interfaces:**
- Consumes: `lots::{load_trades, replay, Replay, Trade, Match}`, the wire types.
- Produces: `pub fn report(conn: &Connection, request: &StockTransactionsReportRequest, today: i64) -> Result<StockTransactionsReport>` in `mod.rs`. After this task it fills `currency`, `fx_quality`, `today`, `first_transaction_day`, `summary`, `tickers`, `sales`, `uncovered_tickers`; `stats` is zeroed and `buckets`, `ledger`, `open_lots` are empty (Task 3 fills them).
- Produces for Task 3 (`sales.rs`): `pub struct Valued { pub proceeds: Vec<Option<f64>>, pub fee: Vec<Option<f64>>, pub volume: Vec<Option<f64>> }` — per trade index, in the report currency at the trade's day (`volume` = quantity × price, `proceeds` = the same for sales, `None` for buys); `pub struct PoolState { pub quantity: f64, pub cost: Option<f64>, pub fee: Option<f64> }` per `investment_id` after the whole replay (the average method's remaining pool; `None` once an unconvertible buy entered it); and `pub fn pass_day(buy_day: i64, months: u32) -> i64` (the day after `buy_day` plus `months` calendar months, clamped to the month's last day).

Rules to implement (spec D5, D6, D7, D15, D16): amounts of a match as in D5; `fifo` and `average` as in D6 (the average replays the whole history of the position, not only the period); a match is exempt when `sale.day >= pass_day(buy.day, months)`; a sale is `counted = false` when its proceeds, its fee, or the cost or buy fee of any of its matches cannot be converted, and then its matches carry `None` amounts and it is left out of `summary` and `tickers`; an uncovered quantity produces no match and takes its share of proceeds and sale fee out of the sums; `uncovered_tickers` lists each such ticker once. Conversions go through one `fx::History` loaded for the span from the first trade to `today`, each tracked in one `fx::Quality` for the response. `sales` holds the sales with `from_day <= day <= to_day`, newest first.

- [ ] **Step 1: Write the failing tests** in `sales.rs` (in-memory DB helper `setup_test_db()` in `mod.rs` under `#[cfg(test)]` with `stock_investments`, `investment_transactions` incl. `fee`, `exchange_rate_history`; a helper that inserts the shared fixture). Period 2026-01-01..2026-12-31 unless stated:
  - `fifo_amounts_follow_each_lot`: fixture, `fifo`, test off → the two matches and `summary.total` equal the "fifo" rows of the fixture table; `summary.taxable`/`exempt` are `None`; `sales_count` 1, `sales_in_profit` 1.
  - `average_amounts_use_the_running_average`: same with `average` → the "average" rows; each match has `average_cost` 3 500.
  - `the_holding_test_splits_matches`: `fifo`, 36 months → `taxable` = match 2 row, `exempt` = match 1 row, and for every field `taxable + exempt == total`; match flags `exempt` are `Some(true)`, `Some(false)`.
  - `the_test_boundary_is_the_day_after_the_full_period`: buy 2023-02-10; sale on 2026-02-10 is taxable, on 2026-02-11 exempt (36 months).
  - `a_month_end_purchase_clamps`: `pass_day(2022-11-30, 3)` is 2023-03-01; `pass_day(2023-11-30, 3)` is 2024-03-01 (leap February).
  - `a_custom_length_in_months_works`: 20 months on the fixture → match 1 exempt (2024-09-10 < sale), match 2 exempt too (2025-09-10 < sale).
  - `the_period_includes_both_ends`: period = the sale day only → the sale is in; the day before only → `sales` empty and `summary.total` all zero.
  - `lots_bought_before_the_period_are_used`: the fixture already proves it; assert `sales[0].matches[0].buy_day` is 2023-01-10.
  - `an_oversold_sale_counts_only_its_covered_part`: position BBB (CZK): buy 5 @ 10, sell 8 @ 20 with fee 8 → one match: proceeds 100, cost 50, fees 5, after fees 45; `uncovered_quantity` 3; `uncovered_tickers == ["BBB"]`; `summary.total.proceeds` 100.
  - `an_unconvertible_sale_stays_out_of_the_sums`: position in currency `XXX` with no rate anywhere → `counted` false, match amounts `None`, `summary.total` zero, `fx_quality.missing_currencies` contains `XXX`.
  - `a_missing_day_rate_falls_back_to_the_current_quote_and_is_marked`: a USD trade on a day with no USD history within ten days, `fx::set_current_for_test(&[("CZK", 25.0), ("USD", 1.0)])` → amounts present, `fx_quality.estimated` true.
  - `tickers_are_sorted_by_result`: fixture plus a losing position → `tickers[0]` is AAA with the fixture's after-fees result, the loser last; with the test on, `taxable_result + exempt_result == result_after_fees`.
  - `the_cost_method_does_not_change_which_lots_match`: `fifo` and `average` give the same `buy_day`, `quantity` and `exempt` for every match.
- [ ] **Step 2: Run** `cargo test --lib stock_transactions_report` → new tests fail.
- [ ] **Step 3: Implement** `sales.rs` and `report()`.
- [ ] **Step 4: Run** tests, `cargo clippy -- -D warnings`, `cargo fmt --check` → clean.
- [ ] **Step 5: Commit** `feat(stock-report): value sales by cost method and holding test`.

### Task 3: Stats, buckets, ledger, open lots, command

**Files:**
- Create: `src-tauri/src/services/stock_transactions_report/activity.rs` (stats, buckets, ledger), `src-tauri/src/services/stock_transactions_report/open_lots.rs`, `src-tauri/src/commands/stock_transactions_report.rs`
- Modify: `src-tauri/src/services/stock_transactions_report/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!`), `src/lib/tauri-api.ts`; `src-tauri/src/services/stock_dividends.rs` only if a public accessor for shares held and amount per share is needed (no behaviour change)

**Interfaces:**
- Consumes: Task 2's `Valued`, `PoolState`, `pass_day`; `stock_dividends::payouts` / `for_investment`; `positions::value_positions(conn, AssetKind::…, report_currency, today)` for the current native price, its currency and the conversion.
- Produces: the complete `report()`; command `get_stock_transactions_report(db, request) -> Result<StockTransactionsReport>` (validates, computes today's UTC day the way the existing position commands do, delegates); `stockTransactionsApi.getReport(request: StockTransactionsReportRequest)` in `src/lib/tauri-api.ts` invoking `'get_stock_transactions_report'` with `{ request }`.

Rules (spec D9, D10, D12, D14): stats count every trade of the period (uncovered and uncounted parts included when they convert); buckets are calendar months of the period (`'YYYY-MM'`), or calendar years (`'YYYY'`) when the period touches more than 24 months, each with `from_day`/`to_day` clipped to the period, empty buckets included; ledger = buys, sales and counted dividend payouts with the ex-date in the period, newest first, a dividend's `quantity`/`price` set only for a Yahoo payout; open lots = every open quantity with `pass_day` (when the test is on), `value` = open quantity × the position's current price converted as the positions service does, `unrealized` = value − cost by the method (fifo: open quantity × lot price at the lot's day + the lot's fee share; average: open quantity × the pool's cost and fee per share), `None` when the price or a conversion is missing.

- [ ] **Step 1: Write the failing tests** (extend the test helper with the dividend tables and whatever `value_positions` reads; copy table definitions from the `setup_test_db` of `services/stock_dividends.rs` and `services/positions.rs`):
  - `stats_cover_every_trade_of_the_period`: fixture, 2026 → proceeds 112 500, `sales_count` 1, `sold_tickers` 1, bought 0, `fees_paid` 75, `traded_volume` 112 500. Period 2024 → bought 50 000, `buys_count` 1, `fees_paid` 100, proceeds 0.
  - `stats_keep_the_whole_oversold_sale`: BBB → `stats.proceeds` 160 while `summary.total.proceeds` is 100.
  - `a_year_has_twelve_monthly_buckets`: 2026 → 12 buckets `2026-01`…`2026-12`; `2026-02` has sells 112 500 and result 67 335 (`fifo`); the others zero.
  - `a_long_period_is_bucketed_by_year`: 2023-01-01..2026-12-31 → keys `2023`, `2024`, `2025`, `2026`; `2023.buys` 20 000, `2024.buys` 50 000.
  - `a_custom_period_clips_its_first_and_last_bucket`: 2026-02-05..2026-03-20 → two buckets, the first from 2026-02-05, the last to 2026-03-20.
  - `the_ledger_lists_trades_and_dividends_newest_first`: fixture plus a Yahoo dividend event for AAA, ex-date 2026-03-10, 1 USD per share → 2026 ledger = dividend (quantity 5, price 1, amount 125, fee `None`), then the sale (amount 112 500, fee 3, rate 25); `stats.dividends` 125, `dividends_count` 1.
  - `a_manual_dividend_has_no_quantity_or_price`: a manual dividend on a position without Yahoo events → ledger row with `quantity` and `price` `None` and its converted amount.
  - `open_lots_carry_the_pass_day_and_the_unrealized_result`: fixture, current price of AAA 400 USD, current quotes CZK 25 / USD 1.0, 36 months → one open lot: quantity 5, `lot_quantity` 10, `pass_day` 2027-01-11, value 50 000, unrealized 24 950 with `fifo` and 32 465 with `average`.
  - `open_lots_have_no_pass_day_when_the_test_is_off` and `an_unpriced_lot_has_no_value`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement.** **Step 4: Run** `cargo test --lib stock_transactions_report`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `npm run typecheck`, `npm run lint`.
- [ ] **Step 5: Commit** `feat(stock-report): stats, buckets, ledger, open lots and the read command`.

### Task 4: Stored settings

**Files:**
- Create: `src-tauri/src/services/stock_transactions_report/settings.rs`
- Modify: `src-tauri/src/commands/stock_transactions_report.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/services/sync/registry/tables.rs` (`APP_CONFIG_SYNCED_KEYS` becomes six keys), the sync tests and helpers that list or count the keys (`sync/registry/tests.rs`, `sync/test_support.rs`, `sync/snapshot/tests/mod.rs`, and any other the compiler or tests point to), `docs/architecture/database.md` ("Five keys sync"), `src/lib/tauri-api.ts`

**Interfaces:**
- Produces: `pub const CONFIG_KEY: &str = "stock_transactions_report";` `pub fn load(conn: &Connection) -> Result<StockTransactionsReportSettings>` (defaults `fifo`, `false`, `36` when the key is missing or unreadable); `pub fn save(conn: &Connection, settings: &StockTransactionsReportSettings) -> Result<()>` (rejects months outside 1..=600 with `validation.holdingTestMonthsInvalid`); commands `get_stock_transactions_report_settings` and `save_stock_transactions_report_settings(settings)`; `stockTransactionsApi.getSettings()` and `stockTransactionsApi.saveSettings(settings)`.
- Storage pattern to copy: `services/stock_import/formats.rs` (`SELECT value FROM app_config WHERE key = ?1`, upsert with `ON CONFLICT(key) DO UPDATE`).

- [ ] **Step 1: Failing tests** in `settings.rs`: `defaults_when_nothing_is_stored`; `save_then_load_round_trips` (`average`, `true`, `18`); `an_unreadable_value_falls_back_to_defaults` (value `"not json"`); `months_out_of_range_are_refused` (0 and 601). In the sync registry test that pins the allowlist: the array is exactly the previous five keys plus `stock_transactions_report`.
- [ ] **Step 2: Run** → fail. **Step 3: Implement**, register both commands. **Step 4: Run** `cargo test --lib stock_transactions_report` and `cargo test --lib sync::registry` and `cargo test --lib sync::snapshot`; clippy; fmt; `npm run typecheck`.
- [ ] **Step 5: Commit** `feat(stock-report): store the cost method and holding test in app_config`.

### Task 5: Frontend utilities and hooks

**Files:**
- Create: `src/utils/stock-transactions-report.ts` + `.test.ts`, `src/utils/stock-transactions-csv.ts` + `.test.ts`, `src/hooks/use-stock-transactions-report.ts`, `src/hooks/use-stock-transactions-settings.ts`
- Modify: the hooks that write stock data, so the report refetches: add `['stock-transactions-report']` wherever `['stock-performance']` or the stock positions key is invalidated (stock transactions, stock import, splits, dividends, price refresh)

**Interfaces (produced, used by Tasks 6–8):**

```ts
export type SaleClass = 'all' | 'taxable' | 'exempt';
export type LedgerKind = 'all' | 'buy' | 'sell' | 'dividend';
export interface SaleView {
  sale: StockReportSale;
  matches: StockReportMatch[]; // the matches the class filter keeps
  quantity: number; proceeds: number; cost: number; fees: number; resultAfterFees: number; // of those matches
  kind: 'taxable' | 'exempt' | 'mixed' | null; // null when the test is off
  exemptQuantity: number;
}
export function saleViews(sales: StockReportSale[], filter: { search: string; bucket: StockReportBucket | null; saleClass: SaleClass }): SaleView[];
export function totalOf(views: SaleView[]): { proceeds: number; cost: number; fees: number; resultAfterFees: number };
export function ledgerRows(rows: StockReportLedgerRow[], filter: { search: string; bucket: StockReportBucket | null; kind: LedgerKind }): StockReportLedgerRow[];
export function yearPeriod(year: number): { fromDay: number; toDay: number }; // UTC, both ends included
export function holdingSpan(buyDay: number, sellDay: number): { years: number; months: number; days: number }; // whole months, days only under one month
export function parseHoldingMonths(text: string): number | null; // whole number 1..600, otherwise null
export function openLotGroups(lots: StockReportOpenLot[], today: number): { waiting: StockReportOpenLot[]; passed: StockReportOpenLot[]; openValue: number; passedValue: number };
// stock-transactions-csv.ts
export function ledgerCsv(rows: StockReportLedgerRow[], reportCurrency: string): string;
export function ledgerCsvFilename(fromDay: number, toDay: number): string;
// hooks
export function useStockTransactionsReport(request: StockTransactionsReportRequest | null); // { data, isLoading, isPlaceholderData, isError, refetch }, key ['stock-transactions-report', request, fxRevision], keepPreviousData — copy use-stock-performance.ts
export function useStockTransactionsSettings(); // { settings, isLoading, save(next) }, key ['stock-transactions-settings'], optimistic update
```

Rules: search matches ticker or company name, case-insensitive, trimmed; a bucket keeps rows with `fromDay <= day <= toDay`; with `saleClass` other than `all` a sale keeps only its matching matches and disappears when none match; an uncounted sale (`counted === false`) is listed with zero sums and `kind` from its matches when known; `waiting` is sorted by `passDay`, `passed` newest pass first, lots without a `value` add nothing to either sum. CSV: header `Date,Type,Ticker,Name,Quantity,Price,Currency,Rate,Amount,Report currency,Fee`, `Date` as `YYYY-MM-DD`, `Type` `Buy` / `Sell` / `Dividend`, empty string for `null`, RFC 4180 quoting, `\n` line ends, no BOM (the caller adds it); filename `stock_transactions_2026-01-01_2026-12-31.csv`. Saving the settings is a preference write: it invalidates only its own key (the report refetches because its request changes); it does not record a snapshot.

- [ ] **Step 1: Failing tests** (Vitest, small hand-built objects): `saleViews` — search by ticker and by name; bucket filter; class `taxable` on a mixed sale keeps one match and its sums, class `exempt` drops a fully taxable sale; `kind` is `mixed` with `exemptQuantity` 20 for a 25-share sale with matches 20 exempt + 5 taxable; `totalOf` adds the views. `ledgerRows` — kind and search. `yearPeriod(2026)` → `Date.UTC(2026,0,1)/1000` and `Date.UTC(2026,11,31)/1000`. `holdingSpan` — 2021-03-15 → 2026-02-10 is `{years: 4, months: 10, days: 0}`; 12 days apart is `{0, 0, 12}`. `parseHoldingMonths` — `'18'` → 18; `''`, `'0'`, `'601'`, `'1.5'`, `'abc'` → null. `openLotGroups` — one passed, two waiting, an unpriced lot ignored in the sums. `ledgerCsv` — a name containing a comma and a quote is quoted; a manual dividend has empty Quantity and Price; `ledgerCsvFilename`.
- [ ] **Step 2: Run** `npm test -- stock-transactions` → fail. **Step 3: Implement.** **Step 4: Run** `npm test`, `npm run lint`, `npm run typecheck`, `npm run format:check`.
- [ ] **Step 5: Commit** `feat(stock-report): frontend utilities and hooks`.

### Task 6: Page, navigation, period, hero, summary, stats

**Files:**
- Create: `src/pages/StockTransactions.tsx`, `src/components/stock-transactions/PeriodControls.tsx`, `ResultHero.tsx`, `ReportSettings.tsx`, `SummaryTable.tsx`, `ExplainTip.tsx`, `ReportStats.tsx`, `src/i18n/locales/<lang>/stockTransactions.json` for all ten languages
- Modify: `src/App.tsx` (route `/reports/stock-transactions`), `src/components/common/nav-config.ts` (+ its test if it lists items), `src/i18n/index.ts` (namespace `stockTransactions`), `src/i18n/locales/<lang>/common.json` (`nav.stockTransactions`; cs "Akciové transakce", en "Stock transactions")

**Interfaces:**
- Consumes: Task 5 hooks and utils; `PageHead`, `HeroCard`/`HeroValue`, `Stats`/`Stat`, `Segmented`, `Switch`, `Select`, `Input`, `Tooltip`, `Alert`, `ChangeBadge`, `EmptyState`, `ConversionQualityNote`, `useFormat`, `useCurrency`.
- Produces: the page component owning the state `{ mode: 'year' | 'custom', year, fromDay, toDay, bucket: StockReportBucket | null }` and passing `report`, `settings`, `currency` down; `ExplainTip({ label, children })` (an info button with a wide wrapping tooltip: shown on hover and focus, pinned by a click, closed by Esc or an outside click); `StockTransactions` leaves three named slots that Tasks 7 and 8 fill: after the stats, `<ActivityRow />`, `<HoldingTestCard />`, `<TransactionsCard />` — until then render nothing there.

What to build (spec D1–D3, D7–D9, D15, D17, D19; prototype sections "page-head", "hero", "stats"): nav item `stockTransactions` in the `tools` group right after `stocksAnalysis`, icon `Receipt`, `prefKey: 'investments'`; year stepper bounded by `firstTransactionDay` and the current year (copy the inline stepper of `src/pages/SpendingPlans.tsx`) with a `Rok` / `Vlastní období` segment and two date inputs; the hero with the settings strip (cost method segment with its `ExplainTip`, the holding-test switch with its `ExplainTip`, the length select with the six presets and `Vlastní…`, the months input validated with `parseHoldingMonths`); the summary table with the five columns and the rows by test state; the loss alert; the note line; four stats; `ConversionQualityNote`; the empty states of D19. Settings changes call `save` at once. Tooltip copy in Czech is in the prototype's `TIPS`; the holding-test text names no country except Czechia (spec D7).

- [ ] **Step 1:** Add the namespace and nav key in all ten locales and register it; run `npm test -- locales` → passes.
- [ ] **Step 2:** Build the components and the page; wire route and nav.
- [ ] **Step 3: Run** `npm run lint && npm run typecheck && npm run format:check && npm test`.
- [ ] **Step 4: Commit** `feat(stock-report): page, period, result summary and stats`.

### Task 7: Activity chart, result by ticker, holding-test card and dialog

**Files:**
- Create: `src/components/stock-transactions/ActivityCard.tsx`, `TickerResultsCard.tsx`, `HoldingTestCard.tsx`, `HoldingTestDialog.tsx`
- Modify: `src/pages/StockTransactions.tsx`, the ten `stockTransactions.json` files

**Interfaces:**
- Consumes: `report.buckets`, `report.tickers`, `report.openLots`, `report.today`, `openLotGroups`, `MoonyBarChart` (`bars`, `names`, `formatValue`, `onBarClick`, `muted` per bar), `AssetLogo`, `Dialog`, `Segmented`, the page's `bucket` state and setter.
- Produces: the two-card row and the card + dialog in the page's slots.

What to build (spec D10–D12; prototype "rep-grid", "upcoming", "up-modal"): bars of buys (`s1`) and sells (`s3`) per bucket, hover tooltip with buys, sells and result, a click toggles the page's bucket and mutes the other bars, bucket labels through `useFormat` (short month, the year for yearly buckets); ticker rows with logo, name, ticker and quantity, a bar from a common zero line, the signed result, the sub-line of D11 when the test is on, six rows then "Zobrazit všech N titulů"; the holding-test card only when the test is on: the four nearest waiting lots as milestone rows (date chip dark when at most seven days away), the foot line, the button, and the 800 px dialog with the two tabs and the table of D12.

- [ ] **Step 1:** Add the strings in all ten locales. **Step 2:** Build. **Step 3: Run** the frontend gates. **Step 4: Commit** `feat(stock-report): activity chart, ticker results and holding-test detail`.

### Task 8: Table card — sale results, ledger, export

**Files:**
- Create: `src/components/stock-transactions/TransactionsCard.tsx`, `SaleResultsTable.tsx`, `LedgerTable.tsx`
- Modify: `src/pages/StockTransactions.tsx`, the ten `stockTransactions.json` files

**Interfaces:**
- Consumes: `saleViews`, `totalOf`, `ledgerRows`, `holdingSpan`, `ledgerCsv`, `ledgerCsvFilename`, `FilterBadge`, `Badge`, `ChangeBadge`, `AssetLogo`, table primitives, the save flow of `src/components/common/ExportButton.tsx` (`save()` from `@tauri-apps/plugin-dialog`, `writeTextFile(path, '﻿' + csv)`).
- Produces: the card in the page's last slot.

What to build (spec D13, D14, D16; prototype "card--table"): a view segment `Výsledek prodejů` / `Všechny transakce`; search; the `FilterBadge` for the page's bucket with its clear button; in the results view the class segment (test on only), expandable rows (click, Enter, Space; `aria-expanded`), the columns and badges of D13, the "Chybí nákup pro n ks" note and the alert naming `uncoveredTickers`, "—" for an uncounted sale, the totals row; in the ledger the kind segment with `Dividendy`, the columns of D14, and `Exportovat CSV`, which saves the rows shown and confirms with a toast. The export button is not shown in the results view. Column widths must hold at 1080 px (the prototype's `@media (max-width: 1239px)` grid is the reference).

- [ ] **Step 1:** Strings in all ten locales. **Step 2:** Build. **Step 3: Run** the frontend gates. **Step 4: Commit** `feat(stock-report): sale results, ledger and CSV export`.

### Task 9: Documents

**Files:**
- Create: `docs/architecture/decisions/0036-stock-report-fifo-lots.md`
- Modify: `docs/architecture/decisions/README.md` (index), `docs/design-system/README.md` (§6: the explanatory tooltip; §7: a "Report (stock transactions)" archetype row; §5: the new item in Nástroje), `README.md` (feature list), `docs/architecture/overview.md` if it lists services or pages

- [ ] **Step 1:** Write ADR 0036 in the format of the newest ADRs: context (a holding period belongs to a purchase; the cost basis is a weighted average by ADR 0001), decision (oldest-first lots derived on read, only in the report service; the cost method is a display choice of the report; nothing stored), consequences (the report's result can differ from the realized gain elsewhere and includes fees; lots are per position, not per broker).
- [ ] **Step 2:** Update the living docs named above to describe what was built (no line numbers, no file sizes).
- [ ] **Step 3: Commit** `docs: stock transactions report (ADR 0036, design system, README)`.
