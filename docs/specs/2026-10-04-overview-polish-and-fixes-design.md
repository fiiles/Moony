# Overview polish and fixes — design

Date: 2026-10-04 · Status: approved by the owner on 2026-10-04 · Companion spec:
`2026-10-04-stock-csv-import-wizard-design.md` (part B, the stock import wizard).

## 1. Context

The owner sent sixteen requests about the published app (0.9.0): taller and unified charts, a
year-to-date horizon, a cash chart for bank accounts, broken chart tooltips, unreadable insurance
and real estate tables, a real estate chart that shows one point, the stocks analysis page and two
stock detail problems. The app is public, so every change ships at production quality: root causes
fixed (not symptoms), both locales, tests for pure logic and services, and verification in the
running demo profile at the default (1400 px) and the minimum (1080 px) window width.

This spec covers everything except the stock CSV import wizard, which has its own spec. The
"Import CSV" entry point on the stocks page (request 5) is part of this spec; until part B lands
it opens the existing import modal.

### Owner decisions (2026-10-04)

- The tag-group chips on the stocks analysis page filter the whole analysis below them.
- Proposals accepted without change: the cash chart is reconstructed from transactions, all list
  trend cards share one card at 170 px, the same five horizons on every value chart, TWR in
  percent with a prominent whole-portfolio line, an optional purchase date for real estate.

## 2. Findings

Each finding was reproduced in the demo profile or traced in code.

| # | Symptom | Root cause |
|---|---|---|
| F1 | Hovering a buy/sell mark shows the event, then the price tooltip "freezes" on one value while the crosshair moves | `MoonyLineChart` draws marks as a Recharts `Scatter` with its own `data`. In Recharts 3 (`combineDisplayedData`) any graphical item with own data replaces the chart data as the axis' displayed data, so the tooltip resolves its active index against the marks and falls back to `rows[index]`, i.e. the first point of the window. Every chart with marks is affected (stock, crypto, bank account, loan, real estate and watchlist details, the crypto list, the loans list). |
| F2 | Real estate list: the value chart is a single point; same on Other assets | `PortfolioTrendCard` defaults `transactionMarkers = []` (a new array per render). Its date-range memo then recomputes `new Date()` every render, the query key (millisecond ISO strings) changes every render and React Query never returns data. Measured: about 34 `get_portfolio_history` calls per second while the page is open. |
| F3 | Real estate detail: after a revaluation the chart has one point | The valuation log is never seeded. Creating a property writes no row, so the first revaluation overwrites the only record of the original estimate. The purchase is not a point because a property has no purchase date. Secondary: property edits do not invalidate `['real-estate-valuations', id]`; two revaluations on the same day produce unsorted points. |
| F4 | Stocks analysis: the switches above the two widgets do nothing; the portfolio is missing from TWR | The tag-group chips only filter the holdings table at the bottom of the page. The whole-portfolio TWR series exists but is drawn 1.5 px in `chart-cost`, almost the tone of the third tag series, and its end label overlaps the value ticks. The cards differ in height (`items-start`, 130 px ring vs 190 px chart) and the TWR card shrinks by 34 px on every switch (no previous data kept). Latent: `TwrSeries.is_untagged` lacks its camelCase rename, so `isUntagged` is always undefined on the wire. |
| F5 | Watchlist detail: the 52-week range overflows its card | `RangeWithLabels` is given `[&>span:nth-child(2)]:w-full`; the bar becomes 100 % wide and `shrink-0`, and the min/max labels plus gaps are added on top. |
| F6 | Stock detail "About the company" shows only dashes | Company data is fetched only for watched tickers (`refresh_stock_metadata_yahoo` has four callers, all in the stock monitor). `get_investment` never selects the `stock_data` metadata columns, and the page reads fields typed by the deprecated `extended-types.ts`. `tauri-api.ts` still wraps a non-existent `refresh_stock_metadata` command. |
| F7 | Bank accounts total differs from the dashboard's free cash | The bank page sums every account; the dashboard and `totalSavings` leave out accounts excluded from net worth. The i18n key `bank_accounts.metrics.excludedHint` exists but is unused. |
| F8 | Insurance table always scrolls horizontally | Auto table layout, eight columns and nowrap sub-lines; the coverage sub-line concatenates free-text limit titles of unbounded length. `truncate` has no effect in an auto-layout cell. Also: limit amounts are formatted as CZK although they are in the limit's own currency, and limits are sorted across currencies by raw amount. |
| F9 | Real estate table scrolls horizontally | Same layout mechanism as F8 with eight columns of two-line cells (~1,100–1,180 px against 1,040 px at 1400 px and 720 px at 1080 px). |
| F10 | "Recent moves → Show all" opens bank accounts | The list mixes bank transactions, stock and crypto trades; no page lists all of them. Each row already links to its source. |

