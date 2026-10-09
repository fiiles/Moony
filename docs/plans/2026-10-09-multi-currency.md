# Explicit Currencies and EUR Rate Reference Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove CZK as the implicit unit of Moony's portfolio, API and rate store: every monetary value carries its currency, conversion happens in one backend FX service with explicit missing-rate handling, and the rate store uses EUR as its reference.

**Architecture:** A reference-agnostic FX service (`services/fx.rs`) owns quotes, derived pegs, day lookup and quality tracking; every Rust conversion delegates to it. Commands that compute values also exposed through MCP (net worth, history, positions, tags, TWR, projection) take a required `report_currency` and return decimal TEXT in that currency plus a conversion-quality object; the frontend only formats them, and converts simple native page sums with a pure `shared/fx.ts` over an explicit snapshot fetched from the backend. Phase 3 migrates the rate tables to EUR quotes and retires the CZK cache columns.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher, specta), React 19 + TypeScript, TanStack Query, Vitest.

Spec: `docs/specs/2026-10-09-multi-currency-design.md`. Audit (finding IDs C1…K9): `docs/audits/2026-10-09-multi-currency-audit.md`.

## Global Constraints

- Native amount + currency + date are the only facts; nothing converted is authoritative; never rewrite stored native values.
- Missing conversion is explicit: value `null`, native item stays visible, totals exclude it and declare `missingCurrencies`. Never 1:1, never hardcoded rates, never a silent switch to another currency.
- Same-currency conversion is identity and needs no rate (also for codes without any quote).
- Historical conversion: quotes of both currencies from one common UTC day on or before the requested day, at most ten days back.
- Derived quotes (read time only, never stored): AED 3.6725, SAR 3.75, QAR 3.64, BHD 0.376, OMR 0.3845 per USD; BGN 1.95583 per EUR.
- Main (profile) currency must be supported (source quotes ∪ derived = 36 codes). Entity currencies stay free.
- Converted monetary wire fields are decimal TEXT rounded at the currency's ISO 4217 minor unit (`reporting::money_in_currency` / `isoMinorUnits`), nullable when not convertible.
- Every user-facing string in both `src/i18n/locales/en/` and `cs/` (AGENTS rule 8).
- Rust: thin commands, logic in `services/` taking `&Connection`; `AppError`, no `Result<_, String>`; no `.unwrap()` outside tests; per-field `#[serde(rename = "camelCase")]`; register new wire types in `bindings.rs`, regenerate `shared/generated-types.ts`; `shared/schema.ts` is the wire contract (7-step procedure in `docs/standards/type-contract.md`).
- `invoke()` only in `src/lib/tauri-api.ts`. Mutations follow AGENTS rule 7.
- Migrations append-only in `src-tauri/src/db/migrations.rs` (next numbers 010, 011), regenerate `schema.snapshot`.
- Tests: Rust services with `Connection::open_in_memory()` and hand-written minimal schema (never the `Database` struct); pure TS utilities with Vitest; no component/hook tests.
- Commits: Conventional Commits, one logical change each, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Verify `git branch --show-current` is `feat/multi-currency` before every commit (other sessions may share the checkout). Never push.
- Gates before each commit that touches code: `npm run lint && npm run typecheck && npm test` and `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test` (scope to touched side when the other is untouched; full gates at phase end).
- Never open or modify the user's real database. Phase 3 verification uses a copy supplied in the final task only.

## File Map

**Create**
- `shared/fx-supported.json` — single list of source-quoted codes and derived pegs, read by Rust (`include_str!`) and TS.
- `shared/fx-test-vectors.json` — conversion cases asserted by both Rust and Vitest.
- `src-tauri/src/services/fx.rs` — quotes, derived pegs, normalisation, current snapshot store, day history, quality collector.
- `src-tauri/src/models/fx.rs` — wire types `ConversionQuality`, `FxSnapshot`, `FxDayQuotes`.
- `src-tauri/src/services/positions.rs` — stock/crypto position valuation in a report currency (UI + MCP).
- `src-tauri/src/services/net_worth.rs` — portfolio metrics and history in a report currency (UI + MCP).
- `shared/fx.ts` (+ `shared/fx.test.ts`) — pure frontend conversion over an explicit snapshot/history.
- `src/hooks/use-fx.ts` — fx snapshot query + revision.
- `docs/architecture/decisions/0012-explicit-currencies-and-eur-rate-reference.md`.

**Modify (main ones)**
- `src-tauri/src/services/currency.rs` (fetch/persist only), `reporting.rs`, `cost_basis.rs`, `cashflow_planning.rs`, `fx_backfill.rs`, `portfolio_history.rs`, `ticker_history.rs`, `pricing.rs`, `investments.rs` (TWR), `services/mcp/{money,portfolio,analytics,investments,crypto,exchange_rates,mod}.rs`, `services/auth.rs`, `services/onboarding.rs`.
- `src-tauri/src/commands/{portfolio,investments,crypto,stock_tags,projection,cashflow,budgeting}.rs`, `models/*`, `bindings.rs`, `lib.rs`, `db/migrations.rs`, `db/schema.snapshot`.
- `shared/currencies.ts`, `shared/schema.ts`, `shared/generated-types.ts`, `src/lib/{currency.ts,currency-provider.tsx,format.ts,tauri-api.ts}`, pages and components listed per task.

---

# Phase 0 — standalone fixes (small-fix tier)

Each task here is an independent small fix: write a regression test where the logic is a pure function, fix, run the gates, commit. Components have no tests by policy; verify by typecheck and reading the diff.

### Task 1: Decimal manual price and dividend (P1)

**Files:**
- Modify: `src/components/stocks/ManualPriceModal.tsx` (prefill ~line 65, input ~128-141)
- Modify: `src/components/stocks/ManualDividendModal.tsx` (prefill ~line 65, input ~128-140)

- [ ] **Step 1:** In both modals replace the integer handling: prefill with `priceToShow ? String(Number(priceToShow.toFixed(6))) : ''` (six decimals, the stored precision — same as `BuyInvestmentModal`), input `step="any"`, and on blur parse with the existing locale-aware number parsing used in `BuyInvestmentModal` without `Math.round`. Keep validation `> 0` for price, `>= 0` for dividend.
- [ ] **Step 2:** `npm run typecheck && npm run lint`. Expected: pass.
- [ ] **Step 3:** Commit `fix: keep decimals in manual stock price and dividend`.

### Task 2: Full-precision crypto prices (P2, P4)

**Files:**
- Modify: `src-tauri/src/services/price_api.rs:~542` (and any sibling `format!("{:.2}", …)` that persists a crypto price; grep `{:.2}` in the file)
- Modify: `src/components/crypto/{BuyCryptoModal,SellCryptoModal,AddCryptoModal,UpdateCryptoPriceModal}.tsx` (`toFixed(2)` on prefill/blur)
- Test: `src-tauri/src/services/price_api.rs` tests module

- [ ] **Step 1: Failing test.** Extract the formatting into `fn price_text(price: f64) -> String` (shortest round-trip, `price.to_string()`, trailing `.0` trimmed) next to the writer and add:

```rust
#[test]
fn crypto_price_keeps_sub_cent_precision() {
    assert_eq!(price_text(0.00001234), "0.00001234");
    assert_eq!(price_text(64250.5), "64250.5");
    assert_eq!(price_text(2.0), "2");
}
```
- [ ] **Step 2:** `cd src-tauri && cargo test crypto_price_keeps_sub_cent_precision` → FAIL (function missing).
- [ ] **Step 3:** Implement `price_text` and use it where the crypto USD price is persisted.
- [ ] **Step 4:** In the four crypto modals replace `toFixed(2)` with `Number(x.toPrecision(12))` on prefill and keep the raw typed value on blur (no truncation); inputs `step="any"`.
- [ ] **Step 5:** Run the test (PASS), `cargo clippy -- -D warnings`, `npm run typecheck`. Commit `fix: store and edit crypto prices at full precision`.

### Task 3: Native currency in stock modals opened from the list (P3)

**Files:**
- Modify: `src/pages/Stocks.tsx:182-197` (holding mapping), `src/components/stocks/BuyInvestmentModal.tsx:64-70`, `SellInvestmentModal.tsx:62-64`, `ManualPriceModal.tsx:62-66`
- Check: `src/pages/StockDetail.tsx` (passes native values; must keep working)

- [ ] **Step 1:** In the three modals read the transaction/price currency from `investment.originalAvgCostCurrency ?? investment.avgCostCurrency` (transactions) and `investment.originalCurrency ?? investment.currency` (manual price), and the native price from `originalPrice`/`originalCurrentPrice`. Remove the `as 'USD' | 'EUR' | 'CZK'` casts (type the value as `string`; the form schema accepts any `CurrencyCode`).
- [ ] **Step 2:** Confirm `HoldingData` (where it is declared, likely `src/components/stocks/types.ts` or `Stocks.tsx`) has `originalAvgCostCurrency` and `originalCurrency`; StockDetail's object passes the same keys with native values (add them there if missing).
- [ ] **Step 3:** `npm run typecheck && npm run lint`. Commit `fix: use the stock's own currency in buy, sell and manual price from the list`.

### Task 4: Display-currency inputs and outputs on asset pages (U1–U4)

**Files:**
- Modify: `src/pages/RealEstate.tsx:377-380`, `src/pages/OtherAssets.tsx:184` — pass `convert(totalCzk, 'CZK', currencyCode)` to `PortfolioTrendCard.currentValue`.
- Modify: `src/components/stocks/InvestmentsSummary.tsx:82`, `src/pages/StockDetail.tsx:206-208,586` — dividend amounts are CZK: format with `formatCurrency` and compute yield % from `convert(yearlyDividendCzk,'CZK',currencyCode) / currentValue`.
- Modify: `src/components/bank-accounts/BankAccountZoneManager.tsx` — accept `currency: string` prop (passed by its parent from the account) and format thresholds with `fmt.money(value, currency)`; overlap messages the same.
- Modify: `src/pages/AnnuityCalculator.tsx:130-137` — `loadLoan` converts `loan.principal` with `convert(Number(loan.principal), loan.currency, currencyCode)`.

- [ ] **Step 1:** Apply the four edits above.
- [ ] **Step 2:** `npm run typecheck && npm run lint`. Commit `fix: show asset trend, dividends, interest zones and loaded loans in the right currency`.

---

# Phase 1 — one FX service and full rate coverage

### Task 5: Supported-currency table and FX test vectors

