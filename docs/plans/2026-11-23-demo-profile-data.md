# Demo profile data generator: build plan

Spec: `docs/specs/2026-11-23-demo-profile-design.md`, section "Data set: Alex Meyer".
Scope of this plan: the Rust generator and a way to run it from the command line. The first-run
choice screen, the demo mark and the demo folder inside the app are a later plan.

Schema and service reference for every table: the research note the controller hands to each
task (`seeding-reference.md`). `src-tauri/src/db/schema.snapshot` is the current schema.

## Shape

New library module `src-tauri/src/services/demo/` (registered in `services/mod.rs`):

| File | Holds |
|---|---|
| `mod.rs` | `DemoOptions { language: String, now: i64, attachments: bool }`, `pub fn seed(conn: &Connection, opts: &DemoOptions) -> Result<DemoSummary>` calling the parts in order inside one SQL transaction; `DemoSummary` = row counts + net worth in EUR; persona constants |
| `clock.rs` | day helpers: `today`, `months_ago(n, day_of_month)`, `days_ago(n)`, calendar month iteration; all UTC midnight |
| `rng.rs` | the deterministic LCG from `bin/seed_demo.rs` (same seed → same data) |
| `text.rs` | `enum T { … }` + `fn t(lang, T) -> &'static str` for en, cs, de, es, it, nb, pl, sv, ja, zh; unknown language falls back to en |
| `prices.rs` | month-end anchor prices per ticker by absolute date (real, approximate) through the last month the author knows, then a fixed gentle drift; `price_on(ticker, day) -> (f64, &'static str currency)` with deterministic daily wiggle between anchors; `usd_per_eur(day)` the same way; BTC and ETH in USD |
| `bank.rs` | accounts, payments, month-end balance records |
| `planning.rs` | budgets with one past version, spending plans + tags on payments, cashflow items, projection settings left at defaults |
| `invest.rs` | stocks, fees, sells, split, dividends, tags, watchlist, `stock_data`, crypto, bond |
| `assets.rs` | properties + valuations + costs + links, loans + extra repayment, insurance, other assets, documents (only when `attachments`) |
| `history.rs` | USD rate history + current rate, daily ticker value rows, `rebuild_days`, today's live row, the two markers |

Rules that bind every task:
- Business logic only through existing services where one exists (reference note); raw INSERT
  only where the note says there is no service. `AppError`, no `.unwrap()` outside tests.
- Deterministic: no wall clock, no `Uuid::new_v4()`-dependent ordering in results, no random
  source other than `rng.rs`. Everything is relative to `opts.now`.
- All money EUR except: MSFT, AAPL, NVDA, PYPL trades and the travel account in USD; crypto in USD.
- Never rename system categories. Every payment gets a `category_id`.
- A test builds the database with `Connection::open_in_memory()`, `PRAGMA foreign_keys = ON`,
  `moony_lib::db::run_migrations`, and seeds with `attachments: false`.

## History mechanics (do not skip)

- `portfolio_metrics_history` is never written by hand. Insert dated facts, then daily rows in
  `stock_value_history` / `crypto_value_history` through
  `ticker_history::rebuild_stock_ticker_history` (stocks) and `plan_ticker_history` +
  `apply_ticker_plan` (crypto) with `price_at = prices::price_on`, from each first trade to
  yesterday; weekends carry the price.
- Then `history_days::rebuild_days(conn, 0, today_start)`, then today's live row
  (`net_worth::compute_breakdowns` + `portfolio_history::upsert_snapshot(.., Live, ..)`) and
  today's ticker rows, the way `commands/portfolio.rs::update_todays_snapshot` does.
- Then `history_plan::store_rebuild_version(conn)` and
  `history_plan::store_settled_through(conn, yesterday_start)`, so the first unlock does not
  rebuild everything from the network.
- Bank history comes only from `bank_balance_records`: create each account with balance "0",
  then `record_balance` month end by month end, last one today.
- USD needs `exchange_rate_history` rows (units per 1 EUR) for every day from 12 days before
  the first dated fact (the flat purchase, M60) to today, and a current `exchange_rates` row
  with `fetched_at = now`, followed by `currency::load_rates_from_db(conn)` before anything
  converts.
- `stock_data` rows: `quote_currency` = `currency`, `fetched_at = now`, `metadata_fetched_at =
  now`, plus `short_name`, `long_name`, `sector`, `industry`, `exchange`, `quote_type`,
  `previous_close`.
- NVIDIA: trades before 2024-06-10 in original units, then the 10:1 split through
  `stock_splits::record_manual`. First read how a later Yahoo detection of the same
  `(ticker, effective_day)` behaves; if it could create a second pending split, buy NVIDIA
  after the split instead and say so in the report.

## Tasks (one writer at a time, each ends with a commit)

1. **Scaffold.** `mod.rs`, `clock.rs`, `rng.rs`, `prices.rs`, `text.rs` with every string the
   later tasks need (derive the list from the spec; about 80 keys), tests: every key non-empty
   in all ten languages, `price_on` positive and continuous for every ticker over six years
   including two years past the last anchor, `seed` on an empty plan runs. Module registered.
2. **Bank and planning.** `bank.rs`, `planning.rs`. Tests: no month-end balance below zero,
   every payment categorized, salary present in each of 48 months, payment count between
   1 800 and 2 800, budgets have a past version, tagged payments exist for each open plan,
   two runs with the same `now` give identical sums.
3. **Investments and assets.** `invest.rs`, `assets.rs`. Tests: position quantities match the
   trades, PayPal closed, NVIDIA part sold, dividends present for payers, links property ↔ loan
   and property ↔ insurance exist, a deadline exists for the car policy and the savings rate.
4. **History and the command line.** `history.rs`; `bin/seed_demo.rs` gains `--product-demo`
   and `--lang <code>` (default `en`), which create the encrypted profile for Alex Meyer with
   the existing demo password and call `demo::seed` with `attachments: true`; the old data set
   stays the default. Tests: a history row for every day from M60 to today, today's row live,
   both markers set, net worth in EUR between 240 000 and 360 000, the whole seed under ten
   seconds in a debug test, a run with `now` 400 days later still passes.

Gates per task: `cargo fmt`, `cargo clippy --lib --bins -- -D warnings` on the touched crate,
`cargo test --lib demo`. The full suite runs once at the end.
