# Currency leaks: stop CZK showing through for non-CZK users

Date: 2026-10-04

## Context

CZK stays the internal base currency (ADR 0001). Generalising it is deliberately out of scope.
The user already picks a **main currency** in onboarding and in Settings, and the app converts
CZK aggregates into it for display. Several places skip that conversion and show the internal
CZK through to the user: CZK numbers labelled "CZK" next to euro values, a projection input in
CZK, texts naming CZK, a Czech-worded movement threshold, a CSV export in CZK, and MCP outputs
in CZK. This plan closes those leaks at the presentation and output layers only.

## Global constraints (bind every task)

- No change to storage, the DB schema, migrations or the wire contract (`shared/schema.ts`,
  `shared/generated-types.ts`). Backend aggregates stay CZK. If a task seems to need any of
  these, stop and report NEEDS_CONTEXT.
- Conversion follows the existing helpers: frontend `useCurrency()` (`convert`,
  `formatCurrency`, `currencyCode`) in `src/lib/currency.ts` / `src/lib/currency-provider.tsx`;
  backend `src-tauri/src/services/currency.rs`.
- Every user-facing string changes in BOTH `src/i18n/locales/en/` and `src/i18n/locales/cs/`.
- Rust: no `.unwrap()` outside tests, `AppError` / `crate::error::Result`, never `Result<_, String>`.
- Tests per `docs/standards/testing.md`: pure TS utils and Rust services (incl. `services/mcp/`)
  get tests; React components and Tauri command handlers do not.
- Conventional Commits; one logical change per commit; never `--no-verify`. Before every commit
  check `git branch --show-current` prints `fix/currency-leaks`.