**Files:**
- Create: `shared/fx-supported.json`, `shared/fx-test-vectors.json`
- Modify: `shared/currencies.ts` (drop BGN from `ECB_CURRENCY_CODES`, derive lists from the JSON)
- Test: `shared/currencies.test.ts` (create if absent)

**Interfaces — Produces:**
- `shared/fx-supported.json`:
```json
{
  "source": ["AUD","BRL","CAD","CHF","CNY","CZK","DKK","EUR","GBP","HKD","HUF","IDR","ILS","INR","ISK","JPY","KRW","MXN","MYR","NOK","NZD","PHP","PLN","RON","SEK","SGD","THB","TRY","USD","ZAR"],
  "derived": {
    "AED": { "anchor": "USD", "unitsPerAnchor": 3.6725 },
    "SAR": { "anchor": "USD", "unitsPerAnchor": 3.75 },
    "QAR": { "anchor": "USD", "unitsPerAnchor": 3.64 },
    "BHD": { "anchor": "USD", "unitsPerAnchor": 0.376 },
    "OMR": { "anchor": "USD", "unitsPerAnchor": 0.3845 },
    "BGN": { "anchor": "EUR", "unitsPerAnchor": 1.95583 }
  }
}
```
- `shared/fx-test-vectors.json` — quotes are "units of currency per 1 reference"; `expected: null` means not convertible:
```json
{
  "quotes": { "reference": "EUR", "units": { "CZK": 25.0, "USD": 1.1, "JPY": 160.0 } },
  "cases": [
    { "amount": 100, "from": "EUR", "to": "CZK", "expected": 2500 },
    { "amount": 2500, "from": "CZK", "to": "EUR", "expected": 100 },
    { "amount": 110, "from": "USD", "to": "CZK", "expected": 2500 },
    { "amount": 1, "from": "USD", "to": "AED", "expected": 3.6725 },
    { "amount": 3.6725, "from": "AED", "to": "SAR", "expected": 3.75 },
    { "amount": 1, "from": "EUR", "to": "BGN", "expected": 1.95583 },
    { "amount": 100, "from": "KWD", "to": "EUR", "expected": null },
    { "amount": 100, "from": "KWD", "to": "KWD", "expected": 100 },
    { "amount": 100, "from": " usd ", "to": "usd", "expected": 100 },
    { "amount": 100, "from": "EUR", "to": "PLN", "expected": null }
  ]
}
```
- `shared/currencies.ts` exports `FX_SOURCE_CODES: readonly string[]`, `FX_DERIVED_CODES: readonly string[]`, `FX_SUPPORTED_CODES: readonly CurrencyCode[]` (source ∪ derived, sorted, filtered to `CURRENCY_CODES`), `isFxSupported(code): boolean`. `ECB_CURRENCY_CODES` becomes `FX_SOURCE_CODES` filtered to `CURRENCY_CODES` (keep the export name for now; Task 21 removes legacy names).

- [ ] **Step 1: Failing test** (`shared/currencies.test.ts`):
```ts
import { describe, expect, it } from 'vitest';
import { FX_SUPPORTED_CODES, isFxSupported, ECB_CURRENCY_CODES } from './currencies';

describe('fx supported currencies', () => {
  it('has 36 codes: 30 quoted + 6 derived', () => {
    expect(FX_SUPPORTED_CODES).toHaveLength(36);
  });
  it('treats pegs and BGN as supported, floating non-ECB codes as unsupported', () => {
    for (const c of ['AED', 'SAR', 'QAR', 'BHD', 'OMR', 'BGN', 'EUR', 'CZK']) expect(isFxSupported(c)).toBe(true);
    for (const c of ['KWD', 'UAH', 'VND', 'ARS', 'TWD']) expect(isFxSupported(c)).toBe(false);
  });
  it('no longer lists BGN as ECB-quoted', () => {
    expect(ECB_CURRENCY_CODES).not.toContain('BGN');
  });
});
```
- [ ] **Step 2:** `npx vitest run shared/currencies.test.ts` → FAIL.
- [ ] **Step 3:** Create both JSON files; implement the exports in `shared/currencies.ts` by importing `./fx-supported.json` (same import style as `iso4217-minor-units.json`); update the doc comment of `CURRENCY_CODES` ("30 currencies with an ECB rate, 6 derived from a fixed peg, the rest without automatic rates").
- [ ] **Step 4:** Run test → PASS; `npm run typecheck`.
- [ ] **Step 5:** Commit `feat: shared table of FX-supported currencies and conversion test vectors`.

### Task 6: FX service core (quotes, pegs, quality)

**Files:**
- Create: `src-tauri/src/services/fx.rs`
- Modify: `src-tauri/src/services/mod.rs` (`pub mod fx;`)

**Interfaces — Produces (exact):**
```rust
pub const DAY: i64 = 86_400;
pub const MAX_LOOKBACK_DAYS: i64 = 10;
pub fn normalize(code: &str) -> Option<String>;          // trim + uppercase, exactly 3 ASCII letters
pub fn source_codes() -> &'static [String];              // from fx-supported.json
pub fn derived(code: &str) -> Option<(&'static str, f64)>; // (anchor, units per anchor)
pub fn is_supported(code: &str) -> bool;                 // source ∪ derived
pub fn supported_codes() -> Vec<String>;                 // sorted

#[derive(Debug, Clone, PartialEq)]
pub struct Quotes { /* reference: String, units: HashMap<String, f64> */ }
impl Quotes {
    pub fn new(reference: &str) -> Self;
    pub fn reference(&self) -> &str;
    pub fn insert(&mut self, code: &str, units_per_reference: f64); // ignores invalid codes, non-finite/<=0 values, derived codes, the reference
    pub fn units(&self, code: &str) -> Option<f64>;          // reference -> 1.0; derived -> anchor * peg
    pub fn convert(&self, amount: f64, from: &str, to: &str) -> Option<f64>;
    pub fn is_empty(&self) -> bool;                          // no quote besides the reference
    pub fn codes(&self) -> Vec<String>;                      // stored + derived resolvable codes, sorted, incl. reference
    pub fn from_czk_per_unit(rates: &HashMap<String, f64>) -> Self; // phase-1 adapter (reference CZK, units = 1/rate)
    pub fn to_czk_per_unit(&self) -> HashMap<String, f64>;   // all resolvable codes incl. derived; None if CZK unresolvable -> empty map
}

#[derive(Debug, Clone, Default)]
pub struct Quality { /* missing: BTreeSet<String>, missing_count: usize */ }
impl Quality {
    pub fn track(&mut self, from: &str, converted: Option<f64>) -> Option<f64>; // records `from` when None
    pub fn merge(&mut self, other: &Quality);
    pub fn missing_currencies(&self) -> Vec<String>;
    pub fn missing_count(&self) -> usize;
    pub fn is_complete(&self) -> bool;
}
```
`convert`: normalise both codes (invalid → `None` unless the raw strings are equal after trim/uppercase); same code → `Some(amount)` when `amount.is_finite()`; else `amount * units(to) / units(from)`, `None` when either is missing or the result is not finite.

- [ ] **Step 1: Failing tests** (in `fx.rs` `#[cfg(test)] mod tests`):
```rust
use super::*;

fn eur_quotes() -> Quotes {
    let mut q = Quotes::new("EUR");
    q.insert("CZK", 25.0);
    q.insert("USD", 1.1);
    q.insert("JPY", 160.0);
    q
}

#[test]
fn shared_vectors_hold() {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../../../shared/fx-test-vectors.json")).expect("json");
    let mut q = Quotes::new(raw["quotes"]["reference"].as_str().expect("ref"));
    for (code, units) in raw["quotes"]["units"].as_object().expect("units") {
        q.insert(code, units.as_f64().expect("f64"));
    }
    for case in raw["cases"].as_array().expect("cases") {
        let got = q.convert(
            case["amount"].as_f64().expect("amount"),
            case["from"].as_str().expect("from"),
            case["to"].as_str().expect("to"),
        );
        match case["expected"].as_f64() {
            Some(want) => {
                let got = got.unwrap_or_else(|| panic!("case {case} returned None"));
                assert!((got - want).abs() < 1e-9, "case {case}: {got}");
            }
            None => assert_eq!(got, None, "case {case}"),
        }
    }
}

#[test]
fn invalid_stored_quotes_are_ignored() {
    let mut q = Quotes::new("EUR");
    q.insert("USD", 0.0);
    q.insert("GBP", f64::NAN);
    q.insert("AED", 4.0); // derived codes are never stored
    q.insert("usd!", 1.0);
    assert_eq!(q.units("USD"), None);
    assert_eq!(q.units("GBP"), None);
    assert_eq!(q.units("AED"), None); // no USD quote -> no AED
    assert!(q.is_empty());
}

#[test]
fn czk_adapter_round_trips() {
    let czk_per_unit = HashMap::from([("EUR".to_string(), 25.0), ("USD".to_string(), 22.727272727272727)]);
    let q = Quotes::from_czk_per_unit(&czk_per_unit);
    assert_eq!(q.reference(), "CZK");
    assert!((q.convert(1.0, "EUR", "USD").expect("conv") - 1.1).abs() < 1e-9);
    let back = q.to_czk_per_unit();
    assert!((back["EUR"] - 25.0).abs() < 1e-9);
    assert!((back["AED"] - 22.727272727272727 / 3.6725).abs() < 1e-9);
    assert_eq!(back["CZK"], 1.0);
}

#[test]
fn quality_tracks_missing_once_per_currency() {
    let q = eur_quotes();
    let mut quality = Quality::default();
    assert_eq!(quality.track("KWD", q.convert(1.0, "KWD", "EUR")), None);
    assert_eq!(quality.track("KWD", q.convert(2.0, "KWD", "EUR")), None);
    assert!(quality.track("USD", q.convert(1.1, "USD", "EUR")).is_some());
    assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    assert_eq!(quality.missing_count(), 2);
    assert!(!quality.is_complete());
}

#[test]
fn supported_set_matches_shared_table() {
    assert_eq!(supported_codes().len(), 36);
    assert!(is_supported("aed"));
    assert!(!is_supported("KWD"));
    assert_eq!(derived("BGN"), Some(("EUR", 1.95583)));
}
```
- [ ] **Step 2:** `cd src-tauri && cargo test services::fx` → FAIL (module missing).
- [ ] **Step 3: Implement.** Parse `include_str!("../../../shared/fx-supported.json")` once with `std::sync::OnceLock` into `(Vec<String>, HashMap<String,(String,f64)>)`; the parse uses `serde_json` and on a malformed embedded file panics only inside `OnceLock::get_or_init` with `.expect("fx-supported.json is embedded and valid")` (build-time asset, covered by the test). `Quotes.units`: `if code == reference {Some(1.0)} else if let Some((anchor, peg)) = derived(code) { self.units(anchor).map(|a| a * peg) } else { self.units_map.get(code).copied() }`. Module doc: "One conversion policy for the whole backend (ADR 0011, ADR 0012). Quotes are units of a currency per 1 unit of the reference currency; the reference is an implementation detail."
- [ ] **Step 4:** `cargo test services::fx` → PASS; `cargo clippy -- -D warnings`.
- [ ] **Step 5:** Commit `feat: FX service core with derived pegs and conversion quality`.