## 3. Design

### 3.1 Chart core: marks that do not hijack the tooltip (F1)

`MoonyLineChart` renders each event cluster as a Recharts `ReferenceDot` (`ifOverflow="visible"`,
key = cluster id) whose `shape` draws the existing `EventMark`. Reference elements do not register
item data, so the tooltip keeps resolving against the value series. The hover state stays local:
entering a mark shows the event tip and hides the price tooltip; leaving it, or leaving the chart
area (`onMouseLeave` on the chart wrapper), clears it. Table-row highlighting (`hotEventId`) and
click-to-scroll keep working. No other component draws marks, so the fix covers every chart listed
in F1.

### 3.2 One set of horizons, computed in UTC (request 2)

`src/utils/period.ts` (ADR 0008: the only place that turns periods into epoch bounds) gains:

- `CHART_PERIODS = ['30D', '90D', 'YTD', '1Y', 'All']` — the segment shown on every value chart.
- `chartPeriodStart(period, nowSec, earliestSec?)` → UTC-day start in seconds, or `undefined` for
  "All" without a known earliest day. Rolling periods count back from today's UTC day; YTD is
  1 January 00:00 UTC; the result never precedes `earliestSec`.

Users: the dashboard, the shared trend card, the stock/crypto position hero and the bank account
detail. Query keys use the day-granular start (seconds), never `new Date()` inside render, so a key
is stable for the whole day (fixes F2 by construction). Labels exist in both locales
(`common.periods`/`periodsLong`, "Letos" / "This year").

### 3.3 Trend cards (requests 7 and 8, F2)

- New `TrendCard` shell (`src/components/charts/TrendCard.tsx`): title, one sentence, a control on
  the right, the chart, an optional legend — the orb-card markup that is copied four times today.
  `TREND_CHART_HEIGHT = 170`.
- Users: `PortfolioTrendCard` (stocks, crypto, real estate, other assets), the new cash card,
  `DebtTrajectoryCard`, `BondsLadderCard`, `PaymentCalendarCard`. All charts become 170 px
  (previously 96/110/170/170/150).
- `PortfolioTrendCard` takes the earliest transaction day as a number (no default array), uses
  `chartPeriodStart`, offers the five horizons, default 1R.
