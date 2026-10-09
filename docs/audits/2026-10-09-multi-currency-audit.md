# Multi-currency audit — 2026-10-09

Read-only audit of how Moony stores, converts and presents money in more than one currency, done after the report-currency change (ADR 0011, audit `2026-10-08-report-currencies.md`). Question from the owner: Moony is no longer aimed purely at the Czech market — should the implicit CZK base be removed?

Method: three parallel static audits (Rust backend; React frontend; storage, rate sources and integrations), each finding located by file and line, plus live read-only checks of the ECB daily feed and frankfurter.dev on 2026-10-08/09. The highest-severity findings were re-verified by hand (marked ✔). Line numbers are a snapshot of `main` on 2026-10-09.

## 1. Current model in one paragraph

Native amount + currency is stored on every entity and transaction and is authoritative. Exchange rates are stored as **CZK per 1 unit** (`exchange_rates`, `exchange_rate_history`, in-memory `EXCHANGE_RATES` in Rust and a second mutable copy in `shared/currencies.ts`). Two conversion regimes coexist:

- **Report regime (ADR 0011)** — cashflow actuals, budgeting, drilldowns, planned cashflow. The backend converts directly into an explicit `reportCurrency` (historical: common quote day ≤ booking, 10-day lookback; planning: one current snapshot), missing FX is explicit, output is decimal TEXT at ISO minor units, the frontend only formats. This regime is sound.
- **Portfolio regime (legacy ADR 0001)** — net worth, asset pages, stocks/crypto, tags/analysis, TWR, projection, MCP aggregates. The backend sums in CZK at current in-memory rates (`convert_to_czk`, 1.0 fallback for unknown currencies, hardcoded fallback rates), returns unlabelled CZK numbers, and the frontend converts CZK → display currency with its own rate table (`formatCurrency*`, `convert`). Snapshots persist CZK totals plus native `*_by_currency` JSON; per-ticker history persists `value_czk` plus native price/currency.

## 2. Verdict

**The CZK pivot itself is not the defect.** Converting X → CZK → Y with one rate snapshot is mathematically identical to X → Y (CZK cancels; totals are stored as full-precision f64, so no rounding loss). The defects come from three properties of the portfolio regime:

1. **CZK leaks into results** whenever two legs use different dates' rates: cost basis converted to CZK at purchase-day rates, value at today's rate, then the CZK gain converted to the user's currency at today's rate. A EUR user's gain on a USD stock then contains the CZK/EUR move. TWR is computed on `value_czk` — a CZK investor's return.
2. **The unit is implicit.** ~25 wire fields and ~140 frontend call sites depend on the convention "number without currency = CZK". Choosing the wrong formatter double-converts or doesn't convert, and several pages do exactly that today.
3. **Missing rates degrade silently.** 15 of the 45 selectable currencies have no rate source; they convert 1:1 against CZK, so a CZK figure is displayed with an AED/SAR/… symbol.

**CZK as the rate reference is a structural risk, not a precision one.** The ECB feed is rejected as a whole if it lacks CZK; frankfurter days without CZK are skipped. If CZK stops being quoted (euro adoption — BGN disappeared from the ECB feed after 2025-12-31), all FX freezes, including for EUR/USD users who hold no CZK. EUR is the source's native base; storing it removes the inversion and the dependency.

Recommendation: make every monetary response carry an explicit currency, convert on the backend from native data (one shared conversion service for both regimes), compute investment returns in the report currency, make missing FX explicit everywhere, and move the rate reference to EUR. Keep the CZK caches only as clearly labelled, rebuildable derived data (or retire them once nothing reads them).

## 3. Findings

Severity: **High** = wrong number/symbol shown or persisted, or silent 1:1. **Medium** = inconsistent policy, fragile contract, stale data. **Low** = naming, defaults, debt.

### 3.1 CZK leaking into investment results (High)