### Task 7: FX history with the common-day rule; reports delegate to it

**Files:**
- Modify: `src-tauri/src/services/fx.rs` (add `History`)
- Modify: `src-tauri/src/services/reporting.rs:73-105` (`historical_amount`, `historical_czk`)

**Interfaces — Produces:**
```rust
pub struct History { /* days: BTreeMap<i64, Quotes> */ }
impl History {
    /// Loads `exchange_rate_history` for [start_day - MAX_LOOKBACK_DAYS*DAY, end_day].
    /// Phase 1 storage: rows are CZK per unit; each day becomes Quotes::new("CZK") with
    /// units = 1/rate for valid rows (reference CZK implicit 1).
    pub fn load(conn: &Connection, start_day: i64, end_day: i64) -> Result<History>;
    pub fn from_days(days: BTreeMap<i64, Quotes>) -> History; // tests and adapters
    /// Walks back day by day from floor(day) for at most MAX_LOOKBACK_DAYS and uses the first
    /// day on which BOTH currencies resolve. Same currency -> identity without lookup.
    pub fn convert_on(&self, amount: f64, from: &str, to: &str, day: i64) -> Option<f64>;
    pub fn quotes_on(&self, day: i64) -> Option<&Quotes>; // exact stored day
    pub fn days(&self) -> impl Iterator<Item = (&i64, &Quotes)>;
}
```
`reporting::historical_amount(conn, amount, from, target, day) -> Result<Option<f64>>` keeps its signature and error behaviour (`normalize_currency` errors on invalid codes), and becomes: same code → identity; else `History::load(conn, day, day)?.convert_on(amount, &from, &target, day)`.

- [ ] **Step 1: Failing tests** (in `fx.rs` tests, with an in-memory DB):
```rust
fn history_db(rows: &[(i64, &str, f64)]) -> Connection {
    let conn = Connection::open_in_memory().expect("db");
    conn.execute_batch(
        "CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));",
    ).expect("schema");
    for (date, currency, rate) in rows {
        conn.execute("INSERT INTO exchange_rate_history VALUES (?1, ?2, ?3)", rusqlite::params![date, currency, rate]).expect("row");
    }
    conn
}

#[test]
fn convert_on_uses_a_day_that_quotes_both_currencies() {
    let d = 20_000 * DAY;
    // day d: only EUR; day d-1: EUR and USD -> USD->EUR must use d-1 for both
    let conn = history_db(&[(d, "EUR", 24.0), (d - DAY, "EUR", 25.0), (d - DAY, "USD", 20.0)]);
    let h = History::load(&conn, d, d).expect("load");
    let got = h.convert_on(100.0, "USD", "EUR", d + 3600).expect("converted");
    assert!((got - 80.0).abs() < 1e-9, "{got}");
}

#[test]
fn convert_on_gives_up_after_ten_days() {
    let d = 20_000 * DAY;
    let conn = history_db(&[(d - 11 * DAY, "EUR", 25.0)]);
    let h = History::load(&conn, d, d).expect("load");
    assert_eq!(h.convert_on(1.0, "EUR", "CZK", d), None);
    assert_eq!(h.convert_on(1.0, "EUR", "CZK", d - 10 * DAY), Some(25.0));
}

#[test]
fn convert_on_derives_pegs_and_skips_bad_rows() {
    let d = 20_000 * DAY;
    let conn = history_db(&[(d, "USD", 0.0), (d - DAY, "USD", 20.0)]);
    let h = History::load(&conn, d, d).expect("load");
    let aed = h.convert_on(3.6725, "AED", "CZK", d).expect("peg");
    assert!((aed - 20.0).abs() < 1e-9);
    assert_eq!(h.convert_on(5.0, "KWD", "KWD", d), Some(5.0));
}
```
- [ ] **Step 2:** `cargo test services::fx` → FAIL.
- [ ] **Step 3:** Implement `History`; switch `reporting::historical_amount`/`historical_czk` to it (delete the three SQL branches). Keep `normalize_currency` in `reporting.rs` delegating to `fx::normalize` with its existing `AppError::Validation` message.
- [ ] **Step 4:** `cargo test` (whole crate: all reporting, budgeting, cashflow tests must still pass unchanged). If a report test fails, the FX rule changed semantics — fix the implementation, never the report test.
- [ ] **Step 5:** Commit `refactor: reports convert through the FX history service`.

### Task 8: Current snapshot store; remove hardcoded rates and 1:1 fallbacks

**Files:**
- Modify: `src-tauri/src/services/currency.rs` (in-memory store, `load_rates_from_db`, `fetch_ecb_rates`, `get_all_rates`, `convert_*`, `get_rates_for_date*`, `resolve_rates_for_day_from_range`, missing set)
- Modify: `src-tauri/src/services/cost_basis.rs:72-96` (`rate_for`), `src-tauri/src/services/cashflow_planning.rs:1324-1370` (`PlanningFx`), `src-tauri/src/services/mcp/money.rs:60-150`
- Modify: `src-tauri/src/commands/portfolio.rs` (`get_exchange_rates`, `get_exchange_rates_for_date`, `get_exchange_rates_for_date_range`, `get_price_status`)

**Interfaces:**
- Consumes: `fx::Quotes`, `fx::History`, `fx::Quality`.
- Produces (in `fx.rs`): `pub fn current() -> Quotes` (clone of the in-memory snapshot), `pub fn set_current(q: Quotes)`, `pub fn missing_now() -> Vec<String>` (codes that failed a current conversion since the last `set_current`), `fn record_missing(code)`.
- Phase-1 legacy shims kept in `currency.rs` until Task 21 deletes them: `convert_to_czk(amount, currency) -> f64` = `fx::current().convert(amount, currency, "CZK")`, on `None` → `fx::record_missing(currency)` and return `0.0` (excluded from the CZK total, flagged); `get_all_rates()` = `fx::current().to_czk_per_unit()`; `get_rates_for_date(conn, date)` = that day's (walk-back) quotes as CZK-per-unit including derived, or an empty map when no day within 10 days (never current rates); `convert_to_czk_with_rates` on a missing code → record + `0.0`. Delete `convert_from_czk`, `convert_between`, `get_exchange_rate` and their tests only after replacing their callers (MCP money uses `Quotes::convert`).

Rules:
- No hardcoded rates: the store starts empty. `load_rates_from_db` validates each row (finite, > 0) and calls `fx::set_current(Quotes::from_czk_per_unit(&rows))`. `fetch_ecb_rates` sets current from the parsed feed.
- `cost_basis::rate_for` → use `History::convert_on(amount, currency, target, day)`; when `None`, fall back to `fx::current().convert` and mark the position `estimated` (add `pub fx_estimated: bool` to `PositionCostBasis`), when that is also `None` record the currency missing in a `Quality` returned alongside (`cost_basis_for_all` returns `(HashMap<String, PositionCostBasis>, Quality)`) and leave the transaction out of the CZK/target cost (never 1.0). Update callers to merge the quality into their missing list (`get_price_status` reads `fx::missing_now()`; push cost-basis misses via `fx::record_missing`).
- `PlanningFx` uses `fx::current()`; its existing missing/estimated semantics stay (planning tests must pass unchanged). A rent or property-cost amount whose currency is NULL/absent (`cashflow_planning.rs:~85, ~658, ~1149`) is reported as missing conversion (currency `"?"` in the missing list), never valued as CZK (N2) — add a planning test for a property with rent and `monthly_rent_currency = NULL`.
- `MoneyContext` (MCP) holds `Quotes` + `History` and converts with them; `czk_to_main` returns `Option<f64>` and callers emit `null` + list the currency in a `missingCurrencies` output field instead of relabelling (H, audit §3.3).
- `get_exchange_rates*` commands return `to_czk_per_unit()` maps (now including derived codes). `get_price_status.missingCurrencies` reads `fx::missing_now()`.

- [ ] **Step 1: Failing tests** (in `currency.rs` tests; replace the old fallback tests `test_convert_to_czk_unknown_currency_fallback` and `test_unknown_currency_conversion_still_falls_back_to_1_to_1`):
```rust
#[test]
fn unknown_currency_is_excluded_and_reported_not_one_to_one() {
    crate::services::fx::set_current(crate::services::fx::Quotes::from_czk_per_unit(
        &HashMap::from([("EUR".to_string(), 25.0)]),
    ));
    assert_eq!(convert_to_czk(100.0, "KWD"), 0.0);
    assert!(crate::services::fx::missing_now().contains(&"KWD".to_string()));
    assert_eq!(convert_to_czk(2.0, "EUR"), 50.0);
}

#[test]
fn store_starts_without_invented_rates() {
    crate::services::fx::set_current(crate::services::fx::Quotes::new("CZK"));
    assert_eq!(convert_to_czk(1.0, "EUR"), 0.0);
    assert!(get_all_rates().get("EUR").is_none());
}

#[test]
fn rates_loaded_from_db_are_validated() {
    let conn = Connection::open_in_memory().expect("db");
    conn.execute_batch("CREATE TABLE exchange_rates (currency TEXT PRIMARY KEY, rate REAL NOT NULL, fetched_at INTEGER NOT NULL);
        INSERT INTO exchange_rates VALUES ('EUR', 25.0, 1), ('USD', 0.0, 1), ('GBP', -3.0, 1);").expect("schema");
    load_rates_from_db(&conn).expect("load");
    let rates = get_all_rates();
    assert_eq!(rates.get("EUR"), Some(&25.0));
    assert!(rates.get("USD").is_none() && rates.get("GBP").is_none());
}
```
In `cost_basis.rs` tests add:
```rust
#[test]
fn missing_rate_is_reported_not_booked_at_one() {
    let txs = vec![TxRow { tx_type: "buy".into(), quantity: 1.0, price_per_unit: 100.0, currency: "KWD".into(), transaction_date: 20_000 * 86_400 }];
    let history = crate::services::fx::History::from_days(Default::default());
    let (pos, quality) = position_cost_basis(&txs, &history, &crate::services::fx::Quotes::new("CZK"), "CZK");
    let pos = pos.expect("position");
    assert_eq!(pos.cost_basis_target, 0.0);
    assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
}
```
(Signature becomes `position_cost_basis(txs, &History, &Quotes, target: &str) -> (Option<PositionCostBasis>, Quality)` with fields `quantity`, `cost_basis_target`, `cost_basis_native`, `primary_currency`, `fx_estimated`; the CZK callers pass `"CZK"` in phase 1. Rename `cost_basis_czk` → `cost_basis_target` at all call sites.)
Tests touching the global store must not run in parallel with each other: guard them with a `static FX_TEST_LOCK: std::sync::Mutex<()>` in `fx.rs` (`pub(crate) fn test_lock()`), locked at the top of every test that calls `set_current`.
- [ ] **Step 2:** `cargo test` → the new tests FAIL.
- [ ] **Step 3:** Implement as specified; update every caller listed by `grep -rn "convert_from_czk\|convert_between\|get_exchange_rate(\|cost_basis_czk\|rate_for(" src-tauri/src`.
- [ ] **Step 4:** `cargo test && cargo clippy -- -D warnings` → PASS. Existing tests that asserted the hardcoded EUR=25 default or 1:1 fallback are updated to set the store explicitly (they test behaviour that the spec removes; note each in the commit body).
- [ ] **Step 5:** Commit `fix: no invented or 1:1 exchange rates; missing conversions are reported`.

