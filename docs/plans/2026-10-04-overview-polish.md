# Overview polish and fixes — implementation plan

> **For agentic workers:** this plan is executed by parallel subagents, one work package each, in
> separate git worktrees; the coordinator merges and verifies. Steps use checkbox (`- [ ]`) syntax.
> Read the spec first: `docs/specs/2026-10-04-overview-polish-and-fixes-design.md`.

**Goal:** fix the chart, history and table defects the owner reported on 0.9.0 and polish the
dashboard, list pages, loans, insurance, real estate, stocks analysis and stock detail (spec §3).

**Architecture:** frontend changes on the existing design-system primitives; shared foundations
(`chartPeriodStart`/`CHART_PERIODS` in `src/utils/period.ts`, `TrendCard` + `TREND_CHART_HEIGHT` in
`src/components/charts/TrendCard.tsx`) are already on the base branch; three packages touch Rust
(one migration, one service move, one new command).

**Tech stack:** React 19 + TypeScript, TanStack Query, Recharts 3, Tailwind tokens, react-i18next;
Tauri 2 + Rust (rusqlite/SQLCipher, specta bindings).

## Global constraints (every package)

- `AGENTS.md` ten rules apply. In short: `invoke()` only in `src/lib/tauri-api.ts`; Rust types get
  per-field `#[serde(rename = "camelCase")]`, registration in `src-tauri/src/bindings.rs`, a mirror in
  `shared/schema.ts`, then `cd src-tauri && cargo test generate_bindings -- --ignored`; thin commands,
  logic in `src-tauri/src/services/` taking `&Connection`, `AppError`, no `.unwrap()` outside tests;
  migrations append-only; mutation contract (domain keys + `portfolio-metrics` + `cashflow-report`,
  then `portfolioApi.recordSnapshot()` + `portfolio-history`); money TEXT, days UTC-midnight seconds.
- Every user-facing string in both `src/i18n/locales/cs/` and `en/` (Czech plurals `_one/_few/_many/_other`
  where a count appears). Sentence case, no exclamation marks, true minus "−" in signed values.
- Design-system tokens only (`docs/design-system/README.md`); no raw colours; reuse `ui/` primitives.
- Tests: pure functions → co-located `*.test.ts` (Vitest, node environment, no component tests);
  Rust service logic → `#[cfg(test)]` with `Connection::open_in_memory()` and a minimal schema.
  Bug fixes in tested code get a regression test that fails before the fix.
- No new dependencies. Do not edit `docs/design-system/`, `CHANGELOG.md`, `docs/specs/`, `docs/plans/`
  (the coordinator does). Do not use the browser-pane tools. Never push, merge or switch branches of
  the main checkout; work only inside your worktree.
