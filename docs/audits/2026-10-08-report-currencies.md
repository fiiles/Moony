# Report currency audit — 2026-10-08

Implemented through the final fix wave (`fdd6e62`, `fa7b447` and the audit commit). Final whole-change review: **approved**. This record covers the follow-up currency architecture and report UX change, following the earlier money-flow readiness audit.

## Result

Actual cashflow, budgeting, planned cashflow, report drilldowns and CSV export use an explicit target currency. MCP budgeting and planning use the same services. Native transaction, account, contract and budget values remain authoritative; changing the profile currency does not rewrite them. No database migration is required.

Same-currency values need no exchange-rate data, including currencies outside the rate provider. Historical foreign amounts use a common source/target quote date on or before booking, with at most ten days of lookback. Changing current rates does not change past actuals. Planning uses one current rate snapshot. Missing conversion remains explicit and never silently becomes 1:1 or a different report currency. Monetary wire values remain decimal TEXT.

The three pages format the response currency directly, at the ISO 4217 minor unit of the money contract, so rows, totals, drilldowns and CSV reconcile (CZK 100.40 + 100.40 reads 200.80, not 100 + 100 = 201). The everyday cash style outside reports (CZK/HUF without decimals) is unchanged. Query identities contain the target currency and rate revision. Normal results have no technical FX disclaimer; incomplete results have a concise warning. Composite property costs preserve their native components, known partial subtotals and converted-component count. A completely unavailable composite reads unavailable; a known zero subtotal remains visible with an incomplete marker.

## Decision made during implementation

Budget utilization is evaluated in the goal's period-selected currency, independent of profile/report currency. Converting the limit at month start and spending at booking into another currency could otherwise turn the same EUR budget from 50% into 60% used. Native spending, limit and authoritative percentage prevent this. A missing native conversion makes usage unavailable. Overview expenses use the report currency, while budget status counts avoid adding native EUR/USD limits.

The tradeoff is that a budget row can have a different visible currency from the report headline, and the services perform additional historical conversions grouped by budget currency. Reversing this decision would require a different meaning for budget utilization; it cannot be achieved by relabeling the same amount.

## Verification