### Task 9: FX coverage for every held currency and per-currency staleness

**Files:**
- Modify: `src-tauri/src/services/fx_backfill.rs:36-140` (`tx_currencies` → `held_currencies`, `missing_fx_ranges`)
- Modify: `src-tauri/src/commands/portfolio.rs` (`get_price_status`), `src-tauri/src/models/*` (the `PriceStatus` struct), `shared/schema.ts` (`PriceStatus`), `src/components/shell/DataStatus.tsx`, locales `en/common.json` + `cs/common.json` (or wherever DataStatus strings live — grep `noRateHint`)

**Interfaces — Produces:**
- `pub fn held_currencies(conn: &Connection) -> Result<Vec<String>>`: distinct normalised codes from `investment_transactions`, `crypto_transactions`, `bank_transactions`, `bank_accounts`, `stock_data.currency`, `crypto_prices.currency` (if the table exists; grep migrations for the crypto price table name), `bonds`, `loans`, `real_estate` (`purchase_price_currency`, `market_price_currency`, `monthly_rent_currency`), `real_estate_valuations`, `other_assets`, `other_asset_valuations`, `insurance_policies` (both currency columns), `cashflow_items`, `budget_goal_versions`, `user_profile.currency`; minus the reference currency, minus derived codes (their anchor USD/EUR is added instead), minus codes not in `fx::source_codes()`.
- `earliest_day(conn, currency)` considers only tables with a date (transactions, valuations, `real_estate.purchase_date`); currencies with no dated fact need only the tail (from the first stored history day).
- `PriceStatus.staleCurrencies: Vec<String>` (`#[serde(rename = "staleCurrencies")]`): held currencies whose latest `exchange_rate_history` day is more than 14 days before today. Registered in bindings; `shared/schema.ts` updated.
- DataStatus shows stale currencies with the existing missing-rate warning pattern; new strings `dataStatus.staleRates` in en/cs: en "No current rate for {{currencies}} — values use the last rate from {{days}}+ days ago", cs "Pro {{currencies}} chybí aktuální kurz — hodnoty používají kurz starší než {{days}} dní".

- [ ] **Step 1: Failing test** (fx_backfill tests; extend the existing in-memory schema with `bank_accounts(currency)`, `stock_data(currency)`, `real_estate(purchase_price_currency, market_price_currency, monthly_rent_currency, purchase_date)`, `user_profile(currency)`):
```rust
#[test]
fn held_currencies_cover_balances_quotes_properties_and_pegs() {
    let conn = setup_test_db();
    conn.execute_batch("
        INSERT INTO bank_accounts (id, currency) VALUES ('a', 'CHF');
        INSERT INTO stock_data (ticker, currency) VALUES ('CSPX.L', 'USD');
        INSERT INTO real_estate (id, purchase_price_currency, market_price_currency, monthly_rent_currency) VALUES ('r', 'PLN', 'PLN', NULL);
        INSERT INTO user_profile (id, currency) VALUES (1, 'AED');
        INSERT INTO bank_accounts (id, currency) VALUES ('b', 'KWD');
    ").expect("seed");
    assert_eq!(held_currencies(&conn).expect("held"), vec!["CHF", "PLN", "USD"]);
}
```
- [ ] **Step 2:** `cargo test fx_backfill` → FAIL.
- [ ] **Step 3:** Implement; reuse the existing range logic. Add the staleness computation to `get_price_status` via a service function `fx_backfill::stale_currencies(conn, today) -> Result<Vec<String>>` with its own test (one currency last quoted 20 days ago is stale, one quoted 2 days ago is not).
- [ ] **Step 4:** Regenerate bindings (`cd src-tauri && cargo test generate_bindings -- --ignored`), update `shared/schema.ts`, DataStatus, locales. Gates.
- [ ] **Step 5:** Commit `feat: exchange-rate history for every held currency and stale-rate warning`.

### Task 10: Main currency restricted to supported currencies

**Files:**
- Modify: `src-tauri/src/services/auth.rs` (profile create/update; lines ~122, ~254), `src-tauri/src/services/onboarding.rs` (if it sets the currency), the profile `Insert*`/update struct `validate()`
- Modify: the settings currency picker (grep `currencyName(` / `CURRENCY_CODES` under `src/components/settings` or `src/pages/Settings*`), `src/utils/locale-defaults.ts` (region → currency), `src/components/onboarding/OnboardingWizard.tsx:~106`
- Modify: locales en/cs `settings.json` (`currency.noRateHint` → replaced by `currency.unsupportedMain`)
- Test: `src/utils/locale-defaults.test.ts`, Rust test in the profile service module