- Gates before reporting done: `npm run lint && npm run typecheck && npm test`; for Rust changes
  also `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

### Task 1: Signed amounts and the cashflow CSV in the main currency

Items: leak 1 (CZK numbers labelled "CZK") and leak 5 (cashflow CSV export in CZK).

1. Add `formatCurrencySigned: (value: number) => string` to `CurrencyContextValue` in
   `src/lib/currency.ts` and implement it in `src/lib/currency-provider.tsx` next to
   `formatCurrency`: return `''` for null/undefined/NaN, otherwise convert the CZK value to the
   display currency with `convertFromCzK(value, currencyCode)` and format it with
   `fmt.money(converted, currencyCode, { signed: true, decimals: 0 })`. Doc comment in the style
   of `formatCurrency` ("Convert a CZK value to the display currency and format it with an
   explicit sign, 0 decimals.").
2. Replace every `fmt.money(<x>, 'CZK', { signed: true, decimals: 0 })` with
   `formatCurrencySigned(<x>)` at these sites (line numbers as of the plan date):
   - `src/components/other-assets/OtherAssetsTable.tsx` :254 (`r.gainCzk`), :264 (`r.yieldCzk`)
   - `src/pages/OtherAssets.tsx` :155 (`gain`), :165 (`totals.yield`)
   - `src/pages/RealEstate.tsx` :336 (`totals.value - totals.purchase`), :357 (`totals.rent`),
     :481 (`r.appreciation`)
   - `src/pages/Cashflow.tsx` :330 (`avgSavings`), :365 (`bar.a - bar.b`)
   - `src/pages/CashflowPlanning.tsx` :133 (the `signed` helper)
   - `src/pages/Projection.tsx` :342 (`delta`), :364 (`returns`)

   Before replacing, confirm each value is a CZK amount (trace it to its source). If one is not
   CZK, leave that site, and report it as a concern. Drop `fmt` / `useFormatters` imports only
   where they become unused. After the change,
   `grep -rn "'CZK', { signed" src` must return nothing.
3. Cashflow CSV export (`exportCsv` in `src/pages/Cashflow.tsx`): amounts are CZK. Write them in
   the display currency: convert each amount (`m.income`, `m.expenses`, `g.amount`,
   `g.previousAmount`) with `convert(value, 'CZK', currencyCode)` and round to 2 decimals
   (`Math.round(x * 100) / 100`). Header suffix `_czk` becomes `_${currencyCode.toLowerCase()}`
   (for example `income_eur`, `amount_eur`, `previous_eur`). Leave `month`, labels and
   `transactions` counts unchanged.
4. No new tests (React-only change). Run the gates.

Commit: `fix(ui): show signed CZK totals and the cashflow CSV in the main currency`.

### Task 2: Projection contributions entered in the main currency

Item: leak 2. Today `ProjectionParamsDialog` shows and stores the monthly contribution in CZK
with the unit label "CZK". The backend sums contributions as CZK (`get_settings_map` in
`src-tauri/src/commands/projection.rs`). That stays: the stored amount remains CZK and
`contributionCurrency` stays `'CZK'`. Only the dialog converts. The tested helper
`contributionToStore` in `src/utils/projection-assumptions.ts` already exists for this and is
currently unused.

1. In `ParamsForm` (`src/components/projection/ProjectionParamsDialog.tsx`), take
   `currencyCode` and `convert` from `useCurrency()`.
2. Initial draft: show each class's contribution (`p.contribution`, CZK; confirm by tracing
   `ClassParams` back to `resolveAssumptions`) converted to the display currency:
   `Math.round(convert(p.contribution, 'CZK', currencyCode)).toString()`. `initialDraft` must
   receive what it needs (e.g. a `toDisplay` function argument). Keep the initial draft around
   (e.g. in a `useState` initialiser or `useMemo`) so submit can compare against it.
3. Submit: for classes with `hasContribution`, `monthlyContribution =
   contributionToStore(draft.contribution, initialDraft.contribution, p.contribution, (v) =>
   convert(v, currencyCode, 'CZK'))`. For classes without a contribution keep `'0'`. Keep
   `contributionCurrency: 'CZK'` and update its comment: the amount is stored in CZK, the base
   currency, and the dialog converts from the display currency.
4. The contribution input's unit label (`<InputWrap unit="CZK" …>`) shows the display currency.
   Look at how other money inputs in the app label their unit (symbol or code) and do the same.
5. Hint text `projection.dialog.hint` in `src/i18n/locales/en/reports.json` and
   `src/i18n/locales/cs/reports.json`. Use these strings verbatim:
   - en: `"Contributions are added every month. Loans follow their own schedule and have no parameters."`
   - cs: `"Vklady se v projekci připisují každý měsíc. Úvěry se splácejí podle svého kalendáře a nemají parametry."`
6. Tests: `contributionToStore` is covered in `src/utils/projection-assumptions.test.ts`. If you
   extract any new pure helper, add a co-located test. Also update the stale doc comment on
   `ResolvedAssumption.contribution` / `AssumptionSummary.monthlyContribution` only if it is wrong
   (it says CZK, which stays true). Run the gates.

Commit: `fix(projection): enter monthly contributions in the main currency`.

### Task 3: Currency-neutral texts, form defaults and the movement floor

Items: leak 3 (texts), leak 4 (movement threshold wording), leak 7 (form defaults).

1. Texts. Use these strings verbatim (keep the JSON keys):
   - `src/i18n/locales/en/settings.json` `currency.noRateHint`:
     `"Exchange rates are updated automatically only for currencies the ECB publishes. Other currencies (for example AED or SAR) have no rate, so totals that include them are not accurate."`
   - `src/i18n/locales/cs/settings.json` `currency.noRateHint`:
     `"Směnné kurzy se aktualizují automaticky jen u měn, které zveřejňuje ECB. Ostatní měny (např. AED nebo SAR) kurz nemají, takže součty, které je obsahují, nejsou přesné."`
   - `src/i18n/locales/en/bank_accounts.json` `noRateTooltip`:
     `"There is no exchange rate for {{currency}}: the converted amount cannot be shown, and totals that include it are not accurate."`
   - `src/i18n/locales/cs/bank_accounts.json` `noRateTooltip`:
     `"Pro měnu {{currency}} není směnný kurz: přepočtenou částku nelze zobrazit a součty, které ji obsahují, nejsou přesné."`
   - `src/i18n/locales/cs/auth.json` `preview.empty`: `"— Kč"` becomes `"—"` (the en value is
     already `"—"`).
2. Form defaults: `src/components/other-assets/AddOtherAssetModal.tsx` (`currency: 'CZK'`
   default) and `src/components/other-assets/EditOtherAssetModal.tsx` (`defaultValues` with
   `currency: 'CZK'`) default to the user's main currency, `useCurrency().currencyCode`, like
   `BankAccountFormDialog` / `LoanFormDialog` already do. In the edit modal the asset's own
   currency must still win when the form is reset with the asset. Check that it does.
3. Movement floor (behaviour unchanged): in `src/utils/bank-activity.ts` export
   `MOVEMENT_FLOOR_BASE = 10_000` with a doc comment: the absolute floor in the base currency
   (CZK, roughly 400 EUR), converted to the account currency by the caller. Rename the
   `movementThreshold` parameter `tenThousandInCurrency` to `floorInCurrency` and reword its doc
   comment so it no longer says "10 000 Kč": "the base-currency floor (MOVEMENT_FLOOR_BASE) in
   the account currency, or 10 % of the balance, whichever is larger". In
   `src/pages/BankAccountDetail.tsx` replace `convert(10_000, 'CZK', accountCurrency)` and the
   `tenThousandInAccount` name with `convert(MOVEMENT_FLOOR_BASE, 'CZK', accountCurrency)` /
   `floorInAccount`. Update any tests of `movementThreshold` for the rename only.
4. `docs/design-system/README.md`: the sentence "Bank accounts mark only movements above a
   threshold (10 000 Kč or 10 % of balance)" becomes "Bank accounts mark only movements above a
   threshold (an absolute floor of about 400 EUR, or 10 % of balance)".
5. Run the gates (`npm test` must still pass, including the i18n parity tests if present).

Commits: `fix(i18n): currency-neutral no-rate texts` and
`fix(ui): main-currency defaults and a neutral movement floor` (or one commit per item).

### Task 4: MCP money context and current-value tools in the main currency

Item: leak 6, part 1. The MCP server (`src-tauri/src/services/mcp/`, embedded per ADR 0006)
reports amounts in CZK with `*_czk` / `*Czk` keys. It re-implements conversion in 5 inline
closures that read the `exchange_rates` table and fall back to `1.0`:
`portfolio.rs` `portfolio_metrics` and `analytics.rs` (4 closures). An AI client therefore talks
to a EUR user in CZK.

1. Add `src-tauri/src/services/mcp/money.rs` (registered in `mcp/mod.rs`) with:
   - `pub struct MoneyContext { main: String, rates: HashMap<String, f64> }`
   - `pub fn load(conn: &Connection) -> Result<MoneyContext>`: rates from
     `SELECT currency, rate FROM exchange_rates` (X → CZK per unit) with `CZK = 1.0` inserted;
     main currency from `SELECT currency FROM user_profile LIMIT 1`, trimmed and uppercased.
     Fall back to `"CZK"` when there is no row or the value is empty.
   - `pub fn main_currency(&self) -> &str`
   - `pub fn to_czk(&self, amount: f64, currency: &str) -> f64`: delegate to
     `crate::services::currency::convert_to_czk_with_rates(amount, currency, &self.rates)`.
   - `pub fn czk_to_main(&self, czk: f64) -> f64`: `czk` when main is CZK, otherwise
     `czk / rate(main)`. When main has no usable rate (missing, or <= 0), use the same
     last-resort semantics as `currency::convert_from_czk` (its fallback via
     `get_exchange_rate`), so the MCP agrees with the app.
   - Unit tests with an in-memory DB per the repo pattern (hand-written minimal
     `exchange_rates` + `user_profile` tables): main EUR with rate 25 gives `czk_to_main(2500.0)
     == 100.0`; no profile row gives main `"CZK"` and identity; `to_czk` uses table rates.
2. Replace the 5 inline closures with a `MoneyContext` loaded once per tool call. Intermediate
   arithmetic stays in CZK.
3. Outputs of `portfolio_metrics` (`mcp/portfolio.rs`) and every analytics tool in
   `mcp/analytics.rs` that emits CZK amounts: convert each amount with `czk_to_main` at
   serialisation time. Drop the CZK suffix from the key (`net_worth_czk` → `net_worth`,
   `total_assets_czk` → `total_assets`, `savings_czk` → `savings`, `monthly_czk` → `monthly`,
   `amount_czk` → `amount`, `budgetCzk` → `budget`, `actualCzk` → `actual`, `remainingCzk` →
   `remaining`, `currentValueCzk` → `currentValue`, `costBasisCzk` → `costBasis`, `gainLossCzk` →
   `gainLoss`, `annualDividendCzk` → `annualDividend`, and so on for every `*_czk` / `*Czk` key in
   these two files). Add a top-level `"mainCurrency": <code>` to each such response. Remove
   `"note": "All values in CZK."`-style notes. Percentages and ratios stay as they are (they are
   currency-invariant). Keep the money formatting (`format!("{:.2}", …)` strings) as today.
4. Tool descriptions in `mcp/mod.rs` for the tools changed here ("All monetary values are in CZK"
   and similar) say amounts are in the user's main currency, given as `mainCurrency`. Server
   instructions (`get_info`): replace "All monetary values are in CZK unless a currency field says
   otherwise" with "Monetary values are in the user's main currency (the `mainCurrency` field)
   unless a currency field says otherwise".
5. Update the existing MCP tests for the renamed keys. Add at least one test per changed file
   showing a EUR main currency converts the output (e.g. `portfolio_metrics` net worth) and
   reports `mainCurrency: "EUR"`. Note: the global in-memory rates in `currency.rs` hold built-in
   defaults, so a currency missing from the test table may hit those instead of `1.0`. Put the
   currencies a test needs into its table.
6. Run the Rust gates.

Commit: `fix(mcp): report current values in the user's main currency`.

