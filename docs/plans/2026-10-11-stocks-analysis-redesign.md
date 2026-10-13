# Stocks Analysis Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the stocks analysis page around a full-width performance card (TWR, money-weighted return, change of value, drawdown, legend table), a scope card for the chip filters, contribution and deposits cards, custom periods and an optional index line, and enforce one tag per tag group.

**Architecture:** The backend gets a tag service with the one-tag-per-group rule and a migration that cleans legacy data, a `stock_performance` service whose daily engine generalises the existing TWR chain-linking to series, contributions, money-weighted return and deposits, and an on-demand benchmark fetch. The frontend gets pure helpers in `src/utils/stock-performance.ts`, a two-panel Recharts chart with drag selection, and the page split into focused components under `src/components/stocks-analysis/`.

**Tech Stack:** Tauri 2 (Rust, rusqlite/SQLCipher, specta), React 19 + TypeScript, TanStack Query, Recharts 3, react-i18next (10 locales), Vitest, cargo test.

**Spec:** `docs/specs/2026-10-11-stocks-analysis-redesign-design.md`. The HTML prototype used during the design (and as a reference by the tasks) was deleted after the merge at the owner's request.

## Global Constraints

- Work only in the worktree `../Moony-tauri-stocks-analysis` on branch `feat/stocks-analysis-redesign`. Never touch, commit in, reset or switch branches in `../Moony-tauri` (other sessions use it).
- The ten rules of `AGENTS.md` apply. In particular: `invoke()` only in `src/lib/tauri-api.ts`; `shared/schema.ts` is the wire contract and every wire change follows the 7-step procedure in `docs/standards/type-contract.md` (Rust struct with per-field `#[serde(rename = "camelCase")]`, registered in `src-tauri/src/bindings.rs`, `cd src-tauri && cargo test generate_bindings -- --ignored`, hand-mirrored into `shared/schema.ts`, wrapper in `tauri-api.ts`); thin commands, logic in `src-tauri/src/services/` taking `&Connection`; `AppError`, never `Result<_, String>`; no `.unwrap()` outside tests; migrations append-only, next number `015`; money is decimal TEXT on the wire (`crate::services::reporting::money_in_currency(amount, currency)`), timestamps unix seconds at UTC midnight; never convert 1:1 or assume CZK (`services/fx.rs` `History::convert_on_tracked`).
- Every user-facing string goes into all ten locales (`en cs de es it nb pl sv ja zh`) of the namespace; `src/i18n/locales.test.ts` enforces parity. Tasks 1, 7, 8 and 9 write `en` and `cs` only (exact copy in the Copy appendix); Task 11 fills the other eight and deletes obsolete keys everywhere. `npm test` is expected to fail the locale parity test between Task 1 and Task 11; every other test must pass.
- Copy rules (design system §10): sentence case, no exclamation marks, numbers through `useFormat()` (`money`, `percent`, `day`), true minus, signed percentage points as "+ 2,5 p. b." (`fmt.number` + the locale's unit key `stocksAnalysis.pp`). Colour carries meaning only: `gain`/`loss` for signed values, series in `s1`–`s4`, the portfolio line `chart-line` dashed, the index `ink-4` dotted.
- Gates before every commit: `npm run lint`, `npm run typecheck`, `npm run format` then `npm run format:check`; Rust tasks also `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test`. The pre-commit hook runs lint-staged, typecheck and `cargo fmt --check`.
- Commit messages: Conventional Commits, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Agents do not commit.** The orchestrator reviews and commits each task (parallel agents share one worktree and one pre-commit hook). Agents report the exact list of files they changed.

## Execution phases

| Phase | Tasks (parallel within a phase) | Model |
|---|---|---|
| A | Task 1 (tags backend) ‖ Task 5 (frontend helpers) | sonnet ‖ sonnet |
| B | Task 2 + Task 3 (sector, performance engine) ‖ Task 6 (chart) | sonnet ‖ sonnet |
| C | Task 4 (benchmark) ‖ Task 9 (tag dialogs) | sonnet ‖ sonnet |
| D | Task 7 (page, scope and performance cards) | sonnet |
| E | Task 8 (contribution, flows, tables) ‖ Task 10 (retire old TWR path) | sonnet ‖ sonnet |
| F | Task 11 (eight locales) ‖ Task 12 (docs) | sonnet ‖ haiku |
| G | Task 13 (verification, review, merge) | orchestrator + sonnet reviewer |

---

### Task 0: Documents (orchestrator)

- [ ] Commit `docs/specs/2026-10-11-stocks-analysis-redesign-design.md`, `docs/specs/2026-10-11-stocks-analysis-prototype.html` and this plan:

```bash
git add docs/specs/2026-10-11-stocks-analysis-redesign-design.md docs/specs/2026-10-11-stocks-analysis-prototype.html docs/plans/2026-10-11-stocks-analysis-redesign.md
git commit -m "docs: stocks analysis redesign spec, prototype and plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 1: One tag per group — migration and tag service

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const `MIGRATION_015`, Vec entry `("015_one_tag_per_group", MIGRATION_015)`, tests in the existing test module)
- Create: `src-tauri/src/services/stock_tags.rs`; Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/stock_tags.rs` (thin wrappers only; `get_stocks_analysis` / `get_tag_metrics` unchanged)
- Modify: `src-tauri/src/models/stock_tags.rs` (`validate()` on `InsertStockTag`, `InsertStockTagGroup`)
- Modify: `src/i18n/locales/en/common.json`, `src/i18n/locales/cs/common.json` (keys under `validation`)
- Modify: `docs/architecture/database.md` (the `stock_investment_tags` row: "at most one tag per tag group per investment (tag service; migration 015 removed older duplicates)")

**Interfaces — Produces:**
- `services::stock_tags::{list_groups, create_group, update_group, delete_group, list_tags, create_tag, update_tag, delete_tag, investment_tags, set_investment_tags}` — same SQL and return types as today's command bodies, each taking `&Connection` first.
- Error keys: `validation.tagNameRequired`, `validation.tagNameTooLong`, `validation.tagNotFound`, `validation.oneTagPerGroup`, `validation.tagGroupConflict`; `AppError::NotFound` for a missing investment, tag or group.

- [ ] **Step 1: Migration test (failing).** In the migrations test module, follow the pattern of the existing tests that apply migrations up to a name (e.g. the ones filtering `*name < "014_balance_records"`): build a DB at `014`, insert groups `g1`, `g2`, tags `a` (g1, name "Alfa"), `b` (g1, "Beta"), `c` (g2), `d` (no group), `e` (g1, "Echo"), investments `i1`, `i2`, `i3`, and assignments with `created_at`:
  - `i1`: `b`@100, `a`@200, `c`@50, `d`@10 → after: `b`, `c`, `d` (oldest in g1 wins)
  - `i2`: `a`@100, `b`@100, `e`@100 → after: `a` (tie by name: "Alfa")
  - `i3`: `e`@300 → unchanged
  Then apply `015` and assert the exact remaining `(investment_id, tag_id)` set.
- [ ] **Step 2: Run** `cd src-tauri && cargo test migrations` — expect FAIL (no `015`).
- [ ] **Step 3: Implement the migration:**

```rust
const MIGRATION_015: &str = r#"
-- A tag group is one partition of the portfolio: keep the first assigned tag of each group per
-- investment (earliest created_at, then tag name) and delete the others. Ungrouped tags stay.
DELETE FROM stock_investment_tags
WHERE EXISTS (
    SELECT 1
    FROM stock_tags t
    JOIN stock_investment_tags o ON o.investment_id = stock_investment_tags.investment_id
    JOIN stock_tags ot ON ot.id = o.tag_id
    WHERE t.id = stock_investment_tags.tag_id
      AND t.group_id IS NOT NULL
      AND ot.group_id = t.group_id
      AND o.tag_id <> stock_investment_tags.tag_id
      AND (o.created_at < stock_investment_tags.created_at
           OR (o.created_at = stock_investment_tags.created_at AND ot.name < t.name))
);
"#;
```

- [ ] **Step 4: Run** the migration test and the schema snapshot test (`cargo test baseline_matches_schema_snapshot`) — both PASS (no schema change).
- [ ] **Step 5: Tag service tests (failing).** In `services/stock_tags.rs` `#[cfg(test)]`, an in-memory schema with `stock_tag_groups`, `stock_tags`, `stock_investments`, `stock_investment_tags` (copy the column definitions from the baseline migration). Tests:
  - `set_tags_rejects_two_tags_of_one_group` → `Err(AppError::Validation(k))` with `k == "validation.oneTagPerGroup"`, nothing written.
  - `set_tags_keeps_created_at_of_kept_assignments` → assign `[a]` (created_at fixed by inserting directly with 100), call `set_investment_tags(conn, "i1", &["a", "d"])`, `a` still has 100, `d` is new.
  - `set_tags_removes_unselected` and `set_tags_unknown_tag_fails` (`validation.tagNotFound`) and `set_tags_unknown_investment_is_not_found`.
  - `moving_a_tag_into_a_group_with_a_conflict_fails` → `i1` has `a` (g1) and `d` (no group); `update_tag(conn, "d", InsertStockTag { group_id: Some("g1"), .. })` → `validation.tagGroupConflict`, group unchanged.
  - `moving_a_tag_without_conflict_succeeds`, `renaming_keeps_group_check_off` (same group → no check).
  - `insert_tag_validate`: empty / whitespace name → `validation.tagNameRequired`; 61 characters → `validation.tagNameTooLong`.
- [ ] **Step 6: Implement** the service. Move each command body into a service function (same SQL), then add:

```rust
/// The tags of `tag_ids` that share a group with another of them, i.e. a violation of the
/// one-tag-per-group rule.
fn check_one_per_group(conn: &Connection, tag_ids: &[String]) -> Result<()> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for id in tag_ids {
        let group: Option<Option<String>> = conn
            .query_row("SELECT group_id FROM stock_tags WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        let Some(group) = group else {
            return Err(AppError::Validation("validation.tagNotFound".into()));
        };
        if let Some(g) = group {
            if seen.insert(g, id.clone()).is_some() {
                return Err(AppError::Validation("validation.oneTagPerGroup".into()));
            }
        }
    }
    Ok(())
}
```

  `set_investment_tags`: dedupe `tag_ids`, check the investment exists (`NotFound`), `check_one_per_group`, then in one transaction (`conn.unchecked_transaction()`) delete assignments whose `tag_id` is not in the list and `INSERT OR IGNORE` the listed ones with `unixepoch()`.
  `update_tag`: when `data.group_id` is `Some(g)` and differs from the stored group, run

```sql
SELECT COUNT(*) FROM stock_investment_tags mine
JOIN stock_investment_tags other ON other.investment_id = mine.investment_id
JOIN stock_tags ot ON ot.id = other.tag_id
WHERE mine.tag_id = ?1 AND other.tag_id <> ?1 AND ot.group_id = ?2
```

  and return `validation.tagGroupConflict` when it is above 0. `InsertStockTag::validate` / `InsertStockTagGroup::validate`: `name.trim()` empty → `validation.tagNameRequired`; `name.trim().chars().count() > 60` → `validation.tagNameTooLong`. Commands call `data.validate()?` and store the trimmed name.
- [ ] **Step 7: Make commands thin** (`db.with_conn(|conn| stock_tags::set_investment_tags(conn, &investment_id, &tag_ids))` etc.).
- [ ] **Step 8: Copy** — add the `validation` keys from the Copy appendix (§A) to `en/common.json` and `cs/common.json`.
- [ ] **Step 9: Gates** (`cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `npm run lint`, `npm run typecheck`, `npm run format:check`). Report changed files.

---

### Task 2: Sector on analysis items

**Files:** `src-tauri/src/models/stock_tags.rs`, `src-tauri/src/services/position_reports.rs`, `shared/schema.ts`, `shared/generated-types.ts` (regenerated), `src/utils/stock-analysis.ts` (`toAnalysisPosition` passes `sector` through if it maps fields).

**Interfaces — Produces:** `StockInvestmentWithTags.sector: string | null` (wire `sector`).

- [ ] **Step 1: Failing test** in `position_reports.rs` tests (extend the existing `stocks_analysis` test setup with a `stock_data` table, or add one): a position with `stock_data.sector = ' Technology '` reports `Some("Technology")`; one with `''` or no row reports `None`.
- [ ] **Step 2: Implement:** `#[serde(rename = "sector")] pub sector: Option<String>` on `StockInvestmentWithTags` (doc comment: "Sector from the cached quote data; `None` for ETFs and unknown"). Load `SELECT ticker, sector FROM stock_data` into a `HashMap<String, String>` (trimmed, non-empty) next to `stock_names`, match on the position's ticker.
- [ ] **Step 3:** `cargo test generate_bindings -- --ignored`; mirror `sector: string | null;` into `shared/schema.ts` (with the same doc comment); fix TypeScript fixtures that build `StockInvestmentWithTags` literals (`grep -rn "yearlyDividend" src --include=*.test.ts`).
- [ ] **Step 4: Gates.**

---

### Task 3: Performance engine and `get_stock_performance`

**Files:**
- Create: `src-tauri/src/services/stock_performance.rs` (+ `services/mod.rs`)
- Create: `src-tauri/src/models/stock_performance.rs` (+ `models/mod.rs` re-exports as the other model modules do)
- Modify: `src-tauri/src/commands/investments.rs` (new command `get_stock_performance`), `src-tauri/src/lib.rs` (register next to `get_stock_twr`), `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts` (`investmentsApi.getStockPerformance(request)`)
- Modify: `src-tauri/src/services/investments.rs` only to make the helpers it already has reusable: `DayHolding`, `lookup_ticker_at`, `ReportFx`, `load_twr_history` and the history SQL become `pub(crate)` (or move into the new module and are imported back). `twr_series` keeps working until Task 10.

**Interfaces — Produces (Rust, wire names in comments):**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PerformanceSeriesSpec {
    pub key: String,
    #[serde(rename = "investmentIds")]
    pub investment_ids: Vec<String>,
    /// Compute this series in another currency (the native return of a currency group); it then
    /// has no contribution.
    #[serde(rename = "currency")]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPerformanceRequest {
    #[serde(rename = "fromTs")]
    pub from_ts: i64,
    #[serde(rename = "toTs")]
    pub to_ts: i64,
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
    #[serde(rename = "series")]
    pub series: Vec<PerformanceSeriesSpec>,
}
// validate(): from_ts < to_ts else "validation.periodInvalid"; series.len() <= 64 else
// "validation.tooManySeries"; keys non-empty and unique else "validation.seriesKeyInvalid".

pub struct PerformanceSeries { key: String, twr: Vec<f64>, contribution: Option<Vec<f64>> }
pub struct PositionPerformance {
    investment_id: String /* investmentId */, twr: f64, contribution: f64,
    start_value: Option<String> /* startValue */, end_value: Option<String> /* endValue */,
}
pub struct StockPerformance {
    currency: String, fx_quality: ConversionQuality /* fxQuality */,
    dates: Vec<String>, portfolio: Vec<f64>, values: Vec<Option<String>>,
    net_flows: Vec<String> /* netFlows */, mwr: Vec<f64>,
    series: Vec<PerformanceSeries>, positions: Vec<PositionPerformance>,
}
pub fn stock_performance(conn: &Connection, request: &StockPerformanceRequest) -> Result<StockPerformance>;
```

TypeScript (`shared/schema.ts`): the same shapes, `contribution: number[] | null`, `values: (string | null)[]`, `startValue/endValue: string | null`, `currency?: string | null` on the spec. Wrapper: `getStockPerformance: (request: StockPerformanceRequest) => tauriInvoke<StockPerformance>('get_stock_performance', { request })`.

- [ ] **Step 1: Failing tests** (in-memory schema: `stock_investments`, `stock_value_history`, `exchange_rate_history` as in the existing TWR tests of `services/investments.rs` — reuse their setup helpers by copying them). Helper `perf(conn, from, to, series)` builds the request in `CZK`. Rates: store EUR-referenced rates exactly as the existing TWR tests do. Tests:
  1. `portfolio_twr_matches_value_ratio_without_flows`: AAPL 10 shares 100 → 110 over 2 days in CZK-quoted test data → `portfolio.last() == 10.0` (±1e-9), `dates[0]` is the start day.
  2. `a_deposit_is_not_return`: day 1 holds 10 @100, day 2 holds 20 @100 (bought 10) → TWR 0, `netFlows.last() == "1000.00"`, `mwr.last() == 0`.
  3. `contributions_of_a_partition_sum_to_the_portfolio`: three positions with different daily moves and one buy in the middle; series `a = [p1]`, `b = [p2, p3]`; `contribution(a).last() + contribution(b).last() == portfolio.last()` (±1e-9), and the per-position contributions sum to it as well.
  4. `series_twr_is_its_own_chain`: series `[p2]` equals the TWR computed by hand from its two daily returns.
  5. `mwr_falls_below_twr_when_money_arrives_before_a_drop`: hold 10 @100; day 2 price 120 and buy 90 more; day 3 price 96 → TWR = 1.2 × 0.8 − 1 = −4 %; MWR is below it (assert `mwr.last() < portfolio.last()`), and equals `(V3 − V0 − F) / (V0 + F · 2/3)` with `V0 = 1000`, `F = 90 × 120 = 10800`, `V3 = 100 × 96 = 9600` → `(9600 − 1000 − 10800) / (1000 + 7200) = −26.83 %` (±0.01).
  6. `native_currency_series_excludes_the_rate_move`: one USD position, price flat, USD/CZK rate up 5 % → report-currency series ≈ +5 %, the same tickers with `currency: Some("USD")` → 0 and `contribution == None`.
  7. `range_is_clamped_to_history` and `emits_at_most_400_points_and_always_the_last_day` (a 1000-day history → `dates.len() <= 401`, last date is the last day).
  8. `validate_rejects_bad_requests` for the three validation keys.
- [ ] **Step 2: Run** `cargo test stock_performance` — FAIL.
- [ ] **Step 3: Implement** the engine. Same data rules as `compute_twr_for_tickers` (history rows `ticker, recorded_at, quantity, price, currency` plus the last row before the range; a holding's value is `quantity × price` converted on its day; a flow is the quantity change × that day's native price converted on that day; a failed conversion drops the holding from that day only). Core:

```rust
/// Daily moves of every held position in one currency. Index 0 is the base day.
struct Daily {
    days: Vec<i64>,
    /// [k][p]: V_p(k−1) + F_p(k) for k ≥ 1; None when a conversion failed that day.
    base: Vec<Vec<Option<f64>>>,
    /// [k][p]: V_p(k) − V_p(k−1) − F_p(k).
    gain: Vec<Vec<Option<f64>>>,
    /// [k][p]: V_p(k), k ≥ 0.
    value: Vec<Vec<Option<f64>>>,
    /// [k][p]: F_p(k).
    flow: Vec<Vec<Option<f64>>>,
}

fn chain(daily: &Daily, members: &[usize]) -> Vec<f64> {
    let mut factor = 1.0;
    let mut out = vec![0.0];
    for k in 1..daily.days.len() {
        let (mut b, mut g) = (0.0, 0.0);
        for &p in members {
            if let (Some(bp), Some(gp)) = (daily.base[k][p], daily.gain[k][p]) {
                b += bp;
                g += gp;
            }
        }
        if b > 1e-9 {
            factor *= 1.0 + g / b;
        }
        out.push((factor - 1.0) * 100.0);
    }
    out
}

fn contribution(daily: &Daily, members: &[usize], all: &[usize]) -> Vec<f64> {
    let (mut growth, mut c) = (1.0, 0.0);
    let mut out = vec![0.0];
    for k in 1..daily.days.len() {
        let (mut b_all, mut g_all, mut g_s) = (0.0, 0.0, 0.0);
        for &p in all {
            if let (Some(bp), Some(gp)) = (daily.base[k][p], daily.gain[k][p]) {
                b_all += bp;
                g_all += gp;
                if members.contains(&p) {
                    g_s += gp;
                }
            }
        }
        if b_all > 1e-9 {
            c += growth * g_s / b_all;
            growth *= 1.0 + g_all / b_all;
        }
        out.push(c * 100.0);
    }
    out
}
```

  Money-weighted return with running sums (flow of day `j` counts `k − j + 1` of the `k` days): keep `sum_f = Σ F_j` and `sum_fj = Σ F_j · j`; at day `k` the weighted flows are `(k + 1) · sum_f − sum_fj` divided by `k`; `mwr = (V_k − V_0 − sum_f) / (V_0 + weighted) · 100`, 0 when the denominator is ≤ 1e-9. Emission: `every = ceil(n / 400)`; emit index 0, every `k % every == 0`, and the last day. `values[k]` is `Some(money_in_currency(Σ known V_p(k)))` (`None` only when every held position failed conversion that day); `net_flows[k]` is `money_in_currency(Σ known F up to k)`. Positions: `twr = chain(&[p]).last()`, `contribution = contribution(&[p], all).last()`, `start_value = value[0][p]`, `end_value = value[last][p]` (money TEXT). Series with `currency`: build a second `Daily` for just its positions in that currency (load `History` once; `convert_on_tracked` works for any currency) and return `chain` only. Investment ids not held are ignored. Keep each function short; the module doc comment explains the decomposition `C_S = Σ G(s−1) · gain_S / base_all`.
- [ ] **Step 4: Command** (thin):

```rust
#[tauri::command]
pub async fn get_stock_performance(
    db: State<'_, Database>,
    request: StockPerformanceRequest,
) -> Result<StockPerformance> {
    request.validate()?;
    db.with_conn(|conn| stock_performance::stock_performance(conn, &request))
}
```

- [ ] **Step 5:** register the handler, the four types in `bindings.rs`, regenerate, mirror into `shared/schema.ts`, add the wrapper. Validation keys from Copy §A go into `en`/`cs` `common.json`.
- [ ] **Step 6: Gates.**

---

### Task 4: Benchmark — `get_stock_benchmark`

**Files:** `src-tauri/src/services/stock_performance.rs` (function `benchmark_series`), `src-tauri/src/models/stock_performance.rs` (`StockBenchmark`), `src-tauri/src/commands/investments.rs`, `lib.rs`, `bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts` (`investmentsApi.getStockBenchmark(ticker, fromTs, toTs, reportCurrency)`).

**Interfaces — Produces:**

```rust
pub struct StockBenchmark {
    ticker: String, currency: String, fx_quality: ConversionQuality /* fxQuality */,
    dates: Vec<String>, twr: Vec<f64>,
}
pub fn benchmark_series(
    conn: &Connection,
    ticker: &str,
    prices: &[crate::services::price_api::HistoricalPrice],
    from_ts: i64,
    to_ts: i64,
    report_currency: &str,
) -> Result<StockBenchmark>;
pub fn validate_benchmark_ticker(ticker: &str) -> Result<String>; // trimmed, uppercased
```

- [ ] **Step 1: Failing tests:** (a) prices on trading days only, a weekend inside the range → daily points with carry-forward, first point 0; (b) USD prices flat while USD/CZK rises 5 % → last twr ≈ 5 in CZK, 0 in USD; (c) no prices → empty `dates`/`twr`; (d) base is the last price on or before `from_ts` (a price three days before is used); (e) `validate_benchmark_ticker` accepts `CSPX.L`, `^GSPC`, `IWDA.L`, rejects `""`, `"a b"`, 25 characters (`validation.benchmarkTickerInvalid`).
- [ ] **Step 2: Implement.** Convert each day's carried price at that day's rate (`History::load` over the range, `convert_on_tracked`); `twr = (P_t·rate_t) / (P_0·rate_0) − 1` in percent; days without a convertible base are skipped. Command (async, network outside the DB lock):

```rust
#[tauri::command]
pub async fn get_stock_benchmark(
    db: State<'_, Database>,
    ticker: String,
    from_ts: i64,
    to_ts: i64,
    report_currency: String,
) -> Result<StockBenchmark> {
    let ticker = stock_performance::validate_benchmark_ticker(&ticker)?;
    let fetched = price_api::get_historical_stock_prices_yahoo(
        std::slice::from_ref(&ticker),
        from_ts - 10 * 86_400,
        to_ts + 86_400,
    )
    .await?;
    let prices = fetched.get(&ticker).cloned().unwrap_or_default();
    db.with_conn(|conn| {
        stock_performance::benchmark_series(conn, &ticker, &prices, from_ts, to_ts, &report_currency)
    })
}
```

  (`HistoricalPrice` must derive `Clone`; make it `pub` if it is not reachable.)
- [ ] **Step 3:** register, bindings, schema, wrapper, `validation.benchmarkTickerInvalid` in `en`/`cs`. **Step 4: Gates.**

---

### Task 5: Frontend helpers — `src/utils/stock-performance.ts`

**Files:** Create `src/utils/stock-performance.ts`, `src/utils/stock-performance.test.ts`.

**Interfaces — Produces** (all pure, timestamps are unix seconds at UTC midnight):

```ts
import type { StockTag, StockTagGroup } from '@shared/schema';

export const DAY = 86_400;
export type PeriodPreset = '3m' | 'ytd' | '1y' | '3y' | 'all' | 'custom';
export const PERIOD_PRESETS: readonly PeriodPreset[] = ['3m', 'ytd', '1y', '3y', 'all', 'custom'];
/** Start of "Vše": the backend clamps it to the first recorded day. */
export const ALL_FROM = Date.UTC(2000, 0, 1) / 1000;
export interface DayRange { from: number; to: number }

export function utcDay(ts: number): number;                       // floor to UTC midnight
export function isoDay(ts: number): string;                        // 'YYYY-MM-DD'
export function dayFromIso(iso: string): number | null;            // strict YYYY-MM-DD
export function presetRange(preset: Exclude<PeriodPreset, 'custom'>, today: number): DayRange;
// 3m: same day three months back (UTC), ytd: Jan 1, 1y: one year back, 3y: three years back,
// all: ALL_FROM; `to` is today. `from` is at most `to − DAY`.
export type RangeError = 'missing' | 'order';
export function parseCustomRange(fromIso: string, toIso: string, min: number, max: number):
  { range: DayRange } | { error: RangeError };
// Clamps to [min, max]; error 'order' unless from ≤ to − 2·DAY.

export type AxisTickKind = 'day' | 'month' | 'quarter' | 'year';
export interface AxisTick { t: number; kind: AxisTickKind; yearStart: boolean }
export function dateAxisTicks(t0: number, t1: number): AxisTick[];
// strictly inside (t0, t1): span ≤ 110 days → the 1st and 15th of every month ('day');
// ≤ 420 → month starts ('month'); ≤ 1300 → quarter starts (Jan/Apr/Jul/Oct, 'quarter');
// else Jan 1 ('year'). yearStart = the tick is Jan 1.

export function drawdown(twr: readonly (number | null)[]): (number | null)[];
// running peak of 100 + twr; (index/peak − 1)·100, null stays null
export function maxDrawdownTo(dd: readonly (number | null)[], index: number): number; // min ≤ 0

export type BucketKind = 'week' | 'month' | 'quarter' | 'year';
export function bucketKind(days: number): BucketKind; // ≤100 week, ≤450 month, ≤1300 quarter, else year
export interface FlowBucket { start: number; end: number; deposits: number; market: number }
export function flowBuckets(
  dates: readonly number[], values: readonly (number | null)[],
  netFlows: readonly number[], kind: BucketKind,
): FlowBucket[];
// A bucket groups consecutive points by week (7-day blocks from dates[0]), calendar month,
// quarter or year; its base is the last point before it (point 0 for the first bucket);
// deposits = netFlows[last] − netFlows[base]; market = values[last] − values[base] − deposits;
// a bucket whose first/last/base value is null is dropped.

export interface PositionLite {
  id: string; sector: string | null; currency: string;
  tags: readonly Pick<StockTag, 'id' | 'groupId'>[];
}
export type CompareGroup =
  | { kind: 'tags'; key: string; group: StockTagGroup; tags: StockTag[] }   // key 'group:<id>'
  | { kind: 'sector'; key: 'sector' }
  | { kind: 'currency'; key: 'currency' };
export function compareGroups(groups: readonly StockTagGroup[], tags: readonly StockTag[],
  positions: readonly PositionLite[]): CompareGroup[];
// tag groups that have at least one tag, by createdAt; then 'sector' when the held positions
// have at least two distinct sector values counting null as one; then 'currency' with at least
// two distinct currencies.
export function partKey(p: PositionLite, g: CompareGroup): string;
// tags: 'tag:<id>' of the position's tag in that group, else 'none:<groupId>';
// sector: 'sector:<name>' or 'sector:'; currency: 'currency:<code>'
export interface Part { key: string; positionIds: string[]; tag: StockTag | null;
  sector: string | null; currency: string | null; isNone: boolean }
export function groupParts(positions: readonly PositionLite[], g: CompareGroup,
  tags: readonly StockTag[]): Part[];   // order: tags in group tag order (by name), sectors and currencies alphabetical, none last
export interface SeriesSpecLite { key: string; investmentIds: string[]; currency?: string | null }
export function seriesSpecs(selected: readonly PositionLite[], tags: readonly StockTag[],
  g: CompareGroup | null, reportCurrency: string): SeriesSpecLite[];
// every tag carried by a selected position → 'tag:<id>'; the parts of g that are not tag parts
// ('none:…', 'sector:…', 'currency:…'); for currency parts whose code differs from
// reportCurrency also '<key>:native' with currency <code>. Unique keys, stable order.
export function positionCurrency(item: { priceCurrency: string | null;
  averagePriceCurrency: string }): string;
export function concentration(values: readonly number[]): { top5Share: number; effective: number };
// top5Share = sum of the five largest / total; effective = 1 / Σ w²; total 0 → {0, 0}
export function splitTop<T>(items: readonly T[], size: (t: T) => number, keep = 8, limit = 9):
  { shown: T[]; rest: T[] };     // all when items.length ≤ limit, else the `keep` largest |size|
export function seriesValueAt(dates: readonly number[], benchDates: readonly number[],
  bench: readonly number[]): (number | null)[];
// maps a daily benchmark onto the performance dates: the last benchmark point on or before each
// date, null before the first.
```

- [ ] **Step 1: Write the tests first** — at least: `presetRange` for each preset on 2026-10-11 (ytd from = 2026-01-01; 3m from = 2026-07-11; 1y = 2025-10-11; 3y = 2023-10-11; all = ALL_FROM); `parseCustomRange` (missing → 'missing'; from after to → 'order'; one-day span → 'order'; clamping to min/max); `dateAxisTicks` for a 92-day span (contains 2026-08-01 and 2026-08-15, kind 'day'), the ytd span (month starts Feb…Oct, none on Jan 1 because it equals t0), a 3-year span (quarter starts, yearStart on Jan 1), a 5-year span (years); `drawdown([0, 10, 5, 12, null])` → `[0, 0, -4.545…, 0, null]`; `maxDrawdownTo`; `bucketKind` thresholds; `flowBuckets` on a hand-made 10-point monthly grid with one deposit and a null value; `compareGroups` (group without tags skipped, sector hidden when all positions share one sector and none is null, sector shown with one sector + one ETF, currency hidden with one currency); `partKey` / `groupParts` (none part last); `seriesSpecs` (dedup, native series only for foreign currencies); `concentration([50, 30, 20])` → effective 1/(0.25+0.09+0.04) ≈ 2.63; `splitTop` (9 items kept, 10 items → 8 + rest 2 by absolute size); `seriesValueAt` carry-forward.
- [ ] **Step 2:** `npx vitest run src/utils/stock-performance.test.ts` — FAIL. **Step 3:** implement. **Step 4:** PASS, then `npm run lint`, `npm run typecheck`.

---

### Task 6: Chart — `PerformanceChart` and `DateAxis`, segmented divider

**Files:**
- Create: `src/components/stocks-analysis/PerformanceChart.tsx`, `src/components/charts/DateAxis.tsx`
- Modify: `src/components/ui/segmented.tsx` (optional `divider?: boolean` on `SegmentedOption`: a 1 × 14 px `bg-line-strong` rule rendered before that option, `aria-hidden`)

**Interfaces:**
- Consumes (Task 5): `dateAxisTicks`, `AxisTick`.
- Produces:

```ts
export interface PerformanceLine {
  id: string; name: string; color: string;           // CSS colour (var(--s1)…)
  variant: 'solid' | 'reference' | 'index';          // reference = dashed chart-line, index = dotted ink-4
  twr: (number | null)[]; drawdown: (number | null)[];
}
export interface PerformanceChartProps {
  dates: number[];                     // unix seconds, one per point
  lines: PerformanceLine[];            // parts first, then index, then the reference
  highlighted: string | null;
  onHighlight: (id: string | null) => void;
  onScrub: (index: number | null) => void;
  onRangeSelect: (from: number, to: number) => void;
  lastLabel: string;                   // "Dnes" or the end date
  formatPercent: (v: number, digits: number) => string;     // signed
  formatDay: (ts: number) => string;
  labels: { drawdown: string; tipDrawdown: (v: string) => string; dragTip: (days: number) => string };
  className?: string;
}
export function PerformanceChart(props: PerformanceChartProps): JSX.Element;
export function DateAxis(props: { t0: number; t1: number; gutter: number; lastLabel: string;
  className?: string }): JSX.Element;   // labels through useFormat().day: 'day' → "15. 8.",
  // 'month' → "srp" (+ " 26" on January), 'quarter' → "říj 25", 'year' → "2025"; yearStart bold ink-3
```

Behaviour (follow `MoonyLinesChart` for tokens, tooltip (`ChartTip`), tick formatting via `niceTicks`/`valueDomain` from `@/utils/chart-scale`):
- Two Recharts `ComposedChart`s with the same `syncId="stock-performance"` and the same right gutter (128 px): the TWR panel (280 px) and the drawdown panel (84 px, label "PROPAD OD MAXIMA" in eyebrow style above it, zero at the top, the reference line also as an area in `chart-area`). X axis `type="number"`, `dataKey="t"`, domain `[dates[0], dates.at(-1)]`, hidden; the `DateAxis` under both panels renders labels at real positions with a 5 px tick mark per label and the `lastLabel` at the right end.
- Lines: solid 2.25 px (first) / 2 px, reference `strokeDasharray="4 4"` butt caps, index `strokeDasharray="0.1 4"` round caps 2.5 px; `strokeOpacity` 0.25 for lines other than `highlighted`; end rings and end labels ("Růst + 11,2 %", nudged 14 px apart) in the right gutter like `MoonyLinesChart` `labels="end"`; hovering an end label calls `onHighlight`.
- Hover: crosshair across both panels, tooltip with the date and the lines sorted by value plus `tipDrawdown` of the reference; `onScrub(index)` while hovering, `onScrub(null)` on leave.
- Drag: `onMouseDown` stores `activeLabel`, `onMouseMove` draws a `ReferenceArea` (`fill` ink 6 %) and a tooltip `dragTip(days)`, `onMouseUp` calls `onRangeSelect(min, max)` when the span is at least 2 days; leaving cancels.
- [ ] **Step 1:** implement `divider` in `segmented.tsx` (existing callers unaffected).
- [ ] **Step 2:** implement `DateAxis` and `PerformanceChart`.
- [ ] **Step 3:** `npm run lint`, `npm run typecheck` (errors in files other agents are editing can be ignored; report them).

---

### Task 7: Page — scope card, performance card and board

**Files:**
- Modify: `src/pages/StocksAnalysis.tsx` (orchestration only)
- Create: `src/components/stocks-analysis/ScopeCard.tsx`, `PerformanceCard.tsx`, `PerformanceBoard.tsx`, `src/hooks/use-stock-performance.ts`
- Modify: `src/i18n/locales/{en,cs}/reports.json` (Copy §B: `scope`, `performance`, `board`, `groups`, `periods`, `pp`)

**Interfaces — Consumes:** Task 3 `investmentsApi.getStockPerformance`, Task 4 `investmentsApi.getStockBenchmark`, Task 5 helpers, Task 6 `PerformanceChart`, `Segmented` `divider`. Existing: `stockTagsApi.getAll/getAllGroups/getAnalysis`, `chosenTagIds`/`selectPositions`/`FILTER_ALL` from `@/utils/stock-analysis`, `useCurrency`, `useFormat`, `readSetting`/`writeSetting` from `@/utils/device-storage`, `PageHead`, `Card`, `Chip`, `Segmented`, `DropdownMenu`, `Input`.
**Produces:** `useStockPerformance(...)` returning `{ data, isLoading, isPlaceholderData }`; the page passes `performance`, `parts`, `selection` to the Task 8 cards.

Page behaviour (spec "Page structure" 1–3):
- State: chip filters (per group key), `period: PeriodPreset` and `custom: DayRange | null` and `group` key, all persisted per device with `writeSetting` under `stocks-analysis.period`, `stocks-analysis.custom` (`from|to`), `stocks-analysis.group`; an index id `null | 'sp500' | 'msci'` (not persisted). Benchmarks: `sp500` → ticker `CSPX.L`, `msci` → `IWDA.L`, names "S&P 500" / "MSCI World".
- Filter groups: the tag groups by `createdAt` plus a "Štítky" row for tags without a group (existing logic). Selection as today (`selectPositions`, AND across groups). From the fourth row on, rows hide behind a "Další filtry" toggle; a hidden row with an active filter shows "Region: USA ×" chips next to it.
- Request: `{ fromTs, toTs, reportCurrency: currencyCode, series: seriesSpecs(selected, tags, group, currencyCode) }` with `placeholderData: keepPreviousData`; query key `['stock-performance', request, fxRevision]`. Benchmark query key `['stock-benchmark', ticker, from, to, currencyCode]`, enabled only with an index; `seriesValueAt` maps it onto the dates.
- Values "as of the end": when `to` is today use the analysis `value` of each position, else `positions[].endValue` from the performance response.
- Parts: `groupParts(selected, group, tags)`, ordered by end value, the first four drawn (`s1`…`s4`), the rest listed only; the reference line `Celé portfolio` and the index line.
- `PerformanceCard`: figures (`hv` scrub state from `onScrub`), toolbar (`Rozdělit podle` segmented with `divider` before the first automatic group, or a `Select` when there are more than four options; index `DropdownMenu`; period segmented), split caption (Copy `performance.caption*`), custom range inputs (`Input type="date"`, 140 × 35 px, `→` between, hint or error under them, `parseCustomRange` with min = first date of the "Vše" response or `ALL_FROM`, max = today), the chart, the board.
- `PerformanceBoard`: columns and rows per spec; contribution from `series[key].contribution[index]`, max drawdown with `maxDrawdownTo`, FX sub-line for currency parts (`v USD + 12,1 % · kurz − 3,7 p. b.` from the `:native` series), hover → `onHighlight`, click on a tag part → set that chip filter; caption by group kind.
- Remove the stats row, the allocation ring card and the old TWR card from the page (the `AllocationRing` and `MoonyLinesChart` components stay; other pages use them).
- Loading: skeleton blocks shaped like the scope card and the hero; empty portfolio keeps the existing `EmptyState`.
- [ ] **Step 1:** add the Copy §B keys to `en`/`cs`. **Step 2:** hook. **Step 3:** `ScopeCard`. **Step 4:** `PerformanceBoard`. **Step 5:** `PerformanceCard`. **Step 6:** rewire the page. **Step 7:** lint, typecheck, format; `npm test` except locale parity.

---

### Task 8: Contribution, deposits and the two tables

**Files:** Create `src/components/stocks-analysis/ContributionCard.tsx`, `FlowsCard.tsx`, `TagsTable.tsx`, `HoldingsTable.tsx`; Modify `src/pages/StocksAnalysis.tsx` (render them under the hero: the two cards in `grid grid-cols-2 gap-[18px]`, equal height), `src/i18n/locales/{en,cs}/reports.json` (Copy §C: `contribution`, `flows`, `tagsTable`, `holdings`).

**Interfaces — Consumes:** the page's `performance: StockPerformance`, `parts`, the chosen group, `selected`, `filterActive`, `endValueOf(positionId)`, Task 5 `splitTop`, `flowBuckets`, `bucketKind`, `concentration`. `MoonyBarChart` for the bars (a = market `s1`, b = deposits `s3`; extend it to draw negative values below a zero baseline if it does not yet).

- `ContributionCard`: segmented `Pozice` / `Podle skupiny` (title attribute Copy `contribution.byGroupTitle`); rows with name (logo + company for positions; name + "N pozic" for parts), a diverging bar (zero line; positive `s2`, negative `s3`), value in p. b. coloured `gain`/`loss`; `splitTop` with "Ostatní · N pozic"; footer row "Celé portfolio" or "Výběr celkem"; subtitle per Copy.
- `FlowsCard`: four-cell bridge (Na začátku = `values[0]`, Čisté vklady, Zhodnocení trhu, Dnes / K datu), bars per `flowBuckets(..., bucketKind(days))` with labels (week "7. 7.", month "srp" + year on January and on the first bucket, quarter "3. Q 25", year "2025"), legend; the chart fills the card height.
- `TagsTable`: every tag of the tag groups and the ungrouped tags (`bez skupiny`), Pozice, Hodnota (as of the end), Podíl (of the whole portfolio), Výnos za období (`series['tag:<id>'].twr.at(-1)`), Nerealizovaný zisk (`ChangeBadge`; header "Nerealiz. zisk dnes" when the period ends before today), Dividenda ročně; with a filter only tags that have a selected position.
- `HoldingsTable`: today's table plus Výnos za období (`positions[].twr`), the sub-line `TICKER · CUR · Sector` or `· ETF`, the concentration span in the subtitle (title attribute with the explanation), value header "K D. M. RRRR" before today.
- [ ] Steps: copy keys → each component → page wiring → lint/typecheck/format/tests.

---

### Task 9: Tag dialogs — one choice per group

**Files:** `src/components/stocks-analysis/AssignTagsDialog.tsx`, `src/components/stocks-analysis/ManageTagsDialog.tsx`, `src/utils/stock-analysis.ts` (+ test) for `nextTagIds`, `src/i18n/locales/{en,cs}/reports.json` (Copy §D: `assign`, `manage.moveConflict`).

**Interfaces — Produces:**

```ts
/** The tags a position gets: per group 'keep' leaves its tag, 'none' removes it, a tag id
 * replaces it; free tags: single position → exactly `free`, several → added. */
export function nextTagIds(current: readonly StockTag[], picks: Record<string, string>,
  free: readonly string[], single: boolean, tags: readonly StockTag[]): string[];
/** Positions that would carry two tags of `groupId` if tag `tagId` moved into it. */
export function moveConflicts(positions: readonly { companyName: string; tags: readonly StockTag[] }[],
  tagId: string, groupId: string): string[];
```

- Assign dialog per prototype: positions list (chosen first), per tag group a row of chips (single position: `Bez štítku` + tags, preselected; several: `Beze změny` default + tags + `Bez štítku`), "Bez skupiny" row (multi-select), hint per Copy. Save calls `setForInvestment(id, nextTagIds(...))` per position; on success invalidate `['stocks-analysis']`, `['tag-metrics']`, `['stock-performance']`; errors through `translateApiError`.
- Manage dialog: before `moveMutation`, compute `moveConflicts`; when non-empty show the inline error Copy `manage.moveConflict` (`{{group}}`, `{{positions}}` joined with ", ") and do not call the backend; backend errors still go through `translateApiError`. Invalidate `['stock-performance']` instead of `['stock-twr']`.
- [ ] Steps: tests for `nextTagIds` and `moveConflicts` first (keep/none/replace, free tags single vs several, conflict list) → implement → dialogs → copy → gates.

---

### Task 10: Retire the old TWR path

**Files:** `src-tauri/src/services/investments.rs` (delete `twr_series`, `compute_twr_for_tickers`, `TwrScope`, `twr_scope_tickers` and their tests; port the two FX tests `twr_in_another_currency_includes_the_exchange_rate_move` and `twr_flows_are_converted_at_their_own_day` to `stock_performance` portfolio assertions if not already covered), `src-tauri/src/models/investments.rs` (delete `TwrDataPoint`, `TwrSeries`, `StockTwr`), `src-tauri/src/commands/investments.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts` (`getStockTwr`), `src/utils/twr.ts` (+ test) if unused, query key `stock-twr` → `stock-performance` in `src/hooks/use-history-recalculation.ts`, `src/hooks/use-investment-mutations.ts`, `src/hooks/use-stock-import-mutations.ts`, `src/lib/price-refresh.ts`; `docs/DEPRECATED.md` (new section "Removed with the stocks analysis redesign": `get_stock_twr`, `investmentsApi.getStockTwr`, `TwrSeries`/`StockTwr`/`TwrDataPoint`, `src/utils/twr.ts`, the ring and stats on the analysis page → use `get_stock_performance`).
- [ ] Steps: `grep -rn "twr_series\|getStockTwr\|stock-twr\|TwrSeries\|StockTwr\|twrPercent" src src-tauri/src shared` must end empty except the new code; gates.

---

### Task 11: Eight more locales and obsolete keys

**Files:** `src/i18n/locales/{de,es,it,nb,pl,sv,ja,zh}/{reports,common}.json`, and all ten `reports.json`.
- Translate every new key (Copy §A–D) following `docs/standards/i18n.md` (plural forms per language table, same `{{variables}}`, glossary `i18n-glossary.md`).
- Delete keys no code references any more (the old `stocksAnalysis.stats`, `allocation`, `twr`, `selection`, `filters` subtrees and the legacy flat keys of `stocksAnalysis` such as `twrTitle`, `manageTags`, …) from all ten locales: verify each with `grep -rn "<key>" src` before deleting.
- [ ] `npx vitest run src/i18n` PASS, then `npm test` fully green.

---

### Task 12: Docs

**Files:** `docs/design-system/README.md` (§7 "Analysis" row rewritten to the new page; §6 Segmented row mentions the optional divider), `docs/architecture/decisions/0013-tag-groups-partition-the-portfolio.md` (+ index row in `docs/architecture/decisions/README.md`): context (three different values for one tag), decision (one tag per group, ungrouped tags free, migration 015, enforcement points), consequences; `CHANGELOG.md` under the unreleased section: one user-facing bullet list in the file's existing style.

---

### Task 13: Verification, review, merge (orchestrator)

- [ ] Full gates: `npm run lint && npm run typecheck && npm run format:check && npm test`; `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; `cargo test generate_bindings -- --ignored` leaves `shared/generated-types.ts` unchanged.
- [ ] Demo run: copy `../Moony-tauri/demo-home` into the worktree, add a temporary `moony-demo-1422` launch config (port 1422, `HOME` = the copied demo-home, `backgroundThrottling` disabled), confirm the lock screen greets Alex, unlock (`demo1234`), open `/reports/stocks-analysis`: every preset, custom range by inputs and by drag, Rozdělit podle each option, index on (online), filters, assign dialog, move conflict, tables; console clean; `[MIGRATION] 015` in the log.
- [ ] Code review by a fresh reviewer (sonnet) over `git diff main...HEAD`; fix findings.
- [ ] Merge locally (temporary main worktree, `git merge --no-ff`), verify with `git merge-base --is-ancestor`, remove the temporary launch config and worktrees. No push.

---

## Copy appendix

Plural keys follow the language rules (`cs`: `_one`, `_few`, `_other`; `en`: `_one`, `_other`).

### §A `common.json` → `validation`

| Key | en | cs |
|---|---|---|
| `tagNameRequired` | Enter a tag name. | Zadejte název štítku. |
| `tagNameTooLong` | A name can have at most 60 characters. | Název může mít nejvýš 60 znaků. |
| `tagNotFound` | This tag no longer exists. | Štítek už neexistuje. |
| `oneTagPerGroup` | A position can have only one tag from each group. | Pozice může mít z každé skupiny jen jeden štítek. |
| `tagGroupConflict` | Some positions already have another tag in that group. | Některé pozice už mají v té skupině jiný štítek. |
| `periodInvalid` | The start of the period must be before its end. | Začátek období musí být před jeho koncem. |
| `tooManySeries` | Too many lines requested at once. | Najednou je požadováno příliš mnoho čar. |
| `seriesKeyInvalid` | Invalid chart line request. | Neplatný požadavek na čáru grafu. |
| `benchmarkTickerInvalid` | Invalid index ticker. | Neplatný ticker indexu. |

### §B `reports.json` → `stocksAnalysis` (Task 7)

```json
{
  "pp": "p. b.",
  "groups": { "sector": "Sektor", "currency": "Měna", "autoTitle": "Automaticky z údajů o akcii" },
  "periods": { "3m": "3 měsíce", "ytd": "Letos", "1y": "Rok", "3y": "3 roky", "all": "Vše", "custom": "Vlastní", "label": "Období" },
  "scope": {
    "title": "Které pozice analyzovat",
    "all_one": "{{count}} pozice", "all_few": "Všechny {{count}} pozice", "all_other": "Všech {{count}} pozic",
    "selected": "{{shown}} z {{total}} pozic",
    "clear": "Zrušit výběr",
    "hint": "Klepněte na štítek a v analýze zůstanou jen pozice, které ho mají. Štítky ze dvou skupin musí pozice mít oba.",
    "hintActive": "V analýze jsou jen pozice se štítky {{tags}}. Horní čísla karty výkonnosti a čárkovaná čára zůstávají za celé portfolio.",
    "tagsJoin": " a zároveň ",
    "all": "Vše", "ungrouped": "Štítky", "ungroupedTitle": "Štítky bez skupiny se mohou překrývat",
    "more": "Další filtry", "fewer": "Méně filtrů", "removeFilter": "Zrušit filtr {{name}}",
    "untagged_one": "{{count}} pozice bez štítku", "untagged_few": "{{count}} pozice bez štítku", "untagged_other": "{{count}} pozic bez štítku",
    "assignUntagged": "Přiřadit štítky →"
  },
  "performance": {
    "twrLabel": "Časově vážený výnos · celé portfolio",
    "bestWorst": "nejlepší a nejhorší", "bestWorstSelection": "nejlepší a nejhorší ve výběru",
    "onlyPart": "jediná část výběru", "noPart": "Ve výběru nic ze skupiny {{group}}",
    "aheadOfIndex": "před indexem {{index}} {{value}}", "behindIndex": "za indexem {{index}} {{value}}",
    "fromToday": "od {{from}} do dnes", "fromTo": "od {{from}} do {{to}}", "fromAt": "od {{from}} k {{at}}",
    "positions_one": "{{count}} pozice", "positions_few": "{{count}} pozice", "positions_other": "{{count}} pozic",
    "mwrLabel": "Peněžně vážený výnos", "mwrNote": "zohledňuje, kdy jste vkládali",
    "mwrSame": "stejně jako časově vážený", "mwrGap": "{{gap}} proti časově váženému",
    "changeLabel": "Změna hodnoty", "netDeposits": "čisté vklady {{amount}}", "market": "zhodnocení trhu {{amount}}",
    "splitBy": "Rozdělit podle", "splitByTitle": "Na jaké části se vybrané pozice rozdělí: každá část je jedna čára v grafu a jeden řádek v tabulce",
    "from": "Od", "to": "Do", "dragHint": "Úsek lze vybrat i tažením myší v grafu.",
    "rangeMissing": "Vyplňte obě data.", "rangeOrder": "Začátek musí být aspoň dva dny před koncem.",
    "indexButton": "Srovnat s indexem", "indexActive": "Index: {{name}}", "indexNone": "Bez srovnání",
    "indexVia": "přes {{ticker}}", "indexFailed": "Index se nepodařilo načíst. Zkontrolujte připojení k internetu.",
    "captionTags": "Každá čára sčítá pozice s jedním štítkem skupiny {{group}}: {{parts}}.",
    "captionTagsSelected": "Každá čára sčítá vybrané pozice s jedním štítkem skupiny {{group}}: {{parts}}.",
    "captionSector": "Každá čára sčítá pozice jednoho sektoru: {{parts}}. Sektor se bere z údajů o akcii.",
    "captionSectorSelected": "Každá čára sčítá vybrané pozice jednoho sektoru: {{parts}}. Sektor se bere z údajů o akcii.",
    "captionCurrency": "Každá čára sčítá pozice v jedné měně: {{parts}}. Výnos je v {{currency}}.",
    "captionCurrencySelected": "Každá čára sčítá vybrané pozice v jedné měně: {{parts}}. Výnos je v {{currency}}.",
    "captionPortfolio": "Čárkovaně celé portfolio.", "captionMore": "Další jsou jen v tabulce pod grafem.",
    "captionEmpty": "Ve výběru není žádná pozice se štítkem skupiny {{group}}. Čárkovaně celé portfolio.",
    "captionNoGroups": "Založte skupinu štítků a uvidíte, jak si vedou její části. Zatím jen celé portfolio.",
    "drawdown": "Propad od maxima", "tipDrawdown": "Propad portfolia {{value}}",
    "dragTip": "{{days}} dní · pusťte myš a období se nastaví",
    "portfolio": "Celé portfolio", "today": "Dnes"
  },
  "board": {
    "return": "Výnos za období", "vsPortfolio": "Proti portfoliu", "contribution": "Příspěvek", "maxDrawdown": "Max. propad",
    "share": "Podíl na hodnotě", "shareSelection": "Podíl na hodnotě výběru", "value": "Hodnota", "valueAt": "K {{date}}",
    "reference": "referenční čára", "index": "tečkovaně · srovnání", "noLine": "bez čáry",
    "positions_one": "{{count}} pozice", "positions_few": "{{count}} pozice", "positions_other": "{{count}} pozic",
    "fx": "v {{currency}} {{native}} · kurz {{effect}}", "noFx": "bez vlivu kurzu", "filterBy": "Filtrovat: {{name}}",
    "noneTag": "Bez štítku", "noneSector": "Bez sektoru",
    "captionTags": "Každá pozice má ve skupině nejvýš jeden štítek, takže podíly dají 100 % a příspěvky se sečtou na výnos portfolia.",
    "captionSector": "Sektor podle údajů o akcii · ETF sektor nemají a patří do řádku Bez sektoru.",
    "captionCurrency": "Výnos v {{currency}} · pod měnou je výnos v měně pozice a vliv kurzu.",
    "captionMore": "Graf kreslí nejvýš čtyři čáry, další jsou jen v tabulce.",
    "captionSelection": "Příspěvek je k výnosu celého portfolia."
  }
}
```

English (`en/reports.json`), same keys:

```json
{
  "pp": "pp",
  "groups": { "sector": "Sector", "currency": "Currency", "autoTitle": "Automatically from the stock's data" },
  "periods": { "3m": "3 months", "ytd": "This year", "1y": "Year", "3y": "3 years", "all": "All", "custom": "Custom", "label": "Period" },
  "scope": {
    "title": "Which positions to analyse",
    "all_one": "{{count}} position", "all_other": "All {{count}} positions",
    "selected": "{{shown}} of {{total}} positions",
    "clear": "Clear selection",
    "hint": "Click a tag to keep only the positions that carry it. Tags from two groups must both be on a position.",
    "hintActive": "Only positions tagged {{tags}} are analysed. The figures at the top of the performance card and the dashed line stay whole-portfolio.",
    "tagsJoin": " and ",
    "all": "All", "ungrouped": "Tags", "ungroupedTitle": "Tags without a group can overlap",
    "more": "More filters", "fewer": "Fewer filters", "removeFilter": "Remove filter {{name}}",
    "untagged_one": "{{count}} position without a tag", "untagged_other": "{{count}} positions without a tag",
    "assignUntagged": "Assign tags →"
  },
  "performance": {
    "twrLabel": "Time-weighted return · whole portfolio",
    "bestWorst": "best and worst", "bestWorstSelection": "best and worst in the selection",
    "onlyPart": "the only part of the selection", "noPart": "Nothing in the selection belongs to {{group}}",
    "aheadOfIndex": "ahead of {{index}} {{value}}", "behindIndex": "behind {{index}} {{value}}",
    "fromToday": "from {{from}} to today", "fromTo": "from {{from}} to {{to}}", "fromAt": "from {{from}} to {{at}}",
    "positions_one": "{{count}} position", "positions_other": "{{count}} positions",
    "mwrLabel": "Money-weighted return", "mwrNote": "accounts for when you invested",
    "mwrSame": "same as time-weighted", "mwrGap": "{{gap}} against time-weighted",
    "changeLabel": "Change in value", "netDeposits": "net deposits {{amount}}", "market": "market {{amount}}",
    "splitBy": "Split by", "splitByTitle": "How the selected positions are split: each part is one line in the chart and one row in the table",
    "from": "From", "to": "To", "dragHint": "You can also drag across the chart to pick a range.",
    "rangeMissing": "Fill in both dates.", "rangeOrder": "The start must be at least two days before the end.",
    "indexButton": "Compare with an index", "indexActive": "Index: {{name}}", "indexNone": "No comparison",
    "indexVia": "via {{ticker}}", "indexFailed": "The index could not be loaded. Check your internet connection.",
    "captionTags": "Each line adds up the positions with one tag of the group {{group}}: {{parts}}.",
    "captionTagsSelected": "Each line adds up the selected positions with one tag of the group {{group}}: {{parts}}.",
    "captionSector": "Each line adds up the positions of one sector: {{parts}}. The sector comes from the stock's data.",
    "captionSectorSelected": "Each line adds up the selected positions of one sector: {{parts}}. The sector comes from the stock's data.",
    "captionCurrency": "Each line adds up the positions in one currency: {{parts}}. Returns are in {{currency}}.",
    "captionCurrencySelected": "Each line adds up the selected positions in one currency: {{parts}}. Returns are in {{currency}}.",
    "captionPortfolio": "Dashed: the whole portfolio.", "captionMore": "The others are only in the table below the chart.",
    "captionEmpty": "No selected position has a tag of the group {{group}}. Dashed: the whole portfolio.",
    "captionNoGroups": "Create a tag group to see how its parts perform. For now, the whole portfolio only.",
    "drawdown": "Drawdown from peak", "tipDrawdown": "Portfolio drawdown {{value}}",
    "dragTip": "{{days}} days · release to set the period",
    "portfolio": "Whole portfolio", "today": "Today"
  },
  "board": {
    "return": "Period return", "vsPortfolio": "Vs portfolio", "contribution": "Contribution", "maxDrawdown": "Max drawdown",
    "share": "Share of value", "shareSelection": "Share of selection value", "value": "Value", "valueAt": "On {{date}}",
    "reference": "reference line", "index": "dotted · comparison", "noLine": "no line",
    "positions_one": "{{count}} position", "positions_other": "{{count}} positions",
    "fx": "in {{currency}} {{native}} · rate {{effect}}", "noFx": "no exchange rate effect", "filterBy": "Filter: {{name}}",
    "noneTag": "No tag", "noneSector": "No sector",
    "captionTags": "Each position has at most one tag in a group, so shares add up to 100 % and contributions add up to the portfolio return.",
    "captionSector": "Sector from the stock's data · ETFs have no sector and sit in the No sector row.",
    "captionCurrency": "Returns in {{currency}} · under each currency, the return in the position's own currency and the exchange rate effect.",
    "captionMore": "The chart draws at most four lines; the others are only in the table.",
    "captionSelection": "Contributions are to the whole portfolio's return."
  }
}
```

### §C `reports.json` → `stocksAnalysis` (Task 8)

| Key | en | cs |
|---|---|---|
| `contribution.title` | Contribution to return | Příspěvek k výnosu |
| `contribution.byPosition` | Positions | Pozice |
| `contribution.byGroup` | By group | Podle skupiny |
| `contribution.byGroupTitle` | The group is chosen with Split by in the performance card | Skupinu určuje Rozdělit podle v kartě výkonnosti |
| `contribution.subPositions` | Percentage points of the {{value}} return each position brought | Kolik procentních bodů z výnosu {{value}} přinesla každá pozice |
| `contribution.subTags` | Percentage points of the {{value}} return each tag of the group {{group}} brought | Kolik procentních bodů z výnosu {{value}} přinesl každý štítek skupiny {{group}} |
| `contribution.subSectors` | Percentage points of the {{value}} return each sector brought | Kolik procentních bodů z výnosu {{value}} přinesl každý sektor |
| `contribution.subCurrencies` | Percentage points of the {{value}} return each currency brought | Kolik procentních bodů z výnosu {{value}} přinesla každá měna |
| `contribution.subSelection` | The selection brought {{part}} of the portfolio's {{value}} return | Výběr přinesl {{part}} z {{value}} výnosu portfolia |
| `contribution.subSelectionGroup` | The selection brought {{part}} of the portfolio's {{value}} return · split by {{group}} | Výběr přinesl {{part}} z {{value}} výnosu portfolia · rozděleno podle skupiny {{group}} |
| `contribution.other` | Other | Ostatní |
| `contribution.portfolio` | Whole portfolio | Celé portfolio |
| `contribution.selection` | Selection total | Výběr celkem |
| `contribution.periodReturn` | period return {{value}} | výnos za období {{value}} |
| `flows.title` | Deposits and market | Vklady a zhodnocení |
| `flows.sub` | Whole portfolio · {{unit}} from {{from}} | Celé portfolio · {{unit}} od {{from}} |
| `flows.subTo` | Whole portfolio · {{unit}} from {{from}} to {{to}} | Celé portfolio · {{unit}} od {{from}} do {{to}} |
| `flows.units.week` / `month` / `quarter` / `year` | by week / by month / by quarter / by year | po týdnech / po měsících / po čtvrtletích / po letech |
| `flows.start` | At the start | Na začátku |
| `flows.deposits` | Net deposits | Čisté vklady |
| `flows.market` | Market | Zhodnocení trhu |
| `flows.end` | Today | Dnes |
| `flows.endAt` | On {{date}} | K {{date}} |
| `flows.legendDeposits` | Net deposits · buys minus sells | Čisté vklady · nákupy minus prodeje |
| `flows.change` | Change in value | Změna hodnoty |
| `tagsTable.sub` | Shares within a group add up to 100 % · tags without a group can overlap | Ve skupině dají podíly 100 % · štítky bez skupiny se mohou překrývat |
| `tagsTable.periodReturn` | Period return | Výnos za období |
| `tagsTable.gain` | Unrealized gain | Nerealizovaný zisk |
| `tagsTable.gainToday` | Unrealized gain today | Nerealiz. zisk dnes |
| `tagsTable.noGroup` | no group | bez skupiny |
| `tagsTable.valueAt` | On {{date}} | K {{date}} |
| `holdings.periodReturn` | Period return | Výnos za období |
| `holdings.gain` | Unrealized gain | Nerealizovaný zisk |
| `holdings.etf` | ETF | ETF |
| `holdings.concentration` | top 5 hold {{share}} · effectively {{count}} positions | 5 největších tvoří {{share}} · efektivně {{count}} pozice |
| `holdings.concentrationTitle` | Concentration of the whole portfolio. Effective number of positions: how many equally sized positions would be as concentrated. | Koncentrace celého portfolia. Efektivní počet pozic: kolik stejně velkých pozic by mělo stejnou koncentraci. |

### §D `reports.json` → `stocksAnalysis` (Task 9)

| Key | en | cs |
|---|---|---|
| `assign.oneTag` | one tag | jeden štítek |
| `assign.manyTags` | several allowed | lze vybrat víc |
| `assign.noGroup` | No group | Bez skupiny |
| `assign.keep` | No change | Beze změny |
| `assign.none` | No tag | Bez štítku |
| `assign.hintSingle` | A position has at most one tag in each group. The chosen tag replaces the current one. | Pozice má v každé skupině nejvýš jeden štítek. Vybraný štítek nahradí ten dosavadní. |
| `assign.hintMany` | A position has at most one tag in each group. The chosen tag replaces the current one; groups left on No change stay as they are. Tags without a group are added. | Pozice má v každé skupině nejvýš jeden štítek. Vybraný štítek nahradí ten dosavadní, skupiny na Beze změny zůstanou, jak jsou. Štítky bez skupiny se přidají. |
| `manage.moveConflict` | The tag cannot move to {{group}}: {{positions}} already have another tag there. | Štítek nejde přesunout do skupiny {{group}}: {{positions}} v ní už mají jiný štítek. |