**Interfaces:**
- Rust: profile `validate()` returns `AppError::Validation("currency.unsupported".into())` (follow the module's existing validation message convention) when `!fx::is_supported(&currency)`; applies to create and update, not to reading an existing row.
- TS: `defaultCurrencyForRegion(region)` never returns an unsupported code: unsupported → `EUR` for regions in the European set already used by the module, else `USD`.

- [ ] **Step 1: Failing tests.** TS:
```ts
import { defaultCurrencyForRegion } from './locale-defaults';
import { isFxSupported } from '@shared/currencies';
it.each(['AE', 'SA', 'BG', 'UA', 'VN', 'RS', 'KW', 'AR', 'TW', 'EG'])('%s defaults to a supported currency', (r) => {
  expect(isFxSupported(defaultCurrencyForRegion(r))).toBe(true);
});
it('keeps pegged and euro defaults', () => {
  expect(defaultCurrencyForRegion('AE')).toBe('AED');
  expect(defaultCurrencyForRegion('BG')).toBe('EUR');
  expect(defaultCurrencyForRegion('RS')).toBe('EUR');
  expect(defaultCurrencyForRegion('UA')).toBe('USD');
});
```
Rust (profile service test module, in-memory `user_profile` table): updating the currency to `"KWD"` returns `Err(AppError::Validation(_))`; to `"aed"` succeeds and stores `"AED"`.
- [ ] **Step 2:** Run both → FAIL.
- [ ] **Step 3:** Implement. Settings picker lists `FX_SUPPORTED_CODES`; when the current profile currency is unsupported, show it as the selected value plus the warning `currency.unsupportedMain` (en "Your main currency has no exchange-rate source. Totals that mix currencies cannot be converted — choose a supported main currency." / cs "Vaše hlavní měna nemá zdroj kurzu. Součty v různých měnách nejde převést — zvolte podporovanou hlavní měnu."). BG maps to EUR (Bulgaria uses the euro since 2026-01-01).
- [ ] **Step 4:** Gates. Commit `feat: main currency must have an exchange-rate source`.

### Task 11: Phase 1 review checkpoint

- [ ] Full gates (both sides), `cargo test generate_bindings -- --ignored` without drift.
- [ ] Dispatch a code reviewer (model sonnet) over `git diff main...HEAD` restricted to phase 0–1 files against this plan and the spec; fix findings; commit `fix: phase 1 review findings` if any.

---

# Phase 2 — explicit currency in every monetary response

### Task 12: Conversion-quality wire type, FX snapshot commands, required report currency

**Files:**
- Create: `src-tauri/src/models/fx.rs` (+ `models/mod.rs`)
- Modify: `src-tauri/src/bindings.rs`, `src-tauri/src/lib.rs` (register commands), `src-tauri/src/commands/portfolio.rs` (new commands), `commands/cashflow.rs:12-84`, `commands/budgeting.rs:24-56`
- Modify: `shared/schema.ts`, `src/lib/tauri-api.ts`, `shared/generated-types.ts` (regenerated)

**Interfaces — Produces:**
```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct ConversionQuality {
    #[serde(rename = "missingCurrencies")] pub missing_currencies: Vec<String>,
    #[serde(rename = "missingCount")] pub missing_count: i32,
    #[serde(rename = "estimated")] pub estimated: bool,
}
impl From<&fx::Quality> for ConversionQuality { … }

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxSnapshot {
    #[serde(rename = "reference")] pub reference: String,
    #[serde(rename = "units")] pub units: HashMap<String, f64>,   // incl. derived codes and the reference (1.0)
    #[serde(rename = "revision")] pub revision: i64,              // last fetch timestamp (seconds); 0 when empty
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxDayQuotes {
    #[serde(rename = "day")] pub day: i64,
    #[serde(rename = "units")] pub units: HashMap<String, f64>,
}
```
Commands: `get_fx_snapshot() -> Result<FxSnapshot>`, `get_fx_history(start_day: i64, end_day: i64) -> Result<Vec<FxDayQuotes>>` (stored days only, each with derived codes resolved). `fx::Quality` gains `estimated: bool` + `mark_estimated()`. `report_currency: String` (no `Option`, no `"CZK"` default) on the five cashflow/budgeting commands; invalid → `AppError::Validation`.
TS: `fxApi.getSnapshot(): Promise<FxSnapshot>`, `fxApi.getHistory(startDay, endDay): Promise<FxDayQuotes[]>`; report API functions make `reportCurrency` a required parameter.

- [ ] **Step 1:** Rust test in `services/fx.rs`: `History::from_days` with one day `{USD: 0.05}` (CZK reference) → a `snapshot_units(&quotes)` helper returns units containing `CZK: 1.0`, `USD: 0.05`, `AED: 0.05 * 3.6725`. Run → FAIL; implement `pub fn snapshot_units(q: &Quotes) -> HashMap<String, f64>` (all `codes()` with their `units`); PASS.
- [ ] **Step 2:** Add the wire types, commands (thin; logic in `fx.rs`), bindings registration; regenerate types; mirror in `shared/schema.ts` per the 7-step procedure; add `tauri-api.ts` wrappers; make report currency required (frontend callers already pass it — `npm run typecheck` proves it).
- [ ] **Step 3:** Gates. Commit `feat: FX snapshot and history commands, required report currency`.

### Task 13: Frontend FX core

**Files:**
- Create: `shared/fx.ts`, `shared/fx.test.ts`, `src/hooks/use-fx.ts`
- Modify: `src/lib/currency.ts`, `src/lib/currency-provider.tsx`, `src/lib/format.ts:100-120`, `src/components/shell/DataStatus.tsx`, `src/hooks/SyncProvider.tsx:60-75`

**Interfaces — Produces:**
```ts
// shared/fx.ts
export type FxQuotes = { reference: string; units: Record<string, number> }; // units per 1 reference; derived codes included by the backend
export type FxDay = { day: number; units: Record<string, number> };
export function normalizeCode(code: string | null | undefined): string | null;
export function convert(q: FxQuotes | undefined, amount: number, from: string, to: string): number | null;
export function makeHistory(days: FxDay[], reference: string): FxHistory; // sorted by day
export function convertOn(h: FxHistory | undefined, amount: number, from: string, to: string, timestamp: number): number | null; // common day, ≤10 days back
export type FxHistory = { reference: string; days: FxDay[] };
export class MissingTracker { track(from: string, v: number | null): number | null; get missing(): string[]; get complete(): boolean }
```
```ts
// src/hooks/use-fx.ts
export function useFxSnapshot(): { quotes: FxQuotes | undefined; revision: number; isLoading: boolean };
export function useFxHistory(startTs: number | undefined, endTs: number | undefined): FxHistory | undefined;
```
`useCurrency()` context becomes `{ currencyCode, currency, fx: FxQuotes | undefined, fxRevision: number, convert(amount, from, to?): number | null /* to defaults to currencyCode */ }` — the old fields stay until Task 21 so the app keeps compiling; mark them `/** @deprecated removed in Task 21 */`.
`refreshExchangeRates` callers (DataStatus, SyncProvider, CurrencyProvider) invalidate `['fx-snapshot']` after success; every converted query key includes `fxRevision`.
`fmt.money(amount, currency, opts)`: uses `currencyDisplay: 'narrowSymbol'` only when `currency === mainCurrency` (pass the main currency into `getFormatters(locale, mainCurrency)`), otherwise `'code'`.

- [ ] **Step 1: Failing tests** (`shared/fx.test.ts`):
```ts
import { describe, expect, it } from 'vitest';
import vectors from './fx-test-vectors.json';
import { convert, convertOn, makeHistory, MissingTracker } from './fx';

const quotes = (() => {
  const units: Record<string, number> = { ...vectors.quotes.units, [vectors.quotes.reference]: 1 };
  // the backend includes derived codes; mirror that for the vectors
  units.AED = units.USD * 3.6725; units.SAR = units.USD * 3.75; units.BGN = 1.95583;
  return { reference: vectors.quotes.reference, units };
})();

describe('convert', () => {
  it.each(vectors.cases)('$from -> $to', ({ amount, from, to, expected }) => {
    const got = convert(quotes, amount, from, to);
    if (expected === null) expect(got).toBeNull();
    else expect(got).toBeCloseTo(expected, 9);
  });
  it('returns null without quotes except for same currency', () => {
    expect(convert(undefined, 5, 'EUR', 'USD')).toBeNull();
    expect(convert(undefined, 5, 'EUR', 'eur')).toBe(5);
  });
});

describe('convertOn', () => {
  const DAY = 86_400;
  const d = 20_000 * DAY;
  const h = makeHistory([
    { day: d, units: { CZK: 1, EUR: 1 / 24 } },
    { day: d - DAY, units: { CZK: 1, EUR: 1 / 25, USD: 1 / 20 } },
  ], 'CZK');
  it('uses a common day for both currencies', () => {
    expect(convertOn(h, 100, 'USD', 'EUR', d + 3600)).toBeCloseTo(80, 9);
  });
  it('gives up after ten days', () => {
    expect(convertOn(h, 1, 'EUR', 'CZK', d + 11 * DAY)).toBeNull();
  });
});

describe('MissingTracker', () => {
  it('collects missing currencies once', () => {
    const t = new MissingTracker();
    t.track('KWD', null); t.track('KWD', null); t.track('USD', 1);
    expect(t.missing).toEqual(['KWD']);
    expect(t.complete).toBe(false);
  });
});
```
- [ ] **Step 2:** `npx vitest run shared/fx.test.ts` → FAIL.
- [ ] **Step 3:** Implement `shared/fx.ts`, `use-fx.ts`, provider/context changes, formatter symbol rule, invalidation wiring. Add a `format.test.ts` case: `money(5, 'USD')` with main `CZK` renders the code `USD`, with main `USD` renders `$`.
- [ ] **Step 4:** Gates. Commit `feat: frontend FX snapshot, pure conversion and main-currency symbols`.

### Task 14: Positions service (stocks and crypto) in a report currency

**Files:**
- Create: `src-tauri/src/services/positions.rs`
- Modify: `src-tauri/src/services/cost_basis.rs` (realized gains in target), `src-tauri/src/services/pricing.rs` (drop `price_czk` from `ResolvedPrice`; expose native price + currency + source + fetched_at)
- Read first: `shared/calculations/realized-gains.ts`, `shared/calculations/cost-basis.ts`, `shared/calculations/holding-metrics.ts` (the semantics being ported), `src-tauri/src/commands/investments.rs` (`EnrichedStockInvestment`), `src-tauri/src/commands/crypto.rs` (`EnrichedCryptoInvestment`)

**Interfaces — Produces:**
```rust
pub enum AssetKind { Stock, Crypto }
pub struct PositionValue {
    pub investment_id: String,
    pub ticker: String,
    pub quantity: f64,
    pub price: Option<f64>,            // native current price (resolver), None when unpriced
    pub price_currency: Option<String>,
    pub value: Option<f64>,            // report currency, current snapshot
    pub cost_basis: Option<f64>,       // report currency, each tx at its day (estimated flag in quality)
    pub average_price: f64,            // native, primary currency
    pub average_price_currency: String,
    pub unrealized_gain: Option<f64>,  // value - cost_basis
    pub unrealized_gain_percent: Option<f64>,
    pub realized_gain: Option<f64>,    // report currency: sell proceeds at sell day - released cost
    pub dividend_per_share: Option<f64>,      // native yearly per share
    pub dividend_currency: Option<String>,
    pub yearly_dividend: Option<f64>,  // report currency
}
pub struct Positions { pub currency: String, pub items: Vec<PositionValue>, pub quality: fx::Quality }
pub fn value_positions(conn: &Connection, kind: AssetKind, report_currency: &str, today: i64) -> Result<Positions>;
```
Port `realized-gains.ts` semantics exactly (average-cost release on sell; proceeds = qty × price converted at the sell day). Cost-basis per transaction day in the report currency via `History::convert_on`; fallback to current quotes marks `quality.mark_estimated()`; neither → currency recorded missing and `cost_basis`/gains `None` for that position.

- [ ] **Step 1: Failing tests** (positions.rs, in-memory schema with `stock_investments`, `investment_transactions`, `stock_data`, `stock_price_overrides`, `dividend_data`, `dividend_overrides`, `exchange_rate_history` — copy column lists from `src-tauri/src/db/schema.snapshot`):
```rust
#[test]
fn eur_investor_gain_on_usd_stock_ignores_czk_moves() {
    // C1: bought 10 x 100 USD when USDCZK = EURCZK = 25; today USDCZK 25, EURCZK 22.
    let conn = setup();
    let buy_day = 19_000 * DAY;
    seed_rates(&conn, buy_day, &[("USD", 25.0), ("EUR", 25.0)]);
    seed_stock(&conn, "s1", "AAPL", "USD", &[("buy", 10.0, 100.0, buy_day)], 100.0);
    set_current_czk(&[("USD", 25.0), ("EUR", 22.0)]);
    let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
    let item = &p.items[0];
    assert_close(item.cost_basis.expect("cost"), 1000.0);
    assert_close(item.value.expect("value"), 1136.3636363636);
    assert_close(item.unrealized_gain.expect("gain"), 136.3636363636);
    assert!(p.quality.is_complete());
}

#[test]
fn realized_gain_uses_sell_day_rate() {
    let conn = setup();
    let (d1, d2) = (19_000 * DAY, 19_100 * DAY);
    seed_rates(&conn, d1, &[("USD", 20.0)]);
    seed_rates(&conn, d2, &[("USD", 25.0)]);
    seed_stock(&conn, "s1", "AAPL", "USD", &[("buy", 10.0, 100.0, d1), ("sell", 5.0, 100.0, d2)], 100.0);
    set_current_czk(&[("USD", 25.0)]);
    let p = value_positions(&conn, AssetKind::Stock, "CZK", 20_000 * DAY).expect("positions");
    // released cost 5*100*20 = 10000 CZK, proceeds 5*100*25 = 12500 CZK
    assert_close(p.items[0].realized_gain.expect("realized"), 2500.0);
}

#[test]
fn unconvertible_position_is_listed_and_reported() {
    let conn = setup();
    seed_stock(&conn, "s1", "KWFH", "KWD", &[("buy", 1.0, 10.0, 19_000 * DAY)], 10.0);
    set_current_czk(&[("USD", 25.0)]);
    let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].value, None);
    assert_eq!(p.items[0].price, Some(10.0));
    assert_eq!(p.quality.missing_currencies(), vec!["KWD".to_string()]);
}

#[test]
fn same_currency_needs_no_rates() {
    let conn = setup();
    seed_stock(&conn, "s1", "SAP.DE", "EUR", &[("buy", 2.0, 100.0, 19_000 * DAY)], 120.0);
    set_current_czk(&[]);
    let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
    assert_close(p.items[0].unrealized_gain.expect("gain"), 40.0);
}
```
(`setup`, `seed_rates`, `seed_stock(conn, id, ticker, currency, txs, current_price)`, `set_current_czk`, `assert_close(a, b)` with tolerance 1e-6 are test helpers written in the module; `set_current_czk` takes the `fx::test_lock()`.)
- [ ] **Step 2:** `cargo test services::positions` → FAIL.
- [ ] **Step 3:** Implement; crypto uses the same function with `crypto_*` tables (mixed transaction currencies allowed: cost per transaction converted directly into the report currency).
- [ ] **Step 4:** PASS; clippy. Commit `feat: value stock and crypto positions in a report currency`.

### Task 15: Investment and crypto commands, tags, analysis and MCP on the positions service

**Files:**
- Modify: `src-tauri/src/commands/investments.rs` (`get_all_investments`, `get_investment`, `get_investment_with_details`, `update_investment_name` response), `src-tauri/src/commands/crypto.rs` (`get_all_crypto`), `src-tauri/src/commands/stock_tags.rs` (`get_stocks_analysis`, `get_tag_metrics`), `src-tauri/src/services/mcp/{analytics,investments,crypto}.rs`, models for the response types, `bindings.rs`
- Modify: `shared/schema.ts` (+ remove the duplicate `StockInvestmentWithPrice`/`CryptoInvestmentWithPrice` from `shared/types/extended-types.ts`), `src/lib/tauri-api.ts`

**Interfaces — Produces (wire, all monetary converted fields decimal TEXT via `reporting::money_in_currency`, nullable):**
- `EnrichedStockInvestment`/`EnrichedCryptoInvestment` keep every non-CZK field they have today and replace the CZK ones: remove `currentPrice` (CZK) and `dividendYield`; add `price: Option<String>` + `priceCurrency` (native), `value`, `costBasis`, `unrealizedGain`, `unrealizedGainPercent: Option<f64>`, `realizedGain`, `dividendPerShare` + `dividendCurrency` (native), `yearlyDividend`. The list commands return `{ currency: String, fxQuality: ConversionQuality, items: Vec<…>, totals: PositionTotals }` where `PositionTotals { value, costBasis, unrealizedGain, realizedGain, yearlyDividend }` sum convertible items only.
- `get_stocks_analysis(report_currency)` / `get_tag_metrics(report_currency)` build on `value_positions`; fields `currentValue`, `gainLoss`, `gainLossPercent`, `dividendYield` → `value`, `unrealizedGain`, `unrealizedGainPercent`, `yearlyDividend` (TEXT) + response `currency` and `fxQuality`; tag totals likewise.
- MCP `stocks_analysis`, `tag_metrics`, `investments_list`, `crypto_list`: use `value_positions` with the main currency; money via `money_in_currency`; `missingCurrencies` output when incomplete.

- [ ] **Step 1: Failing test** (`commands` are not tested; add to `services/positions.rs`): `tag_totals(&positions, &tag_map) -> HashMap<tag_id, PositionTotals>` sums only convertible items and returns quality per tag — test with one convertible and one KWD position in the same tag (total = convertible one, tag quality lists KWD). Then MCP parity test in `services/mcp/analytics.rs` tests: for the C1 fixture, the MCP `stocks_analysis` output's `gainLoss` for AAPL equals `"136.36"` with `mainCurrency: "EUR"` (read the module's existing test setup for the MCP context).
- [ ] **Step 2:** FAIL → implement → PASS. Regenerate bindings; update `shared/schema.ts` per the 7-step procedure; `tauri-api.ts` functions take `reportCurrency`.
- [ ] **Step 3:** The frontend stops compiling once these wire types change. To keep the branch green, this task also performs the minimal caller update: pass `currencyCode` and adapt field names in `Stocks.tsx`, `Crypto.tsx`, `StockDetail.tsx`, `CryptoDetail.tsx`, `StocksAnalysis.tsx`, `InvestmentsTable.tsx`, `InvestmentsSummary.tsx`, `CryptoTable.tsx`, `CryptoSummary.tsx`, `TagShareBars.tsx`, `PositionHero.tsx` — formatting values with `fmt.money(Number(v), response.currency)`; remove the frontend holding maths there (`calculateMetrics`, `useDatedConvert`, `calculateRealizedGains`, `calculatePortfolioTotals` uses on these pages) in favour of the response fields and `totals`. Query keys: `['investments', currencyCode, fxRevision]`, `['crypto', currencyCode, fxRevision]`, `['stocks-analysis', currencyCode, fxRevision]`, `['tag-metrics', currencyCode, fxRevision]`.
- [ ] **Step 4:** Gates. Commit `feat: investment, crypto, tag and MCP analytics in the report currency`.