- Backend RED regressions reproduced same-currency EUR/AED failures without rates, historical USD/EUR cross conversion, native actuals/budgets/planning, profile-dependent budget utilization, and quality contamination by a replaced missing automatic source.
- Independent Task1 review found intermediate monthly limit rounding. Three no-FX regressions reproduced yearly JPY 100 becoming 96, quarterly EUR 100 becoming 99.99 and yearly EUR 100 becoming 99.96. The fix rounds the completed period once; all 32 budget service tests passed and scoped re-review approved it.
- Independent Task2 review found ambiguity between all-missing and known-zero composite planning amounts. A service regression reproduced the absent availability signal. The fix adds `convertedComponentCount`; all 11 planning service tests passed and scoped re-review approved it. The budget unavailable-usage badge was also corrected to avoid blaming an available limit.
- Full `cargo test --manifest-path src-tauri/Cargo.toml` at `c866a8f`: 1,488 library tests passed, four intentionally ignored; auxiliary suites seven and ten passed; one doc test passed. Local HTTP fixtures ran with local-socket permission. No warning lines in captured output.
- Rust check, clippy all targets with warnings denied, cargo fmt and binding generation passed. Generated types matched the checked-in contract. Normal commit hooks passed.
- Frontend lint, typecheck, format check, build and `npm test` passed: 74 files, 996 tests. Both modified locale pairs parse and have matching keys, allowing Czech plural variants. The build retains pre-existing large-chunk and ineffective dynamic-import warnings; these remain a separate build-maintenance item.
- Final gates after the last follow-up (including the audit commit's code): full `cargo test --manifest-path src-tauri/Cargo.toml` passed 1,491 library tests (four intentionally ignored) plus the seven, ten and one doc-test suites, with local-socket permission and no warning lines; clippy all targets with warnings denied, `cargo fmt --check` and binding generation passed, and `shared/generated-types.ts` has no drift (no wire types changed in the fix wave). Frontend lint, typecheck, format check, build and `npm test` (74 files, 999 tests) passed; settings, budgeting and reports locales have matching cs/en keys. Normal commit hooks passed.
- Full Rust execution includes preserved concurrent MCP bank-import working-tree changes, which are outside this feature's committed diff.

## Final review and fix wave

The interrupted whole-change review (`0d50bab..9faee5a`) was completed and its findings fixed in one wave (`fdd6e62`, `fa7b447`, and two minor follow-ups committed with this audit), then independently re-reviewed. Final verdict: approved, with the accepted limitations below.

- Important, report precision: reports used the cash formatter (CZK, COP, HUF, IDR, TWD 0 decimals) while backend and CSV keep ISO decimals. `formatReportMoney`/`useReportMoney` now apply `isoMinorUnits` to report amounts, native budget values, drilldown originals, planning components and the budget dialog's observed average and scheduled limits. Chart axes stay compact. A regression reconciles two CZK 100.40 rows, the 200.80 total and the CSV, and asserts the unchanged cash style elsewhere.
- Confirmed, budget transaction count: rows without a report quote were skipped before counting, so a native EUR budget with valid usage in a USD report without quotes showed "no transactions" and a disabled drilldown. Category, uncategorized, non-consumption and cashflow group counts now include all relevant rows of the period; RED regressions reproduced 0 instead of 2 (budget), 0 instead of 1 (uncategorized) and 1 instead of 2 (cashflow group). A group with only unconverted rows is still not created as a healthy-looking zero; the report-level incomplete warning and drilldown show those rows. An older expectation (`uncategorizedTransactionCount` 4 beside an expense count of 3 that already included the unquoted row) encoded the inconsistency and now reads 5.
- ISO minor units: the backend mapped only eight exceptions. `shared/iso4217-minor-units.json`, generated from SIX List One published 2026-09-17, now drives both `reporting::minor_units` (embedded at build time) and the frontend `isoMinorUnits`; the reviewer confirmed all 26 numeric non-2 entries (for example JOD/TND 3, UGX/XOF 0, CLF/UYW 4). Codes without an ISO minor unit (XAU, XDR, XXX) and unlisted syntactically valid codes keep two decimals. Contract precision stays distinct from the UI cash style.
- Settings copy no longer says every value is converted: summaries, asset overviews and reports use the main currency, and entered amounts of accounts, transactions, contracts and budgets keep their original currency. The reviewer pointed out that loan and insurance tables display contract amounts converted, so the copy describes stored values rather than every table.
- Build warnings, assessed and not changed: the main chunk is 2.61 MB (739 kB gzip) and `heic2any` 1.35 MB is already lazy. Four ineffective dynamic imports concern small Tauri API/plugin modules that are also imported statically. In a desktop app loading local assets the cost is parse time at startup, not network. Route-level lazy pages and removing the redundant dynamic imports are a separate maintenance task.

Accepted limitations: a category mixing converted and unconverted rows shows a partial report total next to its full count, without a per-row marker; the global incomplete warning covers it. An all-unconverted uncategorized expense row keeps the existing "Uncategorized 0 · N" presentation. Monthly non-consumption flow counts on the cashflow page still count converted rows only. Re-review minors fixed with this audit: budgeting `nonconsumptionFlows[].count` (also in MCP `budgeting_report`) counts unconverted rows, and the Budgeting page no longer shows the "no transactions, import a statement" empty state when the period only has unconverted rows.

## Isolated UI checks

Synthetic local mock IPC, without the installed private database, verified:

- EUR cashflow: income 500, expenses 100, net 400; graph and detail agree and requests explicitly ask for EUR.
- AED cashflow without quotes: values remain AED with no fallback warning.
- KWD 0.123 is shown with three decimals and preserved in the CSV payload. EUR CSV contains 100 and 500 directly. The actual export button completed through mocked Tauri save/write APIs; no real file was written. Browser download-event testing was inapplicable to this native export mechanism and did not produce evidence.
- Missing USD conversion: a compact incomplete warning, preserved native USD transaction and unavailable report contribution. Budget usage is unavailable, with no healthy progress, while the known EUR 200 limit remains visible.
- EUR-to-USD profile change with a delayed response: no old EUR amount is relabeled USD; the completed cashflow shows expenses 110 USD. The EUR budget still shows 100/200 EUR and 50%; its drilldown and editor suggestion request EUR.
- Planning: native annual EUR 1,200 appears as monthly 100 and yearly 1,200. A composite known EUR 60 plus missing USD component retains 60 with an incomplete marker; all-missing composite is unavailable; known zero plus missing component shows zero as an incomplete subtotal.
- After the fix wave, CZK scenario: cashflow categories 100,40 Kč + 100,40 Kč with total 200,80 Kč; the exported CSV payload contains 100.40, 100.40 and 200.80; the budget row shows 100,40 Kč of 200,00 Kč and 50 %; the drilldown shows −100,40 Kč in both columns; the budget dialog average reads 100,40 Kč. KWD still shows 0,123 KWD; planning originals read 1 200,00 € and 1 200,00 $ with the incomplete composite unchanged. The settings hint reads as intended in Czech.
- No console errors or warnings in the checked mock UI sessions.

The mock proves frontend request/render/export wiring. Service regressions independently verify the financial calculations; the mock does not substitute for them.

## Boundaries and follow-up inventory

CZK remains a permitted rate reference and an explicitly denominated derived cache in existing portfolio/projection contracts. This change does not migrate every investment valuation, tax calculation or history endpoint. The independent inventory identified existing projection contribution-currency handling and legacy portfolio/MCP history conversion/fallback behavior for a separate targeted change; they are not claimed fixed here.

Official ISO 4217 precision data comes from [SIX's current list](https://www.six-group.com/dam/download/financial-information/data-center/iso-currrency/lists/list-one.xml), published 2026-09-17; SIX is the [maintenance agency](https://www.six-group.com/en/products-services/financial-information/market-reference-data/data-standards.html).

No installed private SQLCipher database was opened. No production installer was built or installed. No push or merge is part of this task. Unrelated dirty README/database/MCP import files and import documents were preserved.