- Gates before each commit: `npm run lint && npm run typecheck && npm test`; Rust packages also
  `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Commits: Conventional Commits, small, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Worktree setup (done by the coordinator)

Each package has a worktree at `$WT/<package>` on branch `wp/<package>` created from the base
commit, with `node_modules` and `dist` symlinked from the main checkout. Rust packages export
`CARGO_TARGET_DIR=$AGENTS_TARGET` (a shared build directory) before any cargo command.

## File ownership

A package edits only the files it lists. Shared files allowed for several packages are marked
*(shared — keep edits local to your section)*.

| Package | Owns |
|---|---|
| A1 chart marks | `src/components/charts/MoonyLineChart.tsx`, `src/components/charts/EventMarkers.tsx` |
| A2 trend cards | `src/components/common/PortfolioTrendCard.tsx`, `src/utils/trade-events.ts(+test)`, `src/pages/Stocks.tsx`, `src/pages/Crypto.tsx`, `src/pages/BankAccounts.tsx`, `src/components/stocks/AddInvestmentModal.tsx`, `src/components/bonds/BondsLadderCard.tsx`, i18n `stocks` *(shared, `chart.*` and `importCSV` only)*, `crypto`, `bank_accounts`, `bonds` |
| A3 dashboard + periods | `src/pages/Dashboard.tsx`, `src/components/stocks/PositionHero.tsx`, `src/pages/BankAccountDetail.tsx`, i18n `dashboard` |
| A4 loans | `src/components/loans/DebtTrajectoryCard.tsx`, `src/utils/loan-trajectory.ts(+test)`, `src/pages/LoanDetail.tsx`, i18n `loans` |
| A5 insurance | `src/components/insurance/InsuranceTable.tsx`, `src/components/insurance/PaymentCalendarCard.tsx`, `src/utils/insurance.ts(+test)`, i18n `insurance` |
| A6 real estate trace | migration + `schema.snapshot` + `docs/architecture/database.md`, `src-tauri/src/models/real_estate.rs`, `src-tauri/src/services/{real_estate,valuations,other_assets}.rs`, `src-tauri/src/commands/real_estate.rs`, `src-tauri/src/services/mcp/real_estate.rs`, `shared/schema.ts` *(shared)*, `shared/generated-types.ts` *(shared)*, `src/utils/real-estate-payload.ts(+test)`, `src/utils/valuation-trace.ts(+test)`, `src/components/real-estate/AddRealEstateModal.tsx`, `src/hooks/use-real-estate-mutations.ts`, `src/hooks/use-valuation-mutations.ts`, `src/pages/RealEstateDetail.tsx`, i18n `realEstate` *(shared, not `table.*`)*, `common` *(shared, `periods.3Y` and `validation.purchaseDateInFuture` only)* |
| A7 real estate table | `src/pages/RealEstate.tsx` (table card only), i18n `realEstate` *(shared, `table.*` only)* |
| A8 stocks analysis | `src-tauri/src/commands/investments.rs` (`get_stock_twr` only), `src-tauri/src/services/investments.rs` (TWR section), `src-tauri/src/models/investments.rs` (`TwrSeries` only), `shared/generated-types.ts` *(shared)*, `src/lib/tauri-api.ts` *(shared, `getStockTwr` only)*, `src/pages/StocksAnalysis.tsx`, `src/components/charts/MoonyLinesChart.tsx`, `src/components/dashboard/AllocationRing.tsx`, `src/hooks/use-media-query.ts` (new), `src/utils/twr.ts(+test)`, i18n `reports` |
| A9 company info + range | new `src-tauri/src/models/company_info.rs`, `src-tauri/src/services/company_info.rs`, `src-tauri/src/commands/company_info.rs`, `src-tauri/src/{models,commands,services}/mod.rs` *(shared, one line each)*, `src-tauri/src/lib.rs` *(shared, command registration)*, `src-tauri/src/bindings.rs` *(shared)*, `src-tauri/src/services/price_api.rs` (metadata logging only), `shared/schema.ts` *(shared)*, `shared/generated-types.ts` *(shared)*, `src/lib/tauri-api.ts` *(shared, company info + removal of `refreshMetadata`)*, `src/pages/StockDetail.tsx`, `src/components/stock-monitor/RangeBar.tsx`, `src/pages/StockMonitorDetail.tsx`, i18n `stocks` *(shared, `detail.*` only)*, `stockMonitor` |

---

### A1 — Chart marks that do not hijack the tooltip (spec §3.1, F1)

Root cause: `MoonyLineChart` renders event marks as `<Scatter data={markers}>`. Recharts 3
(`combineDisplayedData` in `node_modules/recharts/es6/state/selectors/axisSelectors.js`) uses the data
of any graphical item that has its own `data` as the axis' displayed data, so the tooltip resolves
the active index against the marks and returns `rows[index]` — the first point of the window.
Reproduced on the stock detail: after hovering anywhere the tooltip text never changes.

- [ ] Replace the `Scatter` with one `ReferenceDot` per marker:
  `<ReferenceDot key={m.cluster.id} x={m.t} y={m.y} r={8} ifOverflow="visible" shape={(p) => <EventMark cx={p.cx} cy={p.cy} … />} />`
  (Recharts 3 `ReferenceDot` calls a function `shape` with `cx`/`cy`; default z-index is above
  lines). Remove the `Scatter` import.
- [ ] Keep the public props of `MoonyLineChart` unchanged (other packages rely on them).
- [ ] Hover state: entering a mark sets it and calls `onHotEventChange(firstId)`; leaving clears it;
  add `onMouseLeave` on the chart wrapper that clears a set hover (and `onHotEventChange(null)`), so a
  mark can never stay "stuck".
- [ ] Short comment at the marks explaining why they are reference dots (the Recharts rule above).
- [ ] Gates; commit `fix(charts): event marks no longer freeze the price tooltip`.

### A2 — Trend cards, stock trade marks, cash chart, import entry (spec §3.3, §3.5, §3.6, F2, F7)

- [ ] **`trade-events` helper (TDD).** `src/utils/trade-events.ts`:
  ```ts
  export interface TradeLike {
    type: string; ticker: string; quantity: string; pricePerUnit: string;
    currency?: string | null; transactionDate: number;
  }
  export interface TradeDayEvent extends ChartEvent {
    type: 'buy' | 'sell'; tickers: string[]; amountCzk: number;
  }
  /** One event per UTC day and direction; amounts converted at each trade's date. */
  export function tradeDayEvents(
    trades: readonly TradeLike[],
    toCzk: (amount: number, currency: string, day: number) => number
  ): TradeDayEvent[];
  ```
  id `${type}-${day}`, `t` = UTC day, tickers unique in first-seen order, `'sell'` → sell, anything
  else → buy, sorted by `t` then buy before sell, `currency` defaults to `'CZK'`. Tests: same-day
  grouping, both directions on one day, unique tickers, conversion receives the trade date, ordering.
  `Crypto.tsx` replaces its inline builder with it (same output).
- [ ] **`PortfolioTrendCard`.** Use `TrendCard` and `TREND_CHART_HEIGHT` for every type. Replace the
  `transactionMarkers` prop with `earliest?: number` (first day with data; keep the prop optional so
  `RealEstate.tsx` and `OtherAssets.tsx` stay untouched). Periods: `CHART_PERIODS`, default `'1Y'`.
  Range: `const start = chartPeriodStart(period, Date.now() / 1000, earliest)`; query key
  `['portfolio-history', 'trend', start ?? 'all']`, fetch `portfolioApi.getHistory(start, Math.floor(Date.now() / 1000))`
  computed inside `queryFn` (never in the key). This removes the request storm (F2). Add the type
  `'cash'`: values `convertPoint(h.recordedAt, h.savingsByCurrency, Number(h.totalSavings || 0))`,
  texts from `bank_accounts` `trend.title` / `trend.subtitle`. Events are filtered to `t >= start`.
- [ ] **Stocks page.** Pass `earliest` (first transaction day) instead of markers. Draw buy/sell marks
  with `tradeDayEvents` (convert with `useDatedConvert`), tooltip like crypto
  (`chart.events.buy|sell` with tickers, line `day · amount`, cluster title `chart.events.cluster`),
  legend Hodnota / Nákup / Prodej. Head actions: Obnovit ceny · Export · **Importovat CSV**
  (`variant="outline"`, `Upload` icon, opens the existing import modal) · Přidat investici. Remove the
  `onImportCsv` prop and footer button from `AddInvestmentModal`. The empty state keeps both buttons.
- [ ] **Crypto page.** `earliest` instead of markers; the helper for events.
- [ ] **Bank accounts page.** Totals count only accounts with `!excludeFromBalance` (as the dashboard
  and `totalSavings`); the note of "Celkem na účtech" appends `metrics.excludedHint` with the excluded
  amount when there is any. Insert `<PortfolioTrendCard type="cash" currentValue={totals.balance} />`
  between the stats and the table. New keys `trend.title` ("Vývoj hotovosti" / "Cash over time") and
  `trend.subtitle` ("Podle zůstatků zaznamenaných v Moony. Pohyby podle transakcí ukazuje detail
  účtu." / "From the balances recorded in Moony. The account detail shows the movements from
  transactions.").
- [ ] **Bonds ladder.** `TrendCard` (aside = the badge) and `TREND_CHART_HEIGHT`.
- [ ] Gates; commits per bullet (`feat(stocks): buy and sell marks on the portfolio trend`, …).

### A3 — Dashboard and detail horizons (spec §3.2, §3.4, F10)

- [ ] `Dashboard.tsx`: `CHART_PERIODS` (long labels); range from `chartPeriodStart(period, now)`;
  history query key `['portfolio-history', 'dashboard', start ?? 'all']`, end computed in `queryFn`;
  `periodStartSeconds` = that start; hero chart `height={220}` and skeleton `h-[220px]`; remove the
  `link` of the Recent moves `SectionHead` and delete the now unused `moves.all` keys (both locales);
  drop unused `date-fns` imports.
- [ ] `PositionHero.tsx`: `CHART_PERIODS` (default stays `'All'`), start from `chartPeriodStart`,
  key `['ticker-history', type, ticker, start ?? 'all']`.
- [ ] `BankAccountDetail.tsx`: `CHART_PERIODS`; `chartFrom = chartPeriodStart(chartPeriod, Date.now() / 1000)`.
- [ ] Gates; commits `feat(dashboard): taller net worth chart and the year-to-date horizon`,
  `refactor(charts): one set of horizons for position and account charts`.

### A4 — Loans: new-loan marks (spec §3.7)

- [ ] **Vertical step (TDD)** in `debtSeries`: for every loan whose first reading is later than the
  earliest first reading of all loans, add the stamp `first.t − DAY` to the grid and keep it through
  thinning, so the jump is a one-day step. Test in `loan-trajectory.test.ts`: two loans, the second
  starting later → value at `start − DAY` excludes it, value at `start` includes it, the existing
  test still passes.
- [ ] `DebtTrajectoryCard`: `TrendCard` (aside = horizon segment, legend = `ChartLegend`),
  `TREND_CHART_HEIGHT`. Add one mark per loan start for **all** rows (paid-off ones too):
  `{ id: 'start-<id>', t: r.terms.startDay, type: 'buy', title: chart.milestones.newLoan {name},
  lines: ['<day> · <principal>'] }`, only inside the visible window. Legend item "Nový úvěr"
  (`{ kind: 'event', type: 'buy' }`). Cluster title: all marks milestones → existing `several`;
  mixed → new `chart.events.several` ("N událostí", plurals).
- [ ] `LoanDetail.tsx`: the drawdown mark becomes `type: 'buy'` with a legend item "Čerpání".
- [ ] Gates; commit `feat(loans): mark new loans on the debt trajectory`.

### A5 — Insurance table (spec §3.8, F8)

- [ ] **Largest limit (TDD).** `InsuranceRow` gains `topLimit: InsuranceLimit | null` (largest after
  conversion to CZK) and `limitCount: number`. Test with two currencies where the raw amounts order
  differently than the CZK amounts.
- [ ] `InsuranceTable`: `<Table className="table-fixed">`; columns Pojistka (≈30 %, logo + name,
  sub "<type> · <insurer>"), Pojistné, Krytí (compact total in display currency; sub "<top limit
  title> <amount in its own currency> · +N"), Příští platba, Výročí / konec, actions (`w-11`). Every
  text cell `truncate` with `title` for the full text; headers `truncate`; rows use the default 56 px
  (`h-row`, drop `h-[62px]`). No horizontal scroll at a 720 px content width. Remove the Type and
  Insurer columns and fix `compact(l.amount)` (it formatted foreign limits as CZK).
- [ ] `PaymentCalendarCard`: `TrendCard` + `TREND_CHART_HEIGHT`.
- [ ] Gates; commit `fix(insurance): a policies table that fits without scrolling`.

### A6 — Real estate valuation trace and purchase date (spec §3.10, F2, F3)

- [ ] **Migration `002_real_estate_purchase_date`** (append to `all_migrations()`):
  ```sql
  ALTER TABLE real_estate ADD COLUMN purchase_date INTEGER;
  INSERT INTO real_estate_valuations (id, real_estate_id, value, currency, valued_at, note, created_at)
  SELECT <uuid-v4 expression>, r.id, r.market_price, r.market_price_currency,
         (r.created_at / 86400) * 86400, NULL, unixepoch()
  FROM real_estate r
  WHERE CAST(r.market_price AS REAL) > 0
    AND NOT EXISTS (SELECT 1 FROM real_estate_valuations v WHERE v.real_estate_id = r.id);
  -- same for other_assets → other_asset_valuations (columns asset_id, value, currency)
  ```
  The id is a lowercase UUID v4 built in SQL
  (`lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))`).
  Regenerate `schema.snapshot` (`cargo test regenerate_schema_snapshot -- --ignored`), amend the
  catalog in `docs/architecture/database.md`. Tests: a migration test that seeds empty logs, leaves
  non-empty logs untouched and skips zero prices.
- [ ] **Initial row on creation.** `services::real_estate::create_property` and
  `services::other_assets::create_asset` write the first valuation row (market price, its currency,
  the creation day; only when the price is > 0) through a new
  `valuations::record_initial_valuation(conn, kind, asset_id, value, currency, day)`. Tests in both
  services.
- [ ] **`purchase_date`.** `RealEstate.purchase_date: Option<i64>` (`purchaseDate`), the same on
  `InsertRealEstate` with `validate()` rejecting a date after today (`validation.purchaseDateInFuture`,
  both locales in `common` validation keys). Read in both SELECTs of `commands/real_estate.rs`, written
  by `create_property` and set directly by `update_real_estate` (the update replaces the whole record,
  as for rent and notes). The MCP tool `real_estate_create` takes an optional `purchaseDate` (unix
  seconds). `shared/schema.ts` mirrors it; regenerate bindings. `realEstateUpdatePayload` sends it
  (test). The add/edit modal gets an optional "Datum koupě" date field.
- [ ] **Trace builder (TDD).** `src/utils/valuation-trace.ts`:
  ```ts
  export interface TraceValuation { id: string; t: number; value: number; createdAt: number }
  export interface TracePoint { t: number; value: number }
  export function valuationTrace(input: {
    valuations: readonly TraceValuation[];
    purchase?: { t: number; value: number } | null;
    now: number;
    start?: number;
  }): TracePoint[];
  ```
  Sort by day then creation; same-day rows collapse to the last; the purchase point comes first when
  its day precedes the first valuation; a step point the day before each change; a final point at
  `now`; with `start`, everything before it is replaced by one anchor at `start`; strictly increasing
  `t`. Tests for each rule. `RealEstateDetail.tsx` uses it, adds a purchase event (`type: 'buy'`,
  "Koupě · <price>") and its legend item, shows the purchase date in the eyebrow when known, and uses
  i18n for the hard-coded "1R"/"3R" segment labels (`common.periods.1Y`, new `common.periods.3Y`).
- [ ] **Freshness.** Property saves (modal, `use-real-estate-mutations`) and valuation mutations
  invalidate `['real-estate-valuations', id]`.
- [ ] Gates incl. Rust; commits `feat(real-estate): purchase date and a complete valuation log`,
  `fix(real-estate): the value trace shows every estimate`.

### A7 — Real estate table (spec §3.9, F9)

- [ ] In `RealEstate.tsx` only the table card changes: `<Table className="table-fixed">`, columns
  Nemovitost (≈30 %, logo + name + "mimo jmění" badge, sub "<type> · <address>"), Tržní hodnota (sub
  `table.boughtFor` "koupeno za X"), Zhodnocení (signed amount, sub signed %), Nájem / výnos (monthly
  rent or "bez nájmu", sub gross yield), Vlastní kapitál (sub "LTV Y %" or "bez úvěru"; the progress
  bar goes), actions (`w-11`). Remove the `'purchase'` sort column; rows `h-row` (56 px); `truncate`
  with `title` on text cells, headers `truncate`; `colSpan` follows the new count. Leave the trend
  card call above the table untouched (A2 changes its component).
- [ ] Gates; commit `fix(real-estate): a properties table that fits without scrolling`.

### A8 — Stocks analysis (spec §3.11, F4)

- [ ] **Service move (TDD).** Move the series assembly of `get_stock_twr` into
  `services::investments::twr_series(conn, tag_ids, include_portfolio, include_untagged,
  investment_ids: Option<&[String]>, from_ts, to_ts) -> Result<Vec<TwrSeries>>`; the command becomes
  thin and gains `investment_ids: Option<Vec<String>>`. Every tag series uses only tickers of the given
  investments when the list is present; the whole-portfolio series is never restricted. Tests: tag
  series with and without the restriction; portfolio unchanged. Add `#[serde(rename = "isUntagged")]`
  to `TwrSeries.is_untagged`; regenerate bindings. `tauri-api.ts` `getStockTwr` gets an optional last
  parameter `investmentIds?: string[]`.