### Task 16: TWR and ticker value history in a report currency

**Files:**
- Modify: `src-tauri/src/services/investments.rs:~918-1060` (`compute_twr_for_tickers`), `src-tauri/src/services/ticker_history.rs` (read API), `src-tauri/src/commands/investments.rs` (`get_stock_twr`, `get_stock_value_history`), `src-tauri/src/commands/crypto.rs` (`get_crypto_value_history`), `src-tauri/src/services/mcp/mod.rs:~127` (value history tools), models, bindings, `shared/schema.ts`, `src/lib/tauri-api.ts` (`TickerValueHistory`), `src/components/stocks/PositionHero.tsx`

**Interfaces — Produces:**
- `TickerValueHistory { recordedAt, quantity: String, price: String, currency: String, value: Option<String> /* report currency at that day */, isStale }` plus response `{ currency, fxQuality, points }`; `valueCzk` removed from the wire.
- TWR input: per day `value = quantity × price` converted with `History::convert_on(…, report_currency, day)`; flows (buys/sells) converted at their own day. Days without conversion are skipped and recorded in quality.

- [ ] **Step 1: Failing test** (investments.rs tests): USD-only stock, price constant 100 USD for 3 days, USDCZK moves 20 → 25, report currency USD → TWR series all `0.0`; report currency CZK → last point `25.0` (%). 
- [ ] **Step 2:** FAIL → implement → PASS. PositionHero converts nothing; plots `value` and the position's `costBasis` from Task 15 in the same currency.
- [ ] **Step 3:** Gates. Commit `feat: TWR and ticker history in the report currency`.

### Task 17: Net worth and history in a report currency

**Files:**
- Create: `src-tauri/src/services/net_worth.rs`
- Modify: `src-tauri/src/commands/portfolio.rs` (`get_portfolio_metrics`, `get_portfolio_history`; keep `compute_breakdowns`/snapshot writer as is), `src-tauri/src/services/mcp/portfolio.rs`, models (`PortfolioMetrics`, `PortfolioMetricsHistory` wire), bindings, `shared/schema.ts`, `src/lib/tauri-api.ts`
- Modify (frontend): `src/pages/Dashboard.tsx`, `src/components/common/PortfolioTrendCard.tsx`, `src/components/dashboard/*` consumers of metrics/history; delete `src/hooks/use-historical-display-values.ts` and the CZK paths of `src/utils/historical-rates.ts` once unused

**Interfaces — Produces:**
```rust
pub struct NetWorth { pub currency: String, pub classes: ClassTotals /* savings, investments, crypto, bonds, real_estate_personal, real_estate_investment, other_assets, loans: Option<f64> each */, pub total_assets: f64, pub total_liabilities: f64, pub net_worth: f64, pub quality: fx::Quality }
pub fn current_net_worth(conn: &Connection, report_currency: &str, exclude_personal_real_estate: bool) -> Result<NetWorth>;
pub struct HistoryPoint { pub recorded_at: i64, pub source: String, pub is_stale: bool, pub classes: ClassTotals, pub net_worth: Option<f64> }
pub fn history_in(conn: &Connection, report_currency: &str, from: Option<i64>, to: Option<i64>) -> Result<(Vec<HistoryPoint>, fx::Quality)>;
```
`current_net_worth` sums the native breakdowns that `compute_breakdowns` already produces (convert each `(currency, amount)` with `fx::current()`; class `None` only when every amount in it is unconvertible; totals exclude missing and record them). `history_in` converts each row's `*_by_currency` at that row's day with one `History::load` for the whole range; a class with an empty breakdown and a non-zero CZK total falls back to the CZK total converted at that day (phase-2 compatibility for legacy rows; removed in Task 24). Wire: `PortfolioMetrics { currency, fxQuality, totalSavings…netWorth: String /*TEXT*/, *ByCurrency maps unchanged }`, `PortfolioHistoryResponse { currency, fxQuality, points: Vec<PortfolioHistoryPoint> }` (same class field names as today, TEXT, nullable).

- [ ] **Step 1: Failing tests** (net_worth.rs, in-memory `portfolio_metrics_history` + `exchange_rate_history` + the tables `compute_breakdowns` reads — or test `history_in` and a pure `sum_breakdowns(&HashMap<String,f64>, &Quotes, &mut Quality) -> Option<f64>`):
  - EUR-only breakdowns `{"EUR": 1000}` with no CZK quote anywhere → `history_in(…, "EUR")` returns 1000 for every day (spec acceptance).
  - Row with `{"USD": 100}` on a day with USD=25 CZK, EUR=25 CZK → 100 EUR in report EUR; on a later day USD=25, EUR=20 → 125 EUR.
  - `{"KWD": 5, "EUR": 10}` → class 10, quality lists KWD.
- [ ] **Step 2:** FAIL → implement → PASS. MCP `portfolio_get_metrics`/`portfolio_get_history` call the same functions with the main currency.
- [ ] **Step 3:** Frontend: Dashboard and trend cards read `currency` + TEXT values (`Number(...)`), no conversion; query keys `['portfolio-metrics', currencyCode, fxRevision]`, `['portfolio-history', currencyCode, fxRevision, …]` (mutation invalidation by prefix `['portfolio-metrics']` / `['portfolio-history']` keeps working — verify `use-bank-account-mutations.ts` uses prefix invalidation). Incomplete quality → the compact warning component used by reports (grep `missingFx` in `src/components/reports`).
- [ ] **Step 4:** Gates. Commit `feat: net worth and portfolio history in the report currency`.

### Task 18: Projection in a report currency with native contributions

**Files:**
- Modify: `src-tauri/src/commands/projection.rs` (current values from `net_worth`/`positions`; contribution read natively at ~547), `src-tauri/src/models/projection.rs`, specta registration of `ProjectionTimelinePoint`/`PortfolioProjection` (currently unregistered — register them), `shared/schema.ts`, `src/components/projection/ProjectionParamsDialog.tsx:95-130`, `src/utils/projection-assumptions.ts:158-172`, `src/pages/Projection.tsx`, `src/utils/projection-milestones.ts`

