# Explicit currencies everywhere, EUR rate reference

The owner approved this design on 2026-10-09 after the multi-currency audit (`docs/audits/2026-10-09-multi-currency-audit.md`, finding IDs below refer to it). Moony is no longer aimed only at the Czech market; CZK must stop being the implicit unit of the portfolio, the API and the rate store. Autonomous implementation is authorized: execute without intermediate approvals, feature branch in place, local merge to `main`, no push.

Decisions taken with the owner: full rebuild in phases; main (profile) currency restricted to currencies with a rate source, with USD pegs and BGN derived; the currency-adjacent standalone bugs are fixed first as small fixes, outside this spec.

## Principles

1. Native amount + currency + date are the only facts. Nothing converted is authoritative.
2. Every monetary value the app returns (IPC, MCP) or displays carries its currency, either per value or as the declared currency of the response.
3. Missing conversion is explicit: the value is `null`/unavailable, the native item stays visible, totals exclude it and declare the missing currencies. Never 1:1, never hardcoded rates, never a silent switch to another currency.
4. A value that MCP also exposes is computed by one backend service shared by the UI command and the MCP tool (net worth, history, investment positions/returns, tags, TWR, projection).
5. The rate reference currency is an implementation detail of the FX service. After phase 3 it is EUR, the source's own base.
6. Same-currency conversion is identity and needs no rate.

## Phase 1 — one FX service and full rate coverage

**Service.** New `services/fx.rs` owns every conversion. A `FxSnapshot` (current quotes) and a `FxHistory` (quotes by UTC day, loaded once per range) expose:

- `convert(amount, from, to) -> Option<f64>` on the current snapshot;
- `convert_on(amount, from, to, day) -> Option<f64>` on history: quotes for `from` and `to` from one common day on or before `day`, at most ten days back (the ADR 0011 rule; with an EUR-based ECB feed every quoted currency shares the day);
- a `FxQuality` collector: missing currencies (sorted, deduplicated) and counts of converted/missing items, serialised into responses.

Rules: codes are normalised (trimmed, uppercased) at the boundary; stored rates that are not finite and positive count as missing (R7); no hardcoded fallback rates (R4); no 1:1 fallback (R1, R2). History lookups search per day for a day that quotes both currencies, not merely the first day with any row (R6). Callers that need a current-rate fallback for an uncovered historical day must ask for it explicitly and mark the result `estimated` (planning keeps its current-snapshot semantics).

`currency.rs` keeps fetching/persisting rates and the in-memory current snapshot but loses `convert_to_czk`, `convert_from_czk`, `convert_between`, `get_exchange_rate`, the hardcoded table and the session-global missing set. `reporting::historical_amount`, `cost_basis::rate_for`, `PlanningFx`, `convert_to_czk_with_rates` and the MCP `MoneyContext` conversion delegate to the FX service. Report behaviour does not change.

**Derived quotes.** At read time the FX service derives AED (3.6725), SAR (3.75), QAR (3.64), BHD (0.376), OMR (0.3845) per USD and BGN (1.95583) per EUR from the same day's USD/EUR quote. They are never stored, so their history follows USD/EUR automatically. A stored live quote for a derived code is ignored.

**Supported currencies.** `FX_SUPPORTED = ECB quotes ∪ derived` (30 + 6 = 36 codes; BGN moves from "ECB" to "derived"). Exposed to the frontend as a shared constant. The main currency must be supported: the profile update and onboarding commands validate it (`AppError::Validation`); the settings picker lists only supported codes; onboarding never proposes an unsupported code (region fallback EUR in Europe, USD elsewhere). An existing profile with an unsupported main currency is not rewritten; the app shows a settings prompt and same-currency values keep working. Entity currencies stay free (any ISO code) and are reported as missing in converted totals when unsupported.