### Task 5: MCP history tools, rates tool and write defaults

Item: leak 6, part 2. Depends on Task 4's `MoneyContext` (`src-tauri/src/services/mcp/money.rs`).

1. `portfolio_history` (`mcp/portfolio.rs`): the snapshot totals are CZK. Convert each row's
   amounts into the main currency at that row's day rate (ADR 0001: historical values use the
   rates of their day). Use `crate::services::currency::get_rates_for_date(conn, recorded_at)`
   per row, or the range variant plus a nearest-earlier lookup if a helper for that already
   exists and is reachable. Skip conversion when main is CZK. Per-currency breakdown JSON
   fields, if returned, stay native. Add top-level `"mainCurrency"`.
2. Ticker history in `mcp/investments.rs` and `mcp/crypto.rs`: row key `valueCzk` becomes
   `value`, converted to the main currency at the row's day rate (`recorded_at`). The row's
   native `price` / `currency` fields stay unchanged. Add top-level `"mainCurrency"`.
3. `exchange_rates_list` (`mcp/exchange_rates.rs`): keep the raw rates and
   `"baseCurrency": "CZK"`, add `"mainCurrency"`, and set the note to: "Rates are Moony's
   internal CZK pivot: multiply an amount by its currency's rate to get CZK. Other tools already
   report amounts in mainCurrency."
4. Write tools whose currency argument is optional and today falls back to CZK (at least bank
   account create, described "defaults to CZK" in `mcp/bank_accounts.rs`; bonds create and real
   estate create, described "default CZK" in `mcp/mod.rs`; check insurance and other assets
   too): default a missing currency to the user's main currency in the MCP layer, before calling
   the shared service. Do not change the services' own defaults, which the UI path shares
   (ADR 0007). Update those descriptions to "defaults to the user's main currency".
5. Descriptions in `mcp/mod.rs` for the history tools ("Values are in CZK", "totals in CZK")
   say values are in the user's main currency (`mainCurrency`) at each day's rate; the
   exchange-rates tool description says rates are relative to Moony's internal base CZK.
6. Tests: update the existing ones for renamed keys. Add tests for day-rate conversion in history
   (two days with different `exchange_rate_history` rates convert differently) and for the
   main-currency default of at least one write tool. Run the Rust gates.

Commit: `fix(mcp): history, rates and write defaults follow the main currency`.
