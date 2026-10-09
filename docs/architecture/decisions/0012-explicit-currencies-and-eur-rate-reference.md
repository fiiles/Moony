# 0012. Explicit currencies and the EUR rate reference

Date: 2026-10-09

## Status

Accepted. Supersedes [ADR 0001](0001-czk-base-currency-and-money-as-text.md) except its decimal TEXT storage rule, and the CZK cache clause of [ADR 0011](0011-native-money-and-report-currencies.md); the rest of ADR 0011 (native facts, report currencies, the ten-day common-day rule) stays in force and now covers the whole portfolio.

## Context

The audit of 2026-10-09 (`docs/audits/2026-10-09-multi-currency-audit.md`) found that the report regime of ADR 0011 was sound, while the portfolio regime still assumed CZK. Net worth, positions, tags, TWR, projection and the MCP aggregates summed in CZK at the current rate and returned unlabelled numbers that the frontend converted again. Consequences: a EUR user's gain on a USD stock contained the CZK move; 15 selectable currencies without a rate source were valued 1:1 against CZK and shown with their own symbol; rate history covered only transaction currencies; snapshots persisted CZK copies of native facts. CZK was also the rate reference: a feed without CZK was rejected as a whole, so a currency that leaves the ECB feed (BGN after euro adoption) would freeze every conversion, including for users who hold no CZK.

## Decision

- **Native facts only.** Native amount, currency and date are authoritative; nothing converted is stored as truth. Converted values are computed per response from native data.
- **Every response declares its currency.** Each monetary IPC command and MCP tool takes or reads a report (main) currency and returns it. Converted amounts are decimal TEXT at the currency's ISO 4217 minor unit and `null` when not convertible; the response carries `fxQuality` (`missingCurrencies`, `missingCount`, `estimated`). Missing conversion is never 1:1, never a hardcoded rate and never a silent switch of currency; totals exclude the item and name it.
- **One FX service.** `services/fx.rs` owns every backend conversion. Quotes are reference-agnostic (units per 1 unit of the reference); same-currency conversion is identity and needs no rate; a historical conversion uses one UTC day that quotes both currencies, on or before the requested day, at most ten days back. `shared/fx.ts` implements the same rules for the frontend, pinned by shared test vectors.
- **EUR reference.** Rates are stored as units per 1 EUR, as the ECB publishes them (migration 010, `app_config.fx_reference`); CZK is an ordinary quote. Pegged currencies are derived at read time and never stored: AED, SAR, QAR, BHD, OMR from USD and BGN from EUR (`shared/fx-supported.json`).
- **Supported main currency.** The main currency must be one of the 36 supported codes (source quotes plus derived); profile creation and update (onboarding included) reject others with `validation.currencyUnsupported`. Entity currencies stay free: an unsupported one is listed natively and reported as missing in converted totals.
- **No CZK caches.** Migration 011 drops the `total_*` columns of `portfolio_metrics_history` and `value_czk` of both ticker value histories. Native `*_by_currency` breakdowns (plus `real_estate_personal_by_currency`) and per-day native quantity, price and currency are the stored record.
- **One service per value.** Positions (`services/positions.rs`), net worth and its history (`services/net_worth.rs`), ticker history, TWR and projection serve both the UI commands and the MCP tools. Returns are FX-correct in the report currency: cost converts at each transaction's day, value at the current snapshot.

## Consequences

- No wire contract defaults to CZK any more; the frontend formats the response currency and converts native entities only through `useCurrency().convert` (`null` without a rate). Query keys of converted data include the currency and `fxRevision`.
- A profile whose main currency is unsupported keeps working for same-currency values and is prompted in settings; its data is not rewritten.
- Rebuilding a snapshot day needs only native data and that day's rates. A legacy snapshot row without a breakdown was given `{"CZK": total}` once by migration 011; that is the one place an old CZK figure survives, as a native CZK amount.
- Storage column defaults (`DEFAULT 'CZK'`) and the profile fallback `DEFAULT_MAIN_CURRENCY` remain for records created without a currency; they are native-currency labels, not an aggregation base.
- MCP clients break knowingly: `investments_list` and `crypto_list` return objects instead of arrays, ticker history rows lose `id`, aggregates report `missingCurrencies` / `fxEstimated`, and `exchange_rates_list` quotes against an explicit `baseCurrency` (EUR).
- Currencies that float and have no source (for example KWD, ARS, VND) stay unsupported as a main currency until a second rate provider or user-entered rates exist; both are out of scope here.
- Migrations 010 and 011 rewrite stored data in place; the opener's pre-migration database copy is the rollback.
