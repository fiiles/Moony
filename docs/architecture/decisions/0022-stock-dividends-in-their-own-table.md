# 0022. Stock dividends: Yahoo events and manual payouts in their own tables

Date: 2026-10-30

## Status

Accepted.

## Context

The GIPS return engine (`docs/specs/2026-10-30-gips-stock-performance-design.md`) needs the
dividends each stock position received, gross, on their day. The owner ruled that Yahoo Finance is
the source, that a dividend is entered by hand only for a ticker Yahoo has none for, that broker
CSV files never bring dividends (a payout imported and then fetched would count twice), and that
every dividend shows its source.

Stock transactions live in `investment_transactions` with `type` `buy` or `sell`, and more than
forty readers in Rust, SQL and TypeScript derive quantities as "a buy adds, anything else
subtracts" (history, positions, cost basis, splits, dated facts, FX backfill, stock performance,
MCP, the CSV import, trade-event helpers, the mobile ledger). A dividend has no quantity or unit
price.

Alternatives considered: `type = 'dividend'` in `investment_transactions` (one missed reader, now or
in later code, counts a dividend as a sale; splits would rewrite it); storing each position's Yahoo
payout as a row (a backdated trade would leave it stale).

## Decision

- **Yahoo's dividends are stored per ticker and per share** (`stock_dividend_events`, with
  `stock_dividend_coverage` for the fetched range). A position's payout is the amount per share
  times the shares it held the day before the ex-date, derived on every read.
- **Manual payouts live in `investment_dividends`**, one row per payout of a position (gross total,
  currency, ex-date), only for tickers Yahoo has no dividends for; once Yahoo has any, the manual
  ones are superseded and counted nowhere.
- **`investment_transactions` stays buy/sell only.** Nothing that counts shares reads dividends.
- **Amounts are gross, in their own currency, on the ex-date**, converted only through
  `services/fx.rs` (ADR 0012).
- **One service** (`services/stock_dividends.rs`) serves the detail page, MCP (read only) and the
  return engine (`payouts`).

## Consequences

- Quantities, cost basis, gains, value history, splits and returns are safe by construction.
- A payout is never counted twice: per ticker either Yahoo or manual counts.
- Code that wants every event of a position reads trades and dividends and merges them by date.
- Payouts depend on the share history, so every trade change refreshes the dividend views.
- Without network the stored events stay; a ticker never fetched shows no Yahoo dividends yet.
