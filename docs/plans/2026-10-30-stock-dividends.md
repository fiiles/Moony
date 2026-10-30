# Stock dividends — plan

Spec: `docs/specs/2026-10-30-stock-dividends-design.md`. Branch `feat/stock-dividends` in its
own worktree, commits per task, no merge, no push.

1. **Data model.** Migration `027_stock_dividends` (D2), regenerate `schema.snapshot`,
   `docs/architecture/database.md`, ADR 0022 (own table). FX: add the table to the currency
   lists of `services/fx_backfill.rs` (held currencies, dated facts, converted sources).
2. **Types.** `models/stock_dividends.rs`: `StockDividend`, `InsertStockDividend` +
   `validate()`, `StockDividendList`; register in `bindings.rs`; mirror in `shared/schema.ts`
   (7-step procedure); regenerate `generated-types.ts`.
3. **Service** `services/stock_dividends.rs` (TDD): list with report-currency totals, create,
   update, delete, `bulk_create` (dedup rules 1–2, one-to-one, import anyway, batch stamp) used
   by import and MCP. Tests: validation, CRUD, totals (day rate, estimated, missing), cascade,
   dedup, and that a dividend leaves quantities, metrics, `replayed_quantities`, stock
   performance quantities and split rewrites untouched.
4. **Commands + API.** `commands/stock_dividends.rs`, register in `lib.rs`; `tauri-api.ts`
   `stockDividendsApi`.
5. **Detail page.** `use-stock-dividend-mutations.ts` (rule 7 + `stock-performance`),
   `StockDividendModal` (add/edit), dividend rows in the StockDetail transaction list + filter,
   hero total; all ten locales.
6. **Import backend.** Types (actions, config columns, preview row kind/amount/tax,
   `unpairedTaxes`, `dividendCount`, `needsCashCurrency`), classifier keywords, parse dividend
   and tax rows, pairing, simulate (position rule, dedup), preview, import write + batch count,
   undo; adapters Trading 212, XTB, IBKR cash transactions, Degiro account statement, Moony.
   Tests per step.
7. **Import UI.** Type-value actions, mapping of amount/tax columns, review rows for dividends
   and tax, unpaired-tax warning, account-currency picker; locales.
8. **Export.** Moony CSV with `Amount`, `Withholding tax` columns and dividend rows.
9. **MCP.** `stock_dividends_create`, `stock_dividends`, dividends in `investment_detail`.
10. **Demo seed.** A few dividends (one with tax, one in another currency).
11. **Gates, code review, demo-profile verification** (port 1421, Alex greeting first).