**Interfaces:**
- `calculate_portfolio_projection(report_currency)` → `{ currency, fxQuality, … }` with TEXT amounts; contributions: `monthly_contribution` in `contribution_currency` converted with `fx::current()` into the report currency (missing → contribution excluded, quality records it).
- Save path: the dialog stores the typed amount with `contributionCurrency: currencyCode`; untouched rows keep their stored amount and currency; display converts stored contribution with `useCurrency().convert`.
- Milestone round marks computed on report-currency values.
- If the projection maths lives in a pure function, add the logic there; move it out of the command into `services/projection.rs` if the command does it inline (thin-command rule) — keep `services/projection_math.rs` as is.

- [ ] **Step 1: Failing test** (service test): a stored contribution `500 EUR` with report currency EUR is used as 500/month regardless of the CZK rate; with report currency CZK at EUR=25 → 12500/month; a `'CZK'` legacy row of 12500 in report EUR at EUR=25 → 500.
- [ ] **Step 2:** FAIL → implement → PASS; frontend changes; gates.
- [ ] **Step 3:** Commit `feat: projection in the report currency with contributions in their own currency`.

### Task 19: Native-sum pages convert through the FX snapshot

**Files:**
- Modify: `src/pages/BankAccounts.tsx` (drop the local fallback policy, K6), `src/hooks/use-bank-accounts.ts` (remove unused `metrics`), `src/utils/bank-account-summary.ts`, `src/hooks/use-bonds.ts`, `src/utils/bonds.ts`, `src/components/bonds/*`, `src/hooks/use-loans.ts`, `src/utils/loans.ts`, `src/components/loans/{LoansTable,LoansSummary,DebtTrajectoryCard}.tsx`, `src/hooks/use-insurance.ts`, `src/utils/insurance.ts`, `src/components/insurance/*`, `src/pages/RealEstate.tsx`, `src/pages/RealEstateDetail.tsx`, `src/utils/other-assets.ts`, `src/pages/OtherAssets.tsx`, `src/components/other-assets/OtherAssetsTable.tsx`, `src/components/dashboard/RecentMoves.tsx`, `src/utils/valuation-trace.ts`, `src/utils/bank-activity.ts` + `BankAccountDetail.tsx:610-613`
- Test: the pure utils' existing `*.test.ts` files

**Rules:** each util that today returns `*Czk` numbers takes `(rows, convert: (amount, currency) => number | null)` and returns values in the main currency with `null` for unconvertible rows plus a `missing: string[]`; historical values (debt trajectory points, valuation trace, purchase price, other-asset cost, recent moves) take a `convertAt(amount, currency, timestamp)` built from `useFxHistory` + `convertOn`. Rows render their native amount (`fmt.money(amount, currency)`) when the converted value is `null`, with the missing marker used by reports; totals exclude them and show the compact warning. `MOVEMENT_FLOOR_BASE` becomes `MOVEMENT_FLOOR_EUR = 400` converted into the account currency through `fx` (no conversion available → floor 0).