**Coverage.** `fx_backfill` covers every currency the user holds or that prices are quoted in: transaction currencies, `stock_data`/`crypto_prices` quote currencies, bank accounts, bonds, loans, real estate (all currency columns and valuations), other assets, insurance, cashflow items, budget goals and the main currency — minus derived codes (covered through USD/EUR) and codes the source never quotes. Rates are aged per currency: the current snapshot records each currency's quote day, and a currency whose last quote is older than 14 days is reported stale in `get_price_status` instead of being used silently (R5).

## Phase 2 — explicit currency in every monetary response

**Commands.** `report_currency` becomes a required argument of `get_portfolio_metrics`, `get_portfolio_history`, `get_all_investments`, `get_investment`, `get_investment_with_details`, `get_all_crypto`, `get_stock_value_history`, `get_crypto_value_history`, `get_stock_twr`, `get_stocks_analysis`, `get_tag_metrics`, `calculate_portfolio_projection`, and stays required (no `"CZK"` default) on the cashflow and budgeting commands (K7). Each response declares `currency` and a `fxQuality { missingCurrencies, missingCount }`. Converted monetary fields are decimal TEXT at the currency's ISO minor unit (same contract as reports) and nullable when the item cannot be converted. Implicit-CZK names disappear (`valueCzk` → `value` + `currency`; `dividendYield` split into `dividendPerShare` (native) and `yearlyDividend` (report currency)).

**Investment positions.** One service (`services/positions.rs`) values stock and crypto positions for both UI and MCP, replacing the five aggregators (K2) and the frontend holding maths on the Stocks/Crypto pages. Per position: native quantity, native price and currency, value in the report currency (current snapshot), cost basis in the report currency (each transaction converted at its own day), unrealized and realized gain, gain %, yearly dividend; the price resolver is `pricing.rs` for every caller. Gain is FX-correct in the report currency (C1 regression: EUR profile, USD stock, 1000 USD bought at USDCZK=EURCZK=25, today USDCZK 25 / EURCZK 22 → cost 1000 EUR, value 1136.36 EUR, gain 136.36 EUR). TWR is computed on native daily value converted at each day's rate into the report currency (C2). Ticker history returns native `price`, `quantity`, `currency` and `value` in the report currency at each day's rate (C3, C4).

**Portfolio and history.** `get_portfolio_metrics` sums native breakdowns into the report currency with the current snapshot. `get_portfolio_history` converts each row's `*_by_currency` at that row's day; the frontend `use-historical-display-values` conversion is removed. Projection takes current values from the same services and its timeline is in the report currency.

**Projection contribution.** `projection_settings.monthly_contribution` is stored with the currency the user typed (`contribution_currency`), read natively and converted to the report currency in the projection (N1). Existing rows keep `'CZK'` and stay correct as CZK-native amounts.

**MCP.** `portfolio_get_metrics`, `portfolio_get_history`, `stocks_analysis`, `tag_metrics`, `*_value_history`, `investments_list`/`crypto_list` aggregates use the shared services with the main currency; amounts at ISO minor units (P5); missing conversion reported, never relabelled. `exchange_rates_list` returns quotes against an explicit `baseCurrency` (CZK in phase 2, EUR in phase 3) and the list of supported currencies.

**Frontend.**
- `money(amount, currency, opts)` (existing `fmt.money`) is the only formatter. `formatCurrency`, `formatCurrencySigned`, `formatCurrencyShort` (CZK in) and `formatCurrencyRaw`, `formatPrice` are removed; `useCurrency()` keeps `currencyCode` and gains `fx`.
- `fx` is a `FxSnapshot` fetched with React Query (`['fx-snapshot', revision]`), a pure `convert(snapshot, amount, from, to): number | null` and `convertOn(history, …)` in `shared/fx.ts`, implementing the same rules as the backend (shared test vectors). The module-global `EXCHANGE_RATES`, `updateExchangeRates`, `convertToCzK`, `convertFromCzK`, `BASE_CURRENCY` and the hardcoded rates are removed.
- Pages whose numbers come from the backend (Dashboard, Stocks, Crypto, StockDetail, CryptoDetail, StocksAnalysis, Projection, trend cards) format the response currency. Pages that sum native entities (Bank accounts, Bonds, Loans, Insurance, Real estate, Other assets) convert with `fx` into the main currency; unconvertible items render natively with a missing marker and are excluded from totals with the compact warning used by reports. Historical values (debt trajectory, valuation trace, purchase price, other-asset cost, recent moves) convert at their own day (K5).
- Query keys of every converted query include the report currency and the FX revision; a rate refresh bumps the revision and invalidates them (K3, K4). The bank-accounts page loses its own fallback policy (K6).
- Symbols: `narrowSymbol` only for the main currency; other currencies render their ISO code (P6).
- CZK-scale defaults (calculators, what-if step, movement floor, milestone marks) become currency-relative: defaults scale from a EUR-denominated constant through `fx`, or start empty.