- [ ] **Percent helper (TDD).** `src/utils/twr.ts`: `twrPercent(index: number): number` (`index / 100 − 1`).
- [ ] **`MoonyLinesChart` (opt-in, defaults unchanged for the annuity calculator):** prop
  `labels?: 'end' | 'legend'` (`'legend'` renders a legend row with name and last value under the chart
  instead of labels over the tick gutter) and series `reference?: boolean` (dark `chart-line`, 2 px,
  dashed `4 4`, drawn last, first in the legend).
- [ ] **`StocksAnalysis.tsx`:**
  - grid without `items-start`; the allocation card is a flex column whose body is
    `flex flex-1 items-center`; `AllocationRing size="lg"` from 1240 px viewport width
    (`useMediaQuery('(min-width: 1240px)')`, new hook in `src/hooks/use-media-query.ts`) and `md`
    below;
  - TWR query with `placeholderData: keepPreviousData`, so the card height never jumps;
  - the chips filter the analysis: when any chip is active, the ring, the TWR tag series (pass the
    selected investment ids), the tag table and the holdings table use the selected positions only;
    the stats row stays portfolio-wide; "Zrušit filtr" link; subtitles "Vybrané pozice: N z M";
  - TWR in percent (axis and labels via `twrPercent` and `fmt.percent`, signed), the whole portfolio
    as the `reference` series "Celé portfolio", its period return in the card head, `labels="legend"`.
