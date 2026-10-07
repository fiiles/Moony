# Task 5 report: language consistency and Vitest cleanup

## Changes

- Added Czech and English plural forms for transaction counts, pending promotions, imported-batch deletion, average sample coverage, and missing-FX counts. Zero-count messages are explicit where the UI can show a count. The averages now show the sample explanation once in the report quality note, and explain why they are unavailable when there are no observed completed months with an available rate.
- Reworded the Czech drilldown caption to describe the signed report effect without implying a digital signature. Simplified the absolute-share heading while retaining that its denominator is the sum of absolute values.
- Changed the CSV preview back action to “Upravit sloupce” / “Edit columns”; it returns to the mapping step.
- Kept the bank ledger’s native Incoming/Outgoing labels distinct from report Income/Expense wording. The import preview passes the row currency, falling back to the selected account currency, to `formatPreviewAmount`; that formatter uses the shared format utilities while preserving source precision, including currencies normally displayed with zero decimals.
- Applied the browser bridge plugin only outside Vitest’s `test` mode. The development relay remains enabled in the Vite development mode. Vitest’s configuration documentation supports conditional Vite configuration by mode or `process.env.VITEST`.

## Verification

- `npm test` — passed: 72 test files, 983 tests; fresh run exited in 986 ms with no Vite server-close timeout or warning.
- `npm run typecheck` — passed.
- `npm run lint` — passed.
- `npx prettier --check vite.config.ts src/components/bank-accounts/CsvImportDialog.tsx src/components/reports/ReportQualityNote.tsx src/pages/Cashflow.tsx src/i18n/locales/cs/bank_accounts.json src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/reports.json src/i18n/locales/en/reports.json` — passed.
- Parsed both changed locale namespaces in both languages and compared nested keys. All normal keys match; Czech additionally has the expected `_few` plural variants.
- `git diff --check` — passed.

## Remaining review

No structural issue from this bounded language/configuration task needs controller routing. The known build dynamic/static-import and chunk warnings were not re-evaluated here; Task 6 can triage them during its build gate.

## Independent review fix round 1

Addressed both Important findings from `task-5-review.md`:

- Changed the bank-detail `incomeCount` labels to “incoming movement(s)” / “příchozí pohyb(y)” so transfers and refunds are described by movement direction rather than income classification.
- Added English and Czech zero/one/few/other variants for `readiness.excludedForeign` and `readiness.comparisonFx`; Czech one-count forms now use “pohyb”. Existing call sites pass `count` and resolve these standard i18next plural keys.

Exact verification:

- `npm test` — passed: 72 files, 983 tests, 1.26 s; no Vite close warning.
- `npm run typecheck` — passed.
- `npm run lint` — passed.
- `npx prettier --check src/i18n/locales/cs/bank_accounts.json src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/reports.json src/i18n/locales/en/reports.json` — passed.
- Node JSON parse and nested-key comparison for `bank_accounts` and `reports` — passed; Czech-only `_few` forms are expected locale plural categories.
- `git diff --check` — passed.