| ID | Where | Finding |
|---|---|---|
| C1 ✔ | `commands/stock_tags.rs:344-359`; `services/mcp/analytics.rs:252-290,375-377` | Gain/loss and gain % = value at today's CZK rate − `cost_basis_czk` (purchase-day CZK rates), then converted to display/main currency at today's rate. Example: EUR profile, USD stock bought for 1000 USD when USDCZK = EURCZK = 25; today USDCZK 25, EURCZK 22 → true cost 1000 EUR / gain +136 EUR; app shows cost 1136 EUR / gain 0. Affects Stocks Analysis, tag metrics, MCP `stocks_analysis`/`tag_metrics`. `cost_basis_native` already exists. |
| C2 | `services/investments.rs:~918-1060` | Stock TWR runs on `value_czk` — a CZK-denominated return for every user. |
| C3 | `src/components/stocks/PositionHero.tsx:146` | Position history `valueCzk` converted at today's rate while the cost line on the same chart uses dated rates; the tooltip "return" mixes two FX regimes. Native price/quantity/currency per day are available. |
| C4 | `services/mcp/mod.rs:127` | MCP ticker history re-converts `value_czk` at the day's rate; does not equal native price × quantity. |

### 3.2 Wrong unit on screen (High)

| ID | Where | Finding |
|---|---|---|
| U1 ✔ | `src/pages/RealEstate.tsx:377-380`, `src/pages/OtherAssets.tsx:184` → `PortfolioTrendCard` | `currentValue` must be display currency; these pages pass a CZK sum. For any non-CZK profile the chart's last point jumps to the CZK figure. |
| U2 | `src/components/stocks/InvestmentsSummary.tsx:82` (from `Stocks.tsx:204-208`); `src/pages/StockDetail.tsx:206-208,586` | Estimated yearly dividend is CZK (`dividendYield` = CZK per share) but formatted with `formatCurrencyRaw` (no conversion) and the display symbol; dividend yield % divides CZK by display currency. |
| U3 | `src/components/bank-accounts/BankAccountZoneManager.tsx:33,78-98,256-259` | Interest-zone thresholds are account-native but formatted with `formatCurrency` (CZK→display conversion). |
| U4 | `src/pages/AnnuityCalculator.tsx:130-137` | Loading a loan copies the loan-currency principal into a display-currency input without conversion. |
| U5 | `shared/currencies.ts:261-268`, `currency-provider.tsx:74-87` | A currency outside `CURRENCY_CODES` (e.g. a KES/NGN listing) yields `NaN`, rendered as 0 and poisoning sums (Stocks summary can read 0). Backend uses 1.0 instead — the two layers disagree. |

### 3.3 Missing or invented rates (High/Medium)

| ID | Sev | Where | Finding |
|---|---|---|---|
| R1 | High | `shared/currencies.ts:14,93`; `currency.rs:83-118`; `currency-provider.tsx:96-128` | 15 selectable currencies have no rate source (AED ARS BGN BHD CLP COP EGP KWD OMR QAR RSD SAR TWD UAH VND). Backend portfolio math, snapshots, cost basis and MCP treat them 1:1 with CZK; the frontend shows CZK numbers with their symbol. Only Bank Accounts guards (falls back to CZK). Onboarding maps regions such as AE, SA, BG, UA, VN to these currencies by default (`locale-defaults.ts:26-64`). |
| R2 | High | `services/cost_basis.rs:75-96` | Cost-basis 1.0 fallback only logs; it does not record the currency as missing, so no UI badge. |
| R3 | High | `services/fx_backfill.rs:39-55,188` | FX history is backfilled only for transaction currencies (stock, crypto, bank). Quote currencies that differ from the transaction currency, crypto's USD prices, and currencies of balances, properties, loans, bonds and other assets get no history; historical values and snapshots fall back to current rates and are cached that way. |
| R4 | Medium | `currency.rs:14-30`, `shared/currencies.ts:174-190` | 14 hardcoded fallback rates (EUR 25, USD 23, …) are served as real until the first fetch; a fresh offline install writes `live` snapshots with them. Other ECB currencies are 1:1 until then. |
| R5 | Medium | `currency.rs:139-150,196-285` | Rates are never aged per currency; a currency that drops out of the feed (BGN today) is converted forever at its last rate. `ECB_CURRENCY_CODES` still lists BGN. |
| R6 | Medium | `currency.rs:477-505`, `ticker_history.rs:140-152`, `cost_basis.rs:83-90` | Day lookup picks the first day with *any* rows; a currency absent from that day's map falls to the current rate instead of an earlier day in the window. |
| R7 | Medium | `currency.rs:112,139,196-250` | Rates loaded from the DB are not validated; a stored 0 zeroes a currency, and `convert_from_czk` with 0 returns the amount unchanged. |
| R8 | Medium | `currency.rs:324-336`; `fx_backfill.rs:153-155,195`; `reporting.rs:89-93` | CZK is structurally required: feed without CZK is rejected whole, frankfurter days without CZK dropped, `historical_amount` hard-codes the CZK pivot in SQL. |

