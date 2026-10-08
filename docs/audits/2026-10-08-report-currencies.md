# Report currency audit — 2026-10-08

Implemented through `c866a8f`; final whole-change review pending. This record covers the follow-up currency architecture and report UX change, following the earlier money-flow readiness audit.

## Result

Actual cashflow, budgeting, planned cashflow, report drilldowns and CSV export use an explicit target currency. MCP budgeting and planning use the same services. Native transaction, account, contract and budget values remain authoritative; changing the profile currency does not rewrite them. No database migration is required.

Same-currency values need no exchange-rate data, including currencies outside the rate provider. Historical foreign amounts use a common source/target quote date on or before booking, with at most ten days of lookback. Changing current rates does not change past actuals. Planning uses one current rate snapshot. Missing conversion remains explicit and never silently becomes 1:1 or a different report currency. Monetary wire values remain decimal TEXT.

The three pages format the response currency directly. Query identities contain the target currency and rate revision. Normal results have no technical FX disclaimer; incomplete results have a concise warning. Composite property costs preserve their native components, known partial subtotals and converted-component count. A completely unavailable composite reads unavailable; a known zero subtotal remains visible with an incomplete marker.

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
- Full Rust execution includes preserved concurrent MCP bank-import working-tree changes, which are outside this feature's committed diff.

## Isolated UI checks

Synthetic local mock IPC, without the installed private database, verified:

- EUR cashflow: income 500, expenses 100, net 400; graph and detail agree and requests explicitly ask for EUR.
- AED cashflow without quotes: values remain AED with no fallback warning.
- KWD 0.123 is shown with three decimals and preserved in the CSV payload. EUR CSV contains 100 and 500 directly. The actual export button completed through mocked Tauri save/write APIs; no real file was written. Browser download-event testing was inapplicable to this native export mechanism and did not produce evidence.
- Missing USD conversion: a compact incomplete warning, preserved native USD transaction and unavailable report contribution. Budget usage is unavailable, with no healthy progress, while the known EUR 200 limit remains visible.
- EUR-to-USD profile change with a delayed response: no old EUR amount is relabeled USD; the completed cashflow shows expenses 110 USD. The EUR budget still shows 100/200 EUR and 50%; its drilldown and editor suggestion request EUR.
- Planning: native annual EUR 1,200 appears as monthly 100 and yearly 1,200. A composite known EUR 60 plus missing USD component retains 60 with an incomplete marker; all-missing composite is unavailable; known zero plus missing component shows zero as an incomplete subtotal.
- No console errors or warnings in the checked mock UI session.

The mock proves frontend request/render/export wiring. Service regressions independently verify the financial calculations; the mock does not substitute for them.

## Boundaries and follow-up inventory

CZK remains a permitted rate reference and an explicitly denominated derived cache in existing portfolio/projection contracts. This change does not migrate every investment valuation, tax calculation or history endpoint. The independent inventory identified existing projection contribution-currency handling and legacy portfolio/MCP history conversion/fallback behavior for a separate targeted change; they are not claimed fixed here.

Deferred for final review: accepted-code minor-unit mapping beyond the picker (for example JOD/UGX), and existing settings copy describing every value as converted despite native budget rows. Official ISO 4217 precision data was checked against [SIX's current list](https://www.six-group.com/dam/download/financial-information/data-center/iso-currrency/lists/list-one.xml), published 2026-09-17; SIX is the [maintenance agency](https://www.six-group.com/en/products-services/financial-information/market-reference-data/data-standards.html).

No installed private SQLCipher database was opened. No production installer was built or installed. No push or merge is part of this task. Unrelated dirty README/database/MCP import files and import documents were preserved.