- The stocks list draws buy and sell marks like crypto: one mark per UTC day and direction,
  tooltip "Nákup AAPL, MSFT · 12 400 Kč" (amount at the day's rate), clusters above 24 marks, legend
  "Hodnota · Nákup · Prodej". The day grouping moves from `Crypto.tsx` into a tested pure helper
  `src/utils/trade-events.ts` used by both pages.

### 3.4 Dashboard (requests 1–3, F10)

- Net worth chart 170 → 220 px (skeleton too).
- Horizons: `CHART_PERIODS` with long labels; the change sentence for YTD already exists
  ("od začátku roku").
- "Zobrazit vše" is removed from Recent moves. Rows keep linking to their source.

### 3.5 Bank accounts: cash over time (request 4, F7)

- A `TrendCard` "Vývoj hotovosti" between the stats and the table, five horizons, 170 px, no
  marks (aggregate chart, design system §9).
- Series: for every account counted in net worth, `reconstructBalance` (the account detail's
  tested walk-back from the stored balance through its transactions) over the selected span, each
  day converted to the display currency at today's rate (as the account detail does), summed per
  day. Transactions are loaded per account with `useQueries` (`dateFrom` = period start; "Vše" from
  the earliest transaction). The subtitle says it is reconstructed from transactions.
- Totals become consistent with the dashboard: "Celkem na účtech" sums accounts counted in net
  worth; when some are excluded the note adds "(+ X vyloučeno)" (existing key `excludedHint`).

### 3.6 Stocks list: import entry (request 5)

"Importovat CSV" (outline, Upload icon) joins the page head actions before the primary
"Přidat investici", as on bank accounts. The import button leaves the footer of
`AddInvestmentModal` (the prop `onImportCsv` is removed); the empty state keeps both buttons.

### 3.7 Loans: new-loan marks (request 10)

- `DebtTrajectoryCard` adds one mark per loan start (all loans, paid-off ones included), glyph ▲
  (`buy` type, which reads "the debt went up"), tooltip "Nový úvěr · <name> · <principal>" and the
  date. Legend item "Nový úvěr" next to "Milník". Mixed clusters read "N událostí".
- `debtSeries` adds a reading on the day before each later start, so the jump is vertical instead
  of a ramp (pure, tested in `src/utils/loan-trajectory.test.ts`).
- The loan detail's drawdown mark uses the same glyph and label.

### 3.8 Insurance table (request 11, F8)

Five data columns plus actions, `table-fixed` with explicit widths so long text ends with "…":

| Column | Main line | Sub-line |
|---|---|---|
| Pojistka (≈ 32 %) | provider logo + policy name | type · insurer |
| Pojistné | amount per payment | frequency · yearly total |
| Krytí | total of all limits (compact, display currency) | largest limit title + amount (own currency) · "+N" |
| Příští platba | date | amount |
| Výročí / konec | "za N měs." badge or date | "výročí smlouvy" / "konec smlouvy" / "ukončeno" |
| ··· | row actions | |

Rows 56 px (design system §6). Limit amounts are formatted in their own currency and the largest
limit is chosen after conversion to CZK. Full limit titles stay on the detail page. No horizontal
scroll at 1080 px.

### 3.9 Real estate table (request 12, F9)

Five data columns plus actions, same `table-fixed` treatment, rows 56 px:

| Column | Main line | Sub-line |
|---|---|---|
| Nemovitost (≈ 30 %) | logo + name (+ "mimo jmění" badge) | type · address |
| Tržní hodnota | market value | "koupeno za X" (+ year when the purchase date is known) |
| Zhodnocení | signed amount | signed % |
| Nájem / výnos | monthly rent or "bez nájmu" | gross yield |
| Vlastní kapitál | equity | "LTV Y %" or "bez úvěru" |
| ··· | row actions | |

### 3.10 Real estate valuation trace (request 13, F2, F3)

- Migration `002_real_estate_purchase_date`: `ALTER TABLE real_estate ADD COLUMN purchase_date
  INTEGER` (nullable, UTC day) and a one-time seed of the empty valuation logs: for every real
  estate and other asset without a valuation row, insert its current market price dated at the
  UTC day of its `created_at`. Existing rows are untouched.
- `create_property` (UI and MCP, ADR 0007) and the other-asset creation write the initial
  valuation row, so every asset has a log from day one; the first revaluation no longer loses the
  previous estimate.
- `purchaseDate` joins `RealEstate` / `InsertRealEstate` (validated: not in the future), the
  add/edit modal ("Datum koupě", optional) and the MCP `real_estate_create` tool (optional).
- The detail trace starts with a "Koupě" point (purchase price at the purchase date) when the date
  is known, then the valuation rows (sorted by day, then creation), then today. The purchase stays
  the dashed reference line as well. Same-day revaluations collapse to the last one of that day.
- Property and valuation mutations invalidate `['real-estate-valuations', id]`.
- Out of scope, recorded as a known limitation: the aggregate history (dashboard, list card) shows
  a revaluation from the day it is entered; backdating does not rewrite past snapshots for real
  estate (other assets already do).

### 3.11 Stocks analysis (request 14, F4)

- **Equal heights:** the two-card grid stretches; the allocation card is a flex column whose body
  centres an `AllocationRing size="lg"` (the dashboard's approach). Below ~1240 px viewport width
  the ring falls back to `md` so the legend keeps room. The TWR card keeps the previous series while
  a new one loads (`placeholderData: keepPreviousData`), so it never changes height.
- **Chips filter the analysis:** the selection (one tag per group, AND across groups) restricts the
  ring, the TWR tag series, the tag table and the holdings table. The stats row above the chips stays
  portfolio-wide. A "Zrušit filtr" link appears while a filter is active; the cards' subtitles say
  "Vybrané pozice: N z M".
- **TWR backend:** the ticker selection moves from `commands/investments.rs` into a tested service
  function; `get_stock_twr` gains an optional list of investment ids that every tag series is
  intersected with. The whole-portfolio series is never restricted. `is_untagged` gets its
  `#[serde(rename = "isUntagged")]`.
- **TWR presentation:** values as cumulative percent ("+12,4 %", axis "0 %", "+10 %") instead of
  an index of 100; the whole portfolio is the dark (`ink`) 2 px dashed line named "Celé portfolio"
  and its return for the period is stated in the card head; series labels with values move from
  the tick gutter into a legend row under the chart, so long tag names no longer collide with the
  axis.

### 3.12 Watchlist detail: 52-week range (request 15, F5)

`RangeWithLabels` gets a `fluid` prop: the wrapper becomes a full-width flex row, the bar takes the
remaining width (`flex-1 min-w-0`), labels never shrink. The stat passes `fluid` and an accessible
label. The table usage keeps the fixed 92 px bar.

### 3.13 Stock detail: company data (request 16, F6)

- New service function + thin command `get_stock_company_info(ticker, refresh)`: when `refresh` is
  true and the stored metadata is older than 24 h, it calls `refresh_stock_metadata_yahoo` (network
  outside the DB lock, as the stock monitor does), then returns `StockCompanyInfo` read from
  `stock_data`: sector, industry, P/E, forward P/E, market cap, beta, 52-week high/low, dividend
  rate and yield, quote type, metadata timestamp. Registered type in `bindings.rs`, mirrored in
  `shared/schema.ts`. The dead `refreshMetadata` wrapper is removed.
- The page queries it on mount (`refresh: true`) and after "Obnovit ceny"
  (`['stock-company-info', ticker]`). Metadata fetch failures are logged at `warn`, not `debug`.
- The card lists sector · industry first, shows only rows with a value, says "Pro ETF Yahoo
  Finance tyto údaje neuvádí" for ETFs without them, and "Údaje se nepodařilo načíst" with a retry
  when nothing is stored. The fields leave the deprecated `extended-types.ts` usage for this card.

## 4. Data and contract changes

- Migration `002_real_estate_purchase_date` (+ schema snapshot regeneration, catalog in
  `docs/architecture/database.md`).
- Rust types: `RealEstate.purchase_date`, `InsertRealEstate.purchase_date`, `StockCompanyInfo`,
  `TwrSeries` rename fix; registered in `bindings.rs`, mirrored in `shared/schema.ts`,
  `shared/generated-types.ts` regenerated (7-step procedure, `docs/standards/type-contract.md`).
- Commands: `get_stock_company_info` (new), `get_stock_twr` (optional `investment_ids`).
- Mutation contract (AGENTS rule 7) unchanged; the valuation and property mutations add the
  valuation key.

## 5. Testing

- Vitest (pure): `chartPeriodStart` (each period, YTD across New Year, earliest clamp, UTC),
  `trade-events` grouping (per day and direction, currency conversion, ordering), the debt series
  vertical step, the TWR percent conversion helper, the real estate trace builder (purchase point,
  same-day collapse, ordering), the insurance largest-limit selection across currencies.
- Rust: migration seeding (empty logs seeded, non-empty untouched), `create_property` writes the
  initial row, other-asset creation likewise, `purchase_date` validation, the TWR ticker selection
  with and without the investment filter, `StockCompanyInfo` read from a `stock_data` row.
- Regression tests that fail before the fix where the code is testable (valuation seeding, debt
  step, period helper key stability).
- Gates: lint, typecheck, Vitest, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
  bindings drift.

## 6. Verification in the running app

Demo profile (`moony-demo`), English and Czech, at 1440 and 1080 px:

1. Stock detail: move along the curve, over a mark, and back — the price tooltip follows the
   cursor throughout. Same on crypto list and bank account detail.
2. Real estate and other assets lists: the chart shows history; no request storm (counted).
3. Revalue a property twice (one backdated): the detail trace shows purchase → estimates → today.
4. Dashboard: 220 px chart, YTD selectable, no "Zobrazit vše".
5. Every list page: the trend card is 170 px with the same chrome; stocks show buy/sell marks.
6. Bank accounts: cash chart ends at the stat value; excluded account shows in the note.
7. Loans: a second loan shows a vertical step with a ▲ mark.
8. Insurance (with long limit titles) and real estate tables: no horizontal scroll at 1080 px.
9. Analysis: equal card heights at both widths; chips change ring, TWR, tables; portfolio line
   visible; no height jump on switching.
10. Watchlist detail: range bar inside the card at 1080 px.
11. Stock detail: company data appears (network on); ETF case worded.

## 7. Out of scope

- Rewriting past real estate snapshots for backdated revaluations.
- A per-asset valuation trace for other assets (the README §7 claim stays a gap).
- Responsive review of the other list tables (stocks, crypto, bonds, loans, other assets); checked
  at 1080 px during verification and reported, not changed.