### 3.4 Native facts not preserved (High/Medium)

| ID | Sev | Where | Finding |
|---|---|---|---|
| N1 ✔ | High | `ProjectionParamsDialog.tsx:119-127`; `commands/projection.rs:547-550` | Monthly contribution typed in the display currency is converted to CZK at today's rate and stored with `contributionCurrency = 'CZK'`; the backend ignores `contributionCurrency`. 500 EUR becomes ~12,500 CZK and drifts. Original amount is not recoverable. |
| N2 | Medium | `cashflow_planning.rs:85,658,1149`; `real_estate.rs:109,189` | Rent or property cost JSON without a currency is valued as CZK instead of missing. |
| N3 | Medium | `price_api.rs:439-460`; `stock_import/instruments.rs:91` | Ticker-suffix → currency map defaults many exchanges (.TO .AX .OL .CO .WA .MC .LS .NS .KS .TW .SA .JO .TA …) to USD, and the result is stored as the transaction currency when an import file has none. |
| N4 | Medium | `commands/portfolio.rs:~1405-1580`; `portfolio_history.rs:270-283` | Breakdown repair rewrites static classes to `{"CZK": total}` when they deviate >25 % from the CZK total, and backfilled days carry real estate as a synthetic CZK bucket — native currency lost, CZK treated as truth. |
| N5 | Medium | `portfolio_history.rs:110,162` | `real_estate_by_currency` merges personal and investment property; the split exists only in CZK totals, so the "exclude personal real estate" view cannot be rebuilt natively. |

### 3.5 Precision and input bugs (High/Medium, currency-adjacent)

| ID | Sev | Where | Finding |
|---|---|---|---|
| P1 | High | `ManualPriceModal.tsx:63-66,128-141`; `ManualDividendModal.tsx:63-66,128-140` | Manual price and dividend are integers (`step="1"`, `Math.round`): $15.37 → 15, $0.24 → 0. |
| P2 | High | `price_api.rs:542` | Crypto price persisted as `{:.2}` USD: sub-cent coins store 0.00 and are valued 0. |
| P3 ✔ | High | `Stocks.tsx:182-197` → `BuyInvestmentModal.tsx:64-70`, `SellInvestmentModal.tsx:62-64`, `ManualPriceModal.tsx:62-66` | From the Stocks list the modals get `currency`/`avgCostCurrency` overwritten with the display currency but a native price. Buy/sell then fails the backend's currency-mismatch check for a non-matching profile currency; a manual price may be saved in the wrong currency. From Stock detail they work. |
| P4 | Medium | crypto modals (`BuyCryptoModal.tsx:72,161` and siblings) | Unit price truncated to 2 decimals on prefill/blur. |
| P5 | Medium | `currency-provider.tsx:99-125`; `mcp/money.rs:177`, `mcp/portfolio.rs:190`, `mcp/analytics.rs:235,333` | Provider formatters default to 0 decimals and MCP aggregates to 2 decimals for every currency (JPY, KWD wrong); reports already use ISO minor units. |
| P6 | Medium | `src/lib/format.ts:109-115` | `narrowSymbol` renders USD/CAD/AUD/SGD/MXN all as "$", SEK/NOK/DKK as "kr" — ambiguous for multi-currency users. |

