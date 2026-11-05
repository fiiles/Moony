# 0023. Stock performance follows GIPS: daily time-weighted return and IRR

Date: 2026-11-05

## Status

Accepted.

## Context

The stock performance page used a chained daily return and a cumulative modified Dietz as its
money-weighted figure. Both ignored dividends, dropped positions sold during the range, priced
a buy at the closing price instead of the price paid, and a modified Dietz is not a rate that
can be compared across periods. The owner asked for the method of the GIPS standard
(`docs/specs/2026-10-30-gips-stock-performance-design.md`).

## Decision

- **Position universe.** Every position held at the end of any day of the range or traded in it,
  sold ones included, so a sale never rewrites the past of the period.
- **Flows.** An external flow `F` of a day is its buys (positive) and sells (negative) at the
  transaction price, the fee included. A flow arrives at the start of its day (it is in the
  day's base) and leaves at the end of it. The base day has no flow; its trades are inside `V_0`.
- **Dividends.** Gross, on the ex-date, from the dividends service (`stock_dividends::payouts`,
  ADR 0022), in the report currency through `services/fx.rs` (ADR 0012). A dividend is return
  (it joins the day's gain), never a flow, and is cash returned to the investor in the IRR.
- **Time-weighted return.** Daily returns `(V_k - V_{k-1} - F_k + D_k) / (V_{k-1} + max(F_k, 0))`,
  chained geometrically. A sleeve (a series of some positions) is the same formula over its own
  positions, with no cash: money leaving a sleeve is a sale, money entering is a buy.
- **Money-weighted return.** The IRR `r` with `-V_0 + sum(-F_k + D_k) (1 + r)^{-t_k} +
  V_end (1 + r)^{-T} = 0`, time in calendar days / 365. Newton from 10 % (in `ln(1 + r)`), bisection
  on `(-99.99 %, +10 000 %)` as the fallback; no sign change or no root in the bracket is
  `null`, shown as "-". The wire carries the period rate `(1 + r)^T - 1` (`mwr`) and the annual
  rate (`mwrAnnual`) per emitted date.
- **Annualisation.** A period up to 365 days shows the period rate, a longer one the annual rate
  (the same rule for the TWR). The two are not mixed in one comparison.
- **Price fallback.** A ticker without any price row is priced by its own trades (each trade's
  price on its day, the last one carried to today while the position is still held); one whose
  first row lies after the range is valued at that first price. A day without a convertible
  value is unknown, not zero.

## Consequences

- The hero figures agree with what a GIPS-minded reader computes by hand, and survive sales,
  fees and dividends.
- An IRR over a few days annualises to an absurd or out-of-bracket rate; the page shows the
  period rate there and a "-" when no rate exists, instead of a misleading number.
- Every report recomputes the IRR at up to 400 emitted dates, each over its own flows; the cost
  is bounded by that cap and measured in milliseconds.
- Missing dividends (a ticker Yahoo has not covered) understate the return; they are listed in
  `dividendGapTickers` rather than hidden.
- The engine stays the one place the method lives; the MCP tool, mobile and desktop read it.
