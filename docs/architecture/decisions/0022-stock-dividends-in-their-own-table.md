# 0022. Stock dividends live in their own table, not as a transaction type

Date: 2026-10-30

## Status

Accepted.

## Context

The owner wants received dividends recorded as a new transaction type of a stock position, one
record per payout with its gross amount, so the GIPS return engine
(`docs/specs/2026-10-30-gips-stock-performance-design.md`) can read real payouts. Stock
transactions live in `investment_transactions` with `type` `buy` or `sell`, and more than forty
readers in Rust, SQL and TypeScript derive quantities as "a buy adds, anything else subtracts"
(history days and plan, ticker history, portfolio history, positions, cost basis, splits,
dated facts, FX backfill, stock performance, MCP, the CSV import, trade-event helpers, the
mobile ledger). A dividend also has no quantity or unit price: it is an amount, a tax withheld
from it and a currency.

Alternatives considered: `type = 'dividend'` in `investment_transactions` with the amount in
`price_per_unit` and a quantity of 1 or 0, and every reader changed to name `buy` and `sell`
explicitly (one missed reader, now or in later code, silently counts a dividend as a sale;
splits would rewrite its "price"); a generic cash-flow table shared with fees (no other use
yet).

## Decision

- **We store dividends in `investment_dividends`** (migration 027): one row per payout, FK to the
  position with `ON DELETE CASCADE`, `paid_date` (UTC midnight of the pay date), `gross_amount`
  and `withholding_tax` as decimal TEXT in `currency`, an optional note, and the import batch and
  broker id columns trades have.
- **`investment_transactions` stays buy/sell only.** Nothing that counts shares reads the new
  table, so no existing computation changes.
- **In the UI a dividend is a transaction of the position**: it is listed, filtered, edited and
  deleted with the buys and sells of the detail page, and imported from the same files.
- **One writer** (`services/stock_dividends.rs`) serves the detail page, the CSV import and MCP
  (ADR 0007).
- **Amounts are totals per payout in their own currency** (ADR 0012), never per share, so a split
  does not touch them, and they are converted only through `services/fx.rs`.

## Consequences

- Quantities, cost basis, gains, value history, splits and returns are safe by construction.
- Code that wants "every event of a position" (the detail list, an export, the GIPS engine) reads
  two tables and merges them by date.
- The table joins the FX lists (`services/fx_backfill.rs`), the undo of an import batch and the
  Moony export; a later cash event of a position (a fee, a return of capital) can follow the same
  pattern or extend this table.