### 3.6 Contract, consistency and staleness (Medium)

| ID | Where | Finding |
|---|---|---|
| K1 | wire types (§4) | ~25 implicitly-CZK response fields; `dividendYield` means per-share in one type and per-position in another; `StockInvestmentWithPrice`/`CryptoInvestmentWithPrice` exist twice with different shapes (`shared/schema.ts:607-623` vs `shared/types/extended-types.ts`). |
| K2 | `commands/portfolio.rs:60`, `commands/projection.rs:325`, `mcp/portfolio.rs:30`, `mcp/analytics.rs:195,300`, `stock_tags.rs:281` | Five separate aggregators, each converting via CZK; MCP/tags SQL re-implement the price resolver differently from `pricing.rs` (override expiry), so a position can be valued differently per screen. |
| K3 | `DataStatus.tsx:84-94`, `SyncProvider.tsx:68-72`, `currency-provider.tsx:62` | Rate refreshes after unlock update only the backend table; the frontend table and `ratesTimestamp` stay old. Only `portfolio-metrics` is invalidated; projection, analysis, tag metrics, history and investments keep older CZK values converted with newer frontend rates. |
| K4 | `use-bonds.ts`, `use-loans.ts`, `use-insurance.ts`, `OtherAssets.tsx:49`, `RealEstate.tsx:113-137` | `useMemo` conversions read the global rate table without depending on a rate revision. |
| K5 | `DebtTrajectoryCard.tsx:45-56`, `RealEstateDetail.tsx:311-323`, `RealEstate.tsx:116-125`, `utils/other-assets.ts:27-29`, `RecentMoves.tsx:74` | Historical values (debt trajectory, valuation trace, purchase price, other-asset cost, recent moves) converted at today's rate; stocks/crypto use dated rates. |
| K6 | `BankAccounts.tsx:84-98` | Own policy for rateless currencies (fallback to CZK, unknown accounts count 0), unlike every other page. |
| K7 | `commands/cashflow.rs:20,34,84`, `commands/budgeting.rs:33,56` | `report_currency` optional on the wire, defaults to `"CZK"`. |
| K8 | `portfolio_history.rs:105-107`; `refresh_exchange_rates` | Today's live snapshot uses in-memory rates; a later same-day refresh changes the history rate used to re-value that day. |
| K9 | `commands/portfolio.rs:2261-2300` | Crypto history days without an API price are filled with today's price. |

### 3.7 Defaults, product and docs (Low)

- 20 `DEFAULT 'CZK'` currency columns in migration 001 and ~15 service-level `unwrap_or("CZK")` create-path defaults; ~55 frontend `|| 'CZK'` fallbacks. Stock/crypto forms default to USD, other forms to the display currency. Profile default is `'CZK'` in the backend (`services/auth.rs:122,254`); onboarding picks a region default (EUR fallback) and is fine.
- CZK-scale UI defaults: calculators (`AnnuityCalculator.tsx:73-77`, `EstateCalculator.tsx:140-151`), `LoanWhatIfCard.tsx:24`, movement floor `MOVEMENT_FLOOR_BASE = 10_000` CZK (`bank-activity.ts:131`), milestone round marks on CZK values (`projection-milestones.ts:43-68`).
- MCP `exchange_rates_list` exposes `baseCurrency: "CZK"`; tool texts mention the CZK pivot.
- Dead code: `formatPrice`, `BASE_CURRENCY` (tests only), `DEFAULT_BASE_CURRENCY`, `useBankAccounts().metrics`, `utils/budget-progress.ts`.
- Docs that encode CZK as base: ADR 0001, ADR 0011 (CZK cache clause), `docs/architecture/database.md` (base-currency and rate sections), `docs/architecture/overview.md` (CZK fallback), `docs/standards/typescript-frontend.md` (`convertToCzK`), `docs/playbooks/add-asset-domain.md` (new CZK total column per asset class), `add-tauri-command.md`, `add-db-migration.md`.
- Seeded institutions are Czech banks; demo seed is CZK. Not a currency defect, relevant to an international launch.

