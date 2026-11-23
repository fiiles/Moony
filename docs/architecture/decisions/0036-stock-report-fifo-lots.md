# 0036. Stock transactions report: oldest-first lots derived on read

Date: 2026-11-23

## Status

Accepted. Adds a derivation beside, not instead of, the weighted-average cost basis of
`services/cost_basis.rs`; conversion follows
[ADR 0012](0012-explicit-currencies-and-eur-rate-reference.md). Design:
`docs/specs/2026-11-23-stock-transactions-report-design.md`.

## Context

The owner asked for a desktop report of stock trades for a period, mainly to see a year's balance
before a tax return, with an optional holding-period test (a gain is exempt when the shares were
held longer than a set time; the rule differs by country and some countries have none). A holding
period belongs to a purchase, not to a position. The realized gain the app shows
(`services/cost_basis.rs`) is the weighted-average result of a whole position over its whole life,
without fees: it cannot say what was realized in one year or which part of a sale was
held long enough.

## Decision

- **Lots are derived on read, in the report's own service** (`services/stock_transactions_report/`).
  Per stock position, trades are replayed in the order `cost_basis.rs` uses; every buy opens a lot,
  every sale consumes the oldest open lots first, and each consumed piece is a match. Nothing is
  stored: lots are a pure function of the transactions, so edits, imports, splits and sync merges
  need no upkeep. `cost_basis.rs` and everything that reads it stay as they are.
- **Matches always follow oldest-first order**, whatever the cost method, because the holding
  period of a sold share needs its purchase day.
- **The cost method is a display choice of the report:** oldest buys (the default, each lot at its
  own price and fee) or the weighted average (semantics of `cost_basis.rs`, buy fees pooled). It
  changes what a match cost, never which lots a sale consists of. The choice is a stored preference
  of the report: `app_config` key `stock_transactions_report` (`costMethod`, `holdingTestEnabled`,
  `holdingTestMonths`), in the synced keys, no migration. The period is page state, not stored.
- **The holding-period test is optional and off by default**, with presets and a custom length in
  whole months. Exempt and taxable results are never added together, and an exempt loss does not
  lower a taxable gain.
- **The report computes no tax**, a rate or a return, and the page names no country, central bank
  or tax rule (owner ruling, 2026-11-23).
- **Currency** is the profile's currency, declared by the response; each amount converts at its
  own day through `services/fx.rs`, and a conversion that is estimated or impossible is reported.

## Consequences

- The report's result can differ from the realized gain shown elsewhere: it covers one period,
  includes fees and, with oldest buys, uses a different cost method. The page says so beside the
  cost-method switch.
- Lots are per position (one ticker), not per broker, because Moony has no broker concept for
  stocks.
- Sold more than bought (legacy data) leaves the uncovered part without a lot; it is flagged, not
  guessed, and stays out of the result sums.
- Every read replays all stock trades. Acceptable for personal volumes; a stored lot table would be
  the next step if that stops being true.
- Anyone changing the replay order in `cost_basis.rs` must keep the report's replay in step; one
  test pins that both agree.
