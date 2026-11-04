# Stock dividends — plan

Spec: `docs/specs/2026-10-30-stock-dividends-design.md`. Branch `feat/stock-dividends` in its
own worktree, commits per task, no merge, no push.

The first round (manual dividends with withholding tax, CSV import, MCP write) was built and
then reworked after the owner's rulings of 2026-10-30 (spec header). The rework:

1. **Revert the CSV dividend import** (stock import pipeline, wizard, export, `importWizard`
   copy) to the state before this branch.
2. **Migration 027 reshaped** (never released): `stock_dividend_events`,
   `stock_dividend_coverage`, `investment_dividends` without tax; schema snapshot, database.md,
   ADR 0022, FX lists in `fx_backfill.rs`.
3. **Types and service** (TDD): `StockDividendSource`, `StockDividend`, `InsertStockDividend`,
   `StockDividendList`; payouts from Yahoo events × shares held the day before; manual only
   without Yahoo dividends (refused otherwise, superseded when Yahoo arrives); totals; `payouts`
   for the return engine.
4. **Fetch**: `price_api::refresh_dividends` records the events since the first trade (daily,
   and when a trade predates the coverage); chart parsing tested.
5. **MCP**: read tool with source labels; write tool removed.
6. **UI**: source badges, Yahoo rows read-only, superseded rows, dialog with the Yahoo warning
   and no tax field, gross total; refreshes invalidate `['stock-dividends']`; ten locales.
7. **Demo seed** with Yahoo-shaped events.
8. **Gates, review, demo-profile verification.**
