# Report currencies implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development; sequential sole writers and fresh task reviews.

**Goal:** Cashflow, budgeting and planning report native financial facts directly in the user's chosen currency, with quiet normal-state UX.
**Architecture:** Native source data feeds target-aware services. Historical reports use historical pair rates; planning uses current rates. Responses declare currency, and frontend formats without another conversion. CZK remains permitted as an explicit derived cache in unrelated portfolio interfaces.
**Tech Stack:** Tauri2/Rust/rusqlite, React19/TypeScript, TanStackQuery, Vitest.
**Spec:** docs/specs/2026-10-08-report-currencies-design.md
**BASE:** 0d50bab. Reuse current codex/money-flows-readiness checkout; pre-existing dirty MCP import files are not ours.

## Global Constraints

- Native money is decimal TEXT; epoch INTEGER and UUID TEXT. CZK is permitted as a rate reference or explicitly denominated derived cache, not a compulsory report unit.
- Thin commands, services take &Connection/AppError, invoke only tauri-api. Contract mirrored Rust/schema/generated with registered types.
- Both cs/en; existing design tokens; no private DB, push/merge/install or destructive migrations.
- Historical source/target rates share a date and max10day lookback. Same-currency bypass works with no FX data. No current fallback/implicit1:1.
- Preserve financial resolver precedence, booked-only actuals, signed refunds, scopes, provenance, versioned goal amounts and replacement rules.
- Preserve pre-existing dirty README.md, docs/architecture/database.md, services/mcp/bank_accounts.rs, services/mcp/mod.rs and MCP import docs. Do not stage their unrelated edits.

## Review Focus

- Target currency changes while requests are in flight: never relabel old amounts; query isolation and response currency.
- Same-currency non-provider profile (AED), no rates: accurate report without warning or CZK fallback.
- Mixed property costs and automatic replacements: no double conversion/phantom totals; retain native originals.
- Cross rates with nonmatching dates and JPY/KWD: explicit missing results or common-day precision; detail and total reconcile.
- Existing MCP consumers and CZK projection contracts: report currency explicit, no double-conversion or silent changed units.

## Task 1: Target-aware financial report engine and contracts

**Files:** services/reporting.rs, cashflow_actuals.rs, budgeting.rs, cashflow_planning.rs; corresponding models/commands; services/mcp/analytics.rs; bindings.rs, shared/schema.ts, shared/generated-types.ts, src/lib/tauri-api.ts. Minimal compile-only frontend field renames may accompany contract changes.
**Interfaces:** Add target-aware functions such as reporting::historical_amount(conn, amount, from, target, day), transactions_in_currency/filtered_transactions_in_currency; actuals::get_actuals_in_currency; budgeting::get_report_in_currency; planning::get_report_in_currency. Legacy CZK helpers may wrap these with explicit CZK where needed. Tauri commands accept optional report_currency default CZK for source compatibility; typed frontend wrappers accept reportCurrency. Reports/rows expose reportCurrency, signedAmount, budgetLimit in that unit (no misleading Czk names). MCP uses profile currency explicitly.

- [ ] Write behavioral service regressions before implementation, demonstrate failure: EUR100 remains100 with empty rate table; USD110/EUR pair rates25/27.5 yields100 at a common historical day, changing current rates leaves it100; separate-date-only quotes produce missing; AED same-currency works; KWD0.123 retained and JPY rounded consistently; missing FX rows remain visible.
- [ ] Implement shared target conversion, validation, precision and report services. Keep SQL errors propagated, same-currency branch before DB/rate access, no intermediate rounding. Tests use `historical_amount(&conn, 100.0, "EUR", "EUR", day)` and existing minimal service DBs; expected Some100 without any rate insertion.
- [ ] Cover budgeting versions and EUR goal100 vsEUR spend25 =>25%, changing target does not mutate stored goal. Report and drilldown sum equality, current/comparison unchanged after live-rate change.
- [ ] Cover planning with EUR yearly1200=>monthly100 and noFX; mixed-native propertycosts, replacement inventory and missing target; per-currency target formatting of response TEXT. MCP parity through actual service calls, not mocked helpers.
- [ ] Mirror contracts/wrappers, regenerate bindings, update all impacted callers mechanically to compile; UI remains defaultCZK untilTask2. Covering Rust service tests, cargo check, typecheck and diffcheck pass; commit only owned files.

## Task 2: Currency-native UI, quiet report experience and architecture

**Files:** pages/Cashflow.tsx, CashflowPlanning.tsx, Budgeting.tsx; components/reports/*; budgetgoal/categorydialogs; cashflowitemdialog; lib/use-report-money.ts, currency-provider.tsx; report utilities and cs/en locale files. Architecture ADR0011 +index, AGENTS currencyrule, current currency/design standards. Controller may own docs while sole worker owns code.
**Consumes:** Task1 reportCurrency/signedAmount/budgetLimit and optional reportCurrency request args.
**Produces:** All three pages, drilldowns and exports request/format the intended target; query keys include currency/rate revision. Goal suggestions request goal currency. No default successful-report currency-note block.

- [ ] Add pure utility regressions for currency-key/export/format contracts where a meaningful utility boundary exists. Native100EUR report exports100, not an exchange-rate calculation; JPY/KWD export precision preserved. Do not add component/hook tests.
- [ ] Update queries/formatters and dialogs; use returned currency to prevent stale-response relabel. No current FX dependency for historical arithmetic, but refresh revision may refetch newly available history.
- [ ] Remove repetitive technical currency notes and obsolete translations, retain compact material-incompleteness warnings and useful average sample context. Preserve native editor values and scheduled versions.
- [ ] Document explicit native/target/reference/cache distinctions, supersede ADR0001 reporting decision without rewriting it. Existing investment/portfolio cache interfaces remain explicitly CZK; inventory unrelated existing debt in final audit without claiming it is solved.
- [ ] npm lint/typecheck/test/build/format and locale parity. Commit owned changes; independent task review and scoped fixes.

## Task 3: Integrated verification

- [ ] Full npm gates; cargo fmt/check/clippy alltargets-Dwarnings/test; bindings and schema drift. Only local-port test escalation if needed.
- [ ] Isolated visual mockIPC: EUR profile with EUR actuals/limits/planning, no rates; mixed missingFX warning; profilecurrency switch and detail/export; ordinary pages have no persistent FX explanations. Preserve installed private DB.
- [ ] Whole-change review from BASE0d50bab on capable model. One consolidated finalfix wave plus scoped rereview. Record rulings/limitations and exact test evidence in docs/audits/2026-10-08-report-currencies.md.
- [ ] Keep local reviewable branch, no push/merge/install. Concise Czech final with outcome, evidence and material limits.