### 3.8 What is already sound

- Native storage everywhere, with a currency column per amount; CSV exports are native with a currency column; bank and stock imports take the currency from the file or account, no CZK default.
- The report regime (ADR 0011) end to end, including MCP cashflow/budgeting.
- `shared/calculations` (cost basis, realized gains) takes a dated converter and a target currency — the right abstraction.
- Snapshot `*_by_currency` JSON is the canonical native record and the dashboard history chart converts it at each day's rate.
- Native detail pages (bank account, loan, stock monitor) format in the entity's own currency.

## 4. Implicitly-CZK wire contract

| Command | Fields |
|---|---|
| `get_portfolio_metrics` → `PortfolioMetrics` | `totalSavings`, `totalInvestments`, `totalCrypto`, `totalBonds`, `totalRealEstatePersonal`, `totalRealEstateInvestment`, `totalRealEstate`, `totalOtherAssets`, `totalLiabilities`, `totalAssets`, `netWorth` |
| `get_portfolio_history` → `PortfolioMetricsHistory[]` | the eight `total_*` TEXT columns |
| `get_all_investments` / `get_investment*` → `EnrichedStockInvestment` | `currentPrice`, `dividendYield` (per share) |
| `get_all_crypto` → `EnrichedCryptoInvestment` | `currentPrice` |
| `get_stocks_analysis` → `StockInvestmentWithTags` | `currentValue`, `gainLoss`, `gainLossPercent`, `dividendYield` (per position); `currentPrice` 2 dp CZK |
| `get_tag_metrics` → `TagMetrics` | `totalValue`, `totalCost`, `gainLoss`, `estimatedYearlyDividend` + derived % |
| `get_stock_value_history` / `get_crypto_value_history` | `valueCzk` (named) |
| `get_stock_twr` | `twr` (CZK return) |
| `calculate_portfolio_projection` | all timeline and summary amounts (types not specta-registered) |
| `get/save_projection_settings` | `monthlyContribution` (`contributionCurrency` ignored) |
| `refresh_exchange_rates`, `get_exchange_rates*` | CZK-per-unit maps |
| `get_price_status` | `missingCurrencies` (meaning: converted 1:1 to CZK) |

## 5. Persisted CZK data

| Data | Rebuildable from native? | Note |
|---|---|---|
| `stock_value_history.value_czk`, `crypto_value_history.value_czk` | Yes | native price, quantity, currency stored per row; needs FX history for the day |
| `portfolio_metrics_history.total_*` (savings, loans, investments, crypto, bonds, other assets) | Mostly | from `*_by_currency`; not for legacy `{}` rows or synthetic `{"CZK": …}` buckets |
| `total_real_estate_personal` / `_investment` | Partially | split exists only in CZK (N5) |
| `projection_settings.monthly_contribution` | No | already tagged `'CZK'`; can be read as a native CZK amount |
| `exchange_rates`, `exchange_rate_history` | Yes, where the day has an EUR row | `rate_eur_base(c) = rate_czk(c) / rate_czk(EUR)` |

## 6. Rate sources (live check 2026-10-08/09)

- ECB daily feed: 29 currencies + EUR (USD JPY CZK DKK GBP HUF PLN RON SEK CHF ISK NOK TRY AUD BRL CAD CNY HKD IDR ILS INR KRW MXN MYR NZD PHP SGD THB ZAR). BGN absent since 2026-01 (euro adoption).
- frankfurter.dev: the same set, daily granularity over 16 years; unknown symbols are ignored silently (HTTP 200), so one non-ECB currency does not break a backfill.
- Non-ECB currencies: AED, SAR, QAR, BHD, OMR are hard USD pegs; the rest (ARS, CLP, COP, EGP, KWD, RSD, TWD, UAH, VND) float and need another source or user-entered rates.