- [ ] Gates incl. Rust; commits `refactor(stocks): TWR series assembled in the service`,
  `feat(analysis): chips filter the analysis, TWR in percent with the whole portfolio`.

### A9 — Company data on the stock detail, 52-week range (spec §3.12, §3.13, F5, F6)

- [ ] **Model + service (TDD).** `StockCompanyInfo { ticker, sector, industry, peRatio, forwardPe,
  marketCap, beta, fiftyTwoWeekHigh, fiftyTwoWeekLow, dividendRate, dividendYield, quoteType,
  currency, metadataFetchedAt }` (text fields `Option<String>`, timestamp `Option<i64>`, camelCase
  renames) in `models/company_info.rs`; `services::company_info::read_company_info(conn, ticker)`
  reads the `stock_data` columns (`sector, industry, pe_ratio, forward_pe, market_cap, beta,
  fifty_two_week_high, fifty_two_week_low, trailing_dividend_rate, trailing_dividend_yield,
  quote_type, currency, metadata_fetched_at`); `metadata_is_stale(fetched_at, now)` (24 h). Tests:
  a row with metadata, a row without, a missing ticker, staleness.
- [ ] **Command.** `commands/company_info.rs`: `get_stock_company_info(ticker, refresh: bool)`; when
  `refresh` and stale, call `price_api::refresh_stock_metadata_yahoo(&db, vec![ticker], false)`
  outside `db.with_conn` (as `commands/stock_monitor.rs` does), then read. Register in `lib.rs`,
  `bindings.rs`, mirror in `shared/schema.ts`, regenerate. In `price_api.rs` the metadata fetch
  failures log at `warn`. `tauri-api.ts`: `investmentsApi.getCompanyInfo(ticker, refresh)`; delete the
  dead `refreshMetadata` wrapper.