- [ ] **Step 1: Failing tests:** update each util test so the converter is injected; add one case per util where a `KWD` row yields `null`, is excluded from the total and appears in `missing`. Example (`src/utils/bonds.test.ts`):
```ts
const convert = (amount: number, currency: string) => (currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount);
it('excludes unconvertible bonds from the total and reports them', () => {
  const s = bondsSummary([bond({ currency: 'EUR', value: '100' }), bond({ currency: 'KWD', value: '7' })], convert);
  expect(s.totalValue).toBe(2500);
  expect(s.missing).toEqual(['KWD']);
});
```
(Use the util's real exported function names; rename `*Czk` results to neutral names such as `value`.)
- [ ] **Step 2:** FAIL → implement → PASS; pages wired; `useMemo` deps include `fxRevision`.
- [ ] **Step 3:** Gates. Commit `feat: asset pages convert native amounts through the FX snapshot`.

### Task 20: MCP rates tool and precision

**Files:**
- Modify: `src-tauri/src/services/mcp/exchange_rates.rs`, `src-tauri/src/services/mcp/money.rs:~177`, `mcp/portfolio.rs:~190`, `mcp/analytics.rs:~235,333`, `mcp/mod.rs` (server instructions ~565/870, tool descriptions)

- [ ] **Step 1: Failing test:** `exchange_rates_list` returns `baseCurrency` equal to the FX reference, `rates` as units per base (derived included) and `supportedCurrencies` (36 codes); a JPY main-currency amount is emitted with 0 decimals and KWD with 3 (use `money_in_currency`).
- [ ] **Step 2:** FAIL → implement → PASS; update tool descriptions (no "internal base currency CZK"; explain units per base). Gates. Commit `feat: MCP exchange rates against an explicit base and ISO precision`.

### Task 21: Remove the implicit-CZK machinery and CZK-scale defaults

**Files:**
- Modify/delete: `src/lib/currency.ts`, `src/lib/currency-provider.tsx` (remove `formatCurrency`, `formatCurrencySigned`, `formatCurrencyShort`, `formatCurrencyRaw`, `formatPrice`, CZK-based `convert`), `shared/currencies.ts` (remove `EXCHANGE_RATES`, `BUILT_IN_RATES`, `updateExchangeRates`, `hasKnownRate`, `convertToCzK`, `convertFromCzK`, `BASE_CURRENCY`, `isEcbCurrency`/`ECB_CURRENCY_CODES` if unused), `src/hooks/use-dated-convert.ts` (delete if unused), `shared/calculations/*` functions left without callers (delete with their tests), `src-tauri/src/services/currency.rs` (remove `convert_to_czk`, `convert_to_czk_with_rates`, `convert_to_czk_at`, `get_all_rates`, `get_rates_for_date*` if unused, `breakdown_czk_at_rates` — keep what the snapshot writer still needs until Task 24), `src-tauri/src/constants.rs` (`DEFAULT_BASE_CURRENCY`)
- Modify: `src/pages/AnnuityCalculator.tsx:73-77`, `src/pages/EstateCalculator.tsx:140-151`, `src/components/loans/LoanWhatIfCard.tsx:24` — defaults start empty or scale from EUR constants through `convert`; what-if step = 1 % of the loan balance rounded to a power of ten.
- Modify: `docs/DEPRECATED.md` (list every removed API with its replacement), the remaining `|| 'CZK'` UI fallbacks → the main currency (`currencyCode`).

- [ ] **Step 1:** Remove; let the compiler find every remaining caller and migrate it to `fmt.money(amount, currency)` / `useCurrency().convert` / response currency. The snapshot writer (`compute_breakdowns` → `upsert_snapshot`) keeps writing CZK totals in phase 2 through `fx::current().convert(…, "CZK")` inside `portfolio_history.rs` only.
- [ ] **Step 2:** Acceptance grep (expected: matches only in migrations, `portfolio_history.rs` snapshot writer, tests, DEPRECATED.md):
```bash
rg -n "convert_to_czk|convertToCzK|convertFromCzK|formatCurrencyRaw|formatCurrencyShort|formatCurrencySigned|formatCurrency\(|EXCHANGE_RATES|BASE_CURRENCY" src shared src-tauri/src
```
- [ ] **Step 3:** Full gates. Commit `refactor: remove implicit-CZK converters and formatters`.

### Task 22: Phase 2 review checkpoint

- [ ] Full gates + bindings drift check.
- [ ] Launch the demo profile (`moony-demo` launch config, password from memory note `dev-bridge-background-throttling`) only if the dev bridge is available; otherwise rely on tests. Smoke: Dashboard, Stocks, StockDetail, Crypto, Bonds, Loans, Insurance, Real estate, Other assets, Bank accounts, Projection, Stocks analysis in CZK, EUR and AED main currency; switch currency and confirm refetch.
- [ ] Code review subagent (sonnet) over phase-2 diff; fix; commit.

---

# Phase 3 — EUR rate reference and retiring CZK caches

### Task 23: Migration 010 — rates in EUR quotes

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (append `("010_fx_eur_reference", MIGRATION_010)`), `src-tauri/src/db/schema.snapshot` (regenerate; the schema must come out identical — the migration only rewrites data), `src-tauri/src/services/currency.rs` (`parse_ecb_xml`, `save_rates_to_db`, `save_rates_to_history_db`, `load_rates_from_db`), `src-tauri/src/services/fx_backfill.rs` (`parse_frankfurter_rates`, request symbols), `src-tauri/src/services/fx.rs` (`History::load`, a `REFERENCE: &str = "EUR"`; remove `from_czk_per_unit`/`to_czk_per_unit` after the last caller), `docs/architecture/database.md`

**Migration SQL (exact):**
```sql
-- Rates become units of currency per 1 EUR (the ECB's own base).
CREATE TABLE exchange_rate_history_eur (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));
INSERT INTO exchange_rate_history_eur (date, currency, rate)
  SELECT h.date, h.currency, e.rate / h.rate
  FROM exchange_rate_history h JOIN exchange_rate_history e ON e.date = h.date AND e.currency = 'EUR'
  WHERE h.currency NOT IN ('EUR', 'CZK') AND h.rate > 0 AND e.rate > 0;
INSERT INTO exchange_rate_history_eur (date, currency, rate)
  SELECT date, 'CZK', rate FROM exchange_rate_history WHERE currency = 'EUR' AND rate > 0;
DROP TABLE exchange_rate_history;
ALTER TABLE exchange_rate_history_eur RENAME TO exchange_rate_history;
CREATE INDEX idx_erh_currency_date ON exchange_rate_history(currency, date);

CREATE TABLE exchange_rates_eur (currency TEXT PRIMARY KEY, rate REAL NOT NULL, fetched_at INTEGER NOT NULL);
INSERT INTO exchange_rates_eur (currency, rate, fetched_at)
  SELECT r.currency, e.rate / r.rate, r.fetched_at
  FROM exchange_rates r JOIN exchange_rates e ON e.currency = 'EUR'
  WHERE r.currency NOT IN ('EUR', 'CZK') AND r.rate > 0 AND e.rate > 0;
INSERT INTO exchange_rates_eur (currency, rate, fetched_at)
  SELECT 'CZK', rate, fetched_at FROM exchange_rates WHERE currency = 'EUR' AND rate > 0;
DROP TABLE exchange_rates;
ALTER TABLE exchange_rates_eur RENAME TO exchange_rates;

INSERT OR REPLACE INTO app_config (key, value) VALUES ('fx_reference', 'EUR');
```
(`idx_erh_currency_date` is the index name in `schema.snapshot`; dropping the old table drops it, so it is recreated. Drop derived codes if any were stored: add `DELETE FROM exchange_rate_history WHERE currency IN ('AED','SAR','QAR','BHD','OMR','BGN'); DELETE FROM exchange_rates WHERE currency IN (…);` at the end — derived quotes are never stored.)

Ingest: `parse_ecb_xml` returns units per EUR as published (no CZK requirement; an empty feed is still an error); `parse_frankfurter_rates` stores the response's rates as published, keeps days without CZK; the request no longer forces `symbols=CZK`. `History::load` builds `Quotes::new("EUR")` with `units = rate`.

- [ ] **Step 1: Failing tests** (migrations.rs tests, following the existing migration-test pattern that applies migrations up to N on an in-memory DB):
```rust
#[test]
fn migration_010_rebases_rates_on_eur() {
    let conn = db_migrated_until("010_fx_eur_reference"); // existing helper pattern: apply all migrations before 010
    conn.execute_batch("
        INSERT INTO exchange_rate_history VALUES (86400, 'EUR', 25.0), (86400, 'USD', 20.0), (86400, 'AED', 5.4);
        INSERT INTO exchange_rates VALUES ('CZK', 1.0, 7), ('EUR', 25.0, 7), ('USD', 20.0, 7);
    ").expect("seed");
    apply_migration(&conn, "010_fx_eur_reference");
    let usd: f64 = conn.query_row("SELECT rate FROM exchange_rate_history WHERE date = 86400 AND currency = 'USD'", [], |r| r.get(0)).expect("usd");
    let czk: f64 = conn.query_row("SELECT rate FROM exchange_rate_history WHERE date = 86400 AND currency = 'CZK'", [], |r| r.get(0)).expect("czk");
    assert!((usd - 1.25).abs() < 1e-12 && (czk - 25.0).abs() < 1e-12);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM exchange_rate_history WHERE currency IN ('EUR', 'AED')", [], |r| r.get(0)).expect("n");
    assert_eq!(n, 0);
    let current: f64 = conn.query_row("SELECT rate FROM exchange_rates WHERE currency = 'USD'", [], |r| r.get(0)).expect("cur");
    assert!((current - 1.25).abs() < 1e-12);
    let reference: String = conn.query_row("SELECT value FROM app_config WHERE key = 'fx_reference'", [], |r| r.get(0)).expect("ref");
    assert_eq!(reference, "EUR");
}
```
Plus `parse_ecb_xml` test: a feed without CZK parses and contains USD 1.17 as published (replaces `test_parse_ecb_xml_without_a_czk_quote_is_an_error_not_25`), and an fx History test with EUR-based rows converting USD→CZK.
- [ ] **Step 2:** FAIL → implement → PASS. Regenerate `schema.snapshot` (`cargo test regenerate_schema_snapshot -- --ignored`) and read the diff (index only, if any). `shared/fx.ts` needs no change (reference comes from the snapshot). Update `database.md` rate rows.
- [ ] **Step 3:** Full Rust gates. Commit `feat: store exchange rates against EUR`.

### Task 24: Migration 011 — retire CZK caches

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (`("011_retire_czk_caches", MIGRATION_011)`), `schema.snapshot`, `src-tauri/src/services/portfolio_history.rs` (writer stores native breakdowns only; `carried_statics_from_nearest_live` carries native breakdowns; remove `czk_map`, `czk_total`), `src-tauri/src/commands/portfolio.rs` (delete `backfill_currency_breakdowns` command + its registration in `lib.rs` + its `tauri-api.ts` wrapper and caller), `src-tauri/src/services/currency.rs` (remove `BREAKDOWN_CONSISTENCY_TOLERANCE`, `breakdown_czk_at_rates`), `src-tauri/src/services/ticker_history.rs` + `backfill.rs` + `history_recalc.rs` (stop writing `value_czk`), `src-tauri/src/services/net_worth.rs` (drop the CZK-total fallback; personal real estate from the new column), `docs/architecture/database.md`

**Migration SQL (exact; SQLite ≥ 3.35 `DROP COLUMN` — confirm the bundled version with `SELECT sqlite_version()` in a test; otherwise use create-copy-drop-rename per the playbook):**
```sql
ALTER TABLE portfolio_metrics_history ADD COLUMN real_estate_personal_by_currency TEXT NOT NULL DEFAULT '{}';
UPDATE portfolio_metrics_history SET real_estate_personal_by_currency = json_object('CZK', CAST(total_real_estate_personal AS REAL))
  WHERE CAST(total_real_estate_personal AS REAL) <> 0;
UPDATE portfolio_metrics_history SET savings_by_currency = json_object('CZK', CAST(total_savings AS REAL))
  WHERE savings_by_currency = '{}' AND CAST(total_savings AS REAL) <> 0;
UPDATE portfolio_metrics_history SET investments_by_currency = json_object('CZK', CAST(total_investments AS REAL))
  WHERE investments_by_currency = '{}' AND CAST(total_investments AS REAL) <> 0;
UPDATE portfolio_metrics_history SET crypto_by_currency = json_object('CZK', CAST(total_crypto AS REAL))
  WHERE crypto_by_currency = '{}' AND CAST(total_crypto AS REAL) <> 0;
UPDATE portfolio_metrics_history SET bonds_by_currency = json_object('CZK', CAST(total_bonds AS REAL))
  WHERE bonds_by_currency = '{}' AND CAST(total_bonds AS REAL) <> 0;
UPDATE portfolio_metrics_history SET real_estate_by_currency = json_object('CZK', CAST(total_real_estate_personal AS REAL) + CAST(total_real_estate_investment AS REAL))
  WHERE real_estate_by_currency = '{}' AND CAST(total_real_estate_personal AS REAL) + CAST(total_real_estate_investment AS REAL) <> 0;
UPDATE portfolio_metrics_history SET loans_by_currency = json_object('CZK', CAST(total_loans_principal AS REAL))
  WHERE loans_by_currency = '{}' AND CAST(total_loans_principal AS REAL) <> 0;
UPDATE portfolio_metrics_history SET other_assets_by_currency = json_object('CZK', CAST(total_other_assets AS REAL))
  WHERE other_assets_by_currency = '{}' AND CAST(total_other_assets AS REAL) <> 0;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_savings;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_loans_principal;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_investments;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_crypto;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_bonds;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_real_estate_personal;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_real_estate_investment;
ALTER TABLE portfolio_metrics_history DROP COLUMN total_other_assets;
ALTER TABLE stock_value_history DROP COLUMN value_czk;
ALTER TABLE crypto_value_history DROP COLUMN value_czk;
```
(Column names verified against `schema.snapshot` on 2026-10-09.)

From now on the live writer stores `real_estate_personal_by_currency` (personal subset of `real_estate_by_currency`). `net_worth::history_in` reads personal = that column, investment = `real_estate_by_currency` minus personal per currency; the "exclude personal real estate" view subtracts personal natively.

- [ ] **Step 1: Failing tests:** migration test seeding one legacy row (`'{}'` breakdowns, CZK totals 100/0/…, personal 50 + investment 70) and one modern row (EUR breakdowns) → after 011: legacy row has `{"CZK":100}` savings, real estate `{"CZK":120}`, personal `{"CZK":50}`; modern row breakdowns untouched; `total_*` and `value_czk` columns gone (`PRAGMA table_info`). Net-worth test: a row with real estate `{"EUR": 300}` and personal `{"EUR": 100}`, exclude-personal → real estate 200 EUR.
- [ ] **Step 2:** FAIL → implement → PASS; regenerate `schema.snapshot`, read the diff (exactly: new column, dropped columns).
- [ ] **Step 3:** Acceptance grep: `rg -n "value_czk|valueCzk|total_savings|BREAKDOWN_CONSISTENCY|backfill_currency_breakdowns" src src-tauri/src shared` → only migrations and tests of migrations.
- [ ] **Step 4:** Full gates. Commit `feat: retire CZK snapshot and ticker caches`.

### Task 25: ADR 0012 and living docs

**Files:**
- Create: `docs/architecture/decisions/0012-explicit-currencies-and-eur-rate-reference.md` (Status: Accepted; supersedes ADR 0001 except decimal TEXT storage, and ADR 0011's CZK cache clause; Context = audit summary; Decision = Global Constraints of this plan; Consequences = no CZK caches, EUR reference, main-currency restriction, MCP outputs).
- Modify: `docs/architecture/decisions/README.md` (index row, mark 0001/0011 partially superseded), `docs/architecture/database.md` (base-currency paragraph, rate tables, `portfolio_metrics_history`, value histories, `user_profile.currency`), `docs/architecture/overview.md` (CZK fallback statements), `docs/standards/typescript-frontend.md` (money formatting and conversion rules: `fmt.money`, `useCurrency().convert`, response currency, query keys with `fxRevision`), `docs/standards/rust-backend.md` (all conversion through `services/fx.rs`), `docs/playbooks/add-asset-domain.md` (native breakdown column, no CZK total), `docs/playbooks/add-tauri-command.md`, `docs/playbooks/add-db-migration.md` (remove "CZK-base aggregates"), `docs/DEPRECATED.md`, `AGENTS.md` rule 10 ("…reports and aggregates declare their currency; rates are stored against EUR"), `README.md` currency section (supported currencies, pegs).
- [ ] **Step 1:** Write/update; `rg -n "CZK is the (internal )?base|base currency" docs AGENTS.md README.md` → only historical specs/plans/ADRs 0001/0011.
- [ ] **Step 2:** Commit `docs: ADR 0012 explicit currencies and EUR rate reference`.

### Task 26: Final verification and merge

- [ ] Full gates on both sides; `cargo test generate_bindings -- --ignored` and `git diff --exit-code shared/generated-types.ts`.
- [ ] Migration dry run on a profile copy: if the demo profile database file exists (memory note `dev-bridge-background-throttling` names the `moony-demo` launch config), copy it into the scratchpad, open it with the app's migration runner in a throwaway test binary or `cargo test -- --ignored` helper that takes a path, and compare before/after: for every snapshot row with complete breakdowns, the CZK net worth computed before (old totals) equals the net worth recomputed from breakdowns at that day's rates within 0.01 CZK. Never touch the real profile.
- [ ] Independent whole-branch review (sonnet) against the spec acceptance list; fix wave; re-review.
- [ ] Record the outcome in `docs/audits/2026-10-09-multi-currency-audit.md` under a new "Implementation result" section (commits, verification numbers, accepted limitations).
- [ ] `git switch main && git merge --no-ff feat/multi-currency` (local only, no push).