## Phase 3 — EUR rate reference and retiring CZK caches

**Rates migration (`010_fx_eur_reference`).** Rewrites `exchange_rates` and `exchange_rate_history` to "units of currency per 1 EUR": for each day `q(c) = rate_czk(EUR) / rate_czk(c)`, adds the CZK quote `q(CZK) = rate_czk(EUR)`, drops the EUR row (`q(EUR) = 1` is implicit), deletes days without an EUR row (none expected). Records `app_config('fx_reference', 'EUR')`. Ingest stores the ECB/frankfurter values as published; the CZK-quote requirement, the forced `symbols=CZK` and the frankfurter "skip day without CZK" rule disappear (R8). The FX service, `reporting` SQL, IPC rate commands and `shared/fx.ts` switch to EUR quotes in the same commit.

**Caches (`011_retire_czk_caches`).**
- `portfolio_metrics_history`: rows whose breakdown for a class is `'{}'` while the class total is non-zero get `{"CZK": total}` once (the value was measured in CZK); add `real_estate_personal_by_currency` (native personal subset, `'{}'` default; legacy rows get `{"CZK": total_real_estate_personal}` when non-zero) so the personal/investment split and the "exclude personal real estate" view survive natively (N5); then drop the eight `total_*` columns.
- `stock_value_history` / `crypto_value_history`: drop `value_czk` (native `price`, `quantity`, `currency` remain and are already `NOT NULL`).
- Snapshot writing stores native breakdowns only; `backfill_currency_breakdowns`, `BREAKDOWN_CONSISTENCY_TOLERANCE` and the synthetic `czk_map` carry for backfilled days are removed (N4); backfilled days carry static classes with their native breakdowns.
- Verified on a copy of a real profile before merge: totals in CZK before and after the migration agree to 0.01 CZK for every row with complete breakdowns.

**Docs.** ADR 0012 "Explicit currencies and EUR rate reference" supersedes the remaining parts of ADR 0001 (decimal TEXT storage stays) and ADR 0011's CZK cache clause. Update `database.md`, `overview.md`, `typescript-frontend.md`, `rust-backend.md`, the asset-domain, command and migration playbooks, `DEPRECATED.md` (removed converters and formatters), AGENTS.md rule 10 wording, README currency section.

## Out of scope

User-entered exchange rates for floating non-ECB currencies; changing default currencies of new entities beyond using the main currency; a second rate provider; tax lots/FIFO; per-position FX-gain vs price-gain split in the UI (the data allows it later).

## Acceptance

- No code path returns or displays a converted amount without its currency; `rg "convert_to_czk|convertToCzK|formatCurrencyRaw|value_czk|valueCzk"` finds nothing outside migrations and DEPRECATED.md.
- An EUR profile holding only EUR assets sees identical values regardless of any CZK rate, including when no CZK quote exists.
- An AED profile sees AED values derived from USD; a KWD entity is listed natively and reported missing in totals.
- C1 regression passes in tags, analysis, Stocks page and MCP with the same numbers.
- Profile currency switch refetches converted queries; a rate refresh updates every page without unlock.
- Rust services tested with in-memory `Connection` (FX rules, derived quotes, common day, missing, positions, TWR, history conversion, migrations 010/011 on fixture data); pure TS tests for `shared/fx.ts` with the backend's test vectors and for formatters. Full gates, binding regeneration without drift, independent review per phase.