- [ ] **Stock detail card.** Query `['stock-company-info', ticker]` with `refresh: true`; the price
  refresh invalidates it. Rows: Sektor · Odvětví first, then the existing figures, only rows with a
  value; for `quoteType === 'ETF'` without figures the note `detail.companyInfo.etfNote`; nothing
  stored → `detail.companyInfo.unavailable` with a retry button. Delete the orphan
  `detail.refreshMetadata` / `metadataRefreshed` keys. The card no longer reads these fields from the
  deprecated `extended-types.ts` type.
- [ ] **Range bar.** `RangeWithLabels` gets `fluid?: boolean` (wrapper `flex w-full`, bar
  `w-auto min-w-0 flex-1 shrink`, labels `shrink-0`); `StockMonitorDetail` passes `fluid` and an
  accessible `label`; drop the `[&>span:nth-child(2)]:w-full` hack. The table usage is unchanged.
- [ ] Gates incl. Rust; commits `fix(stocks): company data on the position detail`,
  `fix(stock-monitor): the 52-week range stays inside its card`.

---

## Integration (coordinator)

- [ ] Merge the package branches into `feature/overview-polish` one by one (A1, A3, A2, A4, A5, A9,
  A8, A6, A7), resolve conflicts, regenerate `shared/generated-types.ts` and `schema.snapshot` where
  they conflict, run all gates after each merge.
- [ ] Integration touches: the purchase year in the real estate table sub-line (A6 × A7); design
  system README (§7 List trend card at 170 px and stock marks, §8 TWR in percent, §9 aggregate stock
  marks), `CHANGELOG.md`, `docs/DEPRECATED.md` if something was removed.
- [ ] Verification in the demo profile per spec §6 at 1440 and 1080 px, Czech and English, with
  screenshots; fix what fails.
- [ ] Review pass (code-review at high effort) and fixes; final gates; local merge to `main`.
