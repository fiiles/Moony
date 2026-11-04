# Stocks analysis: performance by the accepted standard (GIPS)

Agreed with the owner on 2026-10-30 in chat. This spec is the only record of the design.

**Builds on the stock dividends feature (owner, 2026-10-30).** Dividends were built first as
their own feature on branch `feat/stock-dividends` (migration 027, ADR 0022, the stock
dividends service): Yahoo payouts per share by ex-date, a position's payout derived from the
shares held the day before; a manual payout only for a ticker Yahoo has none for. This spec
needs that branch merged first. The fee migration and this spec's ADR take the next free
numbers after it.

Settled by Claude after a builder review, not by the owner (check these): money out at the
end of the day, money in at the start; transaction prices as the fallback for a ticker
without market prices and its "no market price" quality note; the IRR bracket, day count and
the "≤ 365 days is not annualised" boundary; fee currency conversion on import; the
"dividends not loaded" quality note from the dividend coverage.

## Intent

The owner wants the stocks analysis page to show **his return over time with his deposits and
withdrawals taken out**, so that he can put it next to an index. That is the time-weighted
return, and the accepted rules for computing it are the CFA Institute's Global Investment
Performance Standards (GIPS). The figures on the page must follow those rules, so that the
comparison with an index is fair.

The trigger: a friend's portfolio showed a large negative time-weighted return with Boeing
contributing −55 percentage points. That case turned out to be **correct** under GIPS (Boeing
was his only holding for the eight months of the 2020 crash, so it was 100 % of the
portfolio), but reviewing it showed four places where the engine departs from the standard.
This spec fixes those four.

## Goals

1. **Every position held during the period counts**, including positions sold since and
   positions sold within the period. Today the engine takes only positions with quantity > 0
   (`held_positions` in the stock performance service), which drops sold positions from the
   past as well. That is survivorship bias and the worst error against an index.
2. **Trades are valued at their own price.** The flow of a buy or a sell is
   `quantity × transaction price` (± the fee), not `quantity × the day's close`. The gain of
   the trade day is then `close value − previous value − flow`, so the move between the trade
   and the close is return, as GIPS requires (trade-date accounting at the transaction price).
3. **An optional fee per transaction.** A new optional field on a stock transaction. A buy's
   flow is `quantity × price + fee`, a sell's flow is `−(quantity × price − fee)`. The fee
   lowers the return (GIPS: returns after trading expenses).
4. **Dividends are part of the return (total return).** The payouts of each position from
   the stock dividends feature, **gross** (before withholding tax) — owner's decision, to
   match an index that reinvests gross dividends.
5. **The money-weighted return is the internal rate of return (IRR)**, the GIPS definition,
   replacing the cumulative modified Dietz.
6. **Annualised figures by the GIPS rule:** a period longer than one year shows the
   annualised return as well; a period of one year or less is never annualised.

## Non-goals

- **No smoothing of an early small portfolio.** A tiny first holding that falls still drags
  the time-weighted return for good (the Boeing case). That is what the standard computes;
  the money-weighted return and the gain in money answer "did I make money".
- **No broker cash.** Moony does not know the cash at a broker. The stock portfolio is
  measured as a sleeve: a buy is money in, a sell is money out, a dividend is money out on its
  ex-date. This is allowed by GIPS for a carve-out without cash and is how the page works now.
- **No withholding tax.** Dividends are gross; no per-country rate.
- **No change to cost basis, unrealized gain or realized gain** elsewhere in the app because
  of the fee. The fee enters the performance engine only (see Follow-ups).
- **No change to the index source.** S&P 500 through CSPX.L and MSCI World through IWDA.L
  are accumulating ETFs: their close already contains reinvested dividends (net of the fund's
  own withholding tax, a small difference against gross dividends that we accept).
- No GIPS composite, no GIPS compliance claim in the UI, no dividend calendar.

## Anti-goals

- The page must not show a return that is better than reality because something was left
  out (sold losers, fees, a trade-day move).
- No figure may compare an annualised value with a cumulative one (today the hero shows the
  MWR − TWR gap; both sides must be on the same basis).

## The method

Everything stays a daily engine in the report currency, values and flows converted at their
own day (ADR 0012, `services/fx.rs`). For position `p` and day `s`:

- `V_p(s)` value at the close: quantity held at the end of `s` × the last price row at or
  before `s` (unchanged).
- `F_p(s)` external flow: the sum over the transactions of `p` dated `s` of
  `+(q × price + fee)` for a buy and `−(q × price − fee)` for a sell, in the transaction
  currency, converted at `s`. **Not** the close price any more.
- `D_p(s)` gross dividend: the sum of the position's counted payouts with ex-date `s` (the
  stock dividends service's `payouts`: Yahoo amount per share × shares held at the end of
  `s − 1`, or a manual payout where Yahoo has none; superseded manual ones excluded), each in
  its own currency, converted at `s`. The dividend leaves the sleeve the same day.
- **Money in at the start of the day, money out at the end** (the usual daily-TWR
  convention): `base_p(s) = V_p(s−1) + max(F_p(s), 0)`. A sell and a dividend do not reduce
  the base. With sells priced at the transaction price, putting a sell in the base would make
  a sell-out day's base `V(s−1) − proceeds`, near zero or negative, and its return absurd.
  When a day has both a buy and a sell of one position, the net flow decides.
- `gain_p(s) = V_p(s) − V_p(s−1) − F_p(s) + D_p(s)`.
- The day's flows, the dividend and the value of a position drop out together when any of
  their conversions fails (counted in `fxQuality`, as today).
- Portfolio, sets, contributions: unchanged formulas over the new `base` and `gain`
  (`R_S = Σ_S gain / Σ_S base`, chain-linked; contribution
  `C_S(t) = Σ G(s−1) · Σ_S gain(s) / Σ_all base(s)`). Contributions of a partition still sum
  exactly to the portfolio TWR.

### The position universe

Every stock position whose quantity at the end of any day of the range (the base day
included) is above zero, or that has a trade on any day after the base day. This includes
positions whose quantity today is 0 (`stock_investments` rows stay while they have
transactions). The range clamp ("Vše") starts at the first day any of these held anything.

**Prices of sold positions.** The history planner already builds `stock_value_history` for a
ticker with a trade on or after the rebuild day, and a rebuild deletes only the days without
a position, so a sold ticker keeps its rows for the days it was held. A ticker Yahoo has no
prices for at all (delisted, unknown) falls back to its own transaction prices: each
transaction's price is a known price on its day, carried forward like a price row. Such a
position is counted in a new "no market price" quality note instead of dropping out silently.

**Splits.** The split feature rewrites both `quantity` and `price_per_unit` of past
transactions to current units, so `q × price` is unchanged by a split. The fee is an amount
per transaction, not per share, so a split leaves it alone.

### Money-weighted return (IRR)

The rate `r` that solves `V_0 + Σ_j CF_j · (1 + r)^{-(t_j − t_0)/365} = V_T · (1 + r)^{-(T − t_0)/365}`
where the cash flows are the external flows `F` (in) and the dividends `D` (out) of the
portfolio. `t_0` is the base day of the range: `V_0` is its closing value and its own
trades are inside `V_0` (the engine gives the base day no flow), flows run from the next day.
Time is calendar days / 365. Solved numerically: Newton with a bisection fallback on the
bracket `r ∈ (−99.99 %, +10 000 %)`; no sign change in the bracket gives `null`, shown as "—".

Shown as:
- period ≤ 365 days (exactly 365 included): the **period** rate
  `(1 + r)^{(T − t_0)/365} − 1` (not annualised);
- period > 365 days: the annualised rate `r`.

The per-date `mwr` series (used when hovering the chart) follows the same rule for the
period from the first date to the hovered date.

### Annualised TWR

For a period longer than 365 days: `(1 + TWR)^{365/days} − 1`. The cumulative TWR stays the
main figure; the annualised one is shown next to it. For ≤ 365 days it is not shown.

The hero's MWR gap compares MWR with the TWR **on the same basis**: annualised with
annualised over a year, period with period up to a year.

### Dividends

- **Source:** the stock dividends service (`payouts`), the one rule for which payouts count:
  Yahoo per-share events (`stock_dividend_events`) × the shares held the day before the
  ex-date, derived on every read; a manual payout (`investment_dividends`) only where Yahoo
  has no dividends for the ticker. Every amount is gross; no tax is stored. The engine reuses
  that rule (a range-limited variant if `payouts` is too slow for the page), never its own.
- **Ex-date.** Every payout is dated by its ex-date, the GIPS accrual date, the day the price
  drops.
- **Dividends not loaded.** A ticker held in the range whose Yahoo fetch has not covered the
  range yet (`stock_dividend_coverage` missing or starting later) may lack payouts. Such
  tickers are counted in a quality note next to `fxQuality` (copy in all ten locales), so
  missing dividends never pass silently.
- **Not the yearly estimate.** `dividend_data` and `dividend_overrides` (the yearly sum, the
  manual override) stay as they are for the yield shown elsewhere; the engine ignores them.
- **Known gap from the dividend feature:** while a detected split waits for confirmation,
  payouts before it are computed from the old share count.
- **On the page:** "Změna hodnoty" keeps net deposits + market change; dividends paid in the
  period are shown as a separate line (a new field of the performance report), since they
  are return that left the portfolio.

### The fee

- Migration: nullable `fee` TEXT on `investment_transactions`, in the transaction currency.
  `NULL` and empty mean 0. Validated ≥ 0 by `validate()`.
- Wire contract per the type-contract procedure (schema.ts, Rust types, bindings).
- UI: an optional "Poplatek" field in the stock transaction form (add and edit).
- Import: the wizard already detects a fee/commission column (`hasFeeColumn`) and drops it;
  map it into the new field. One currency per transaction: when a broker states the fee in a
  different currency than the trade, the import converts it at the trade date through
  `services/fx.rs`; when the file gives no fee currency, the fee is taken to be in the trade
  currency. A fee is stored as a positive amount whatever sign the file uses.
- MCP: the transaction write tool accepts the fee too (one write path, ADR 0007).

## On the page

- **Every position held in the period appears** wherever positions are listed for the
  period: the board, "Příspěvek k výnosu", the holdings table, and the parts of
  "Rozdělit podle" (tags, sector, currency). No separate "sold" group — owner's decision.
  A position sold before the end of the period has end value 0.
- Hero: cumulative TWR as now, plus "ročně X %" when the period is longer than a year;
  MWR by the IRR rule above with its label saying "ročně" or "za období".
- Copy changes land in all ten locales.

## Builder's call

- How the holdings table shows a sold position's tags and actions.
- Whether the per-date IRR series is computed for every emitted date or interpolated
  (performance: at most 400 dates).

## Docs

- A new ADR: "Stock performance follows GIPS" (position universe, transaction-price flows,
  fee, gross dividends on the ex-date, IRR, annualisation, sleeve without cash).
- `docs/architecture/database.md` for the new table and column.
- The module doc of the stock performance service rewritten to the new method.

## Testing

Rust service tests (in-memory DB, per the testing standard):
- a position sold out before the end of the period still contributes and the contributions
  of all positions sum to the portfolio TWR;
- a position sold before the period starts and not held in it does not appear;
- a buy above or below the close books the difference as that day's gain;
- a full sell-out at a price different from the previous close gives that day a sane return
  (base is the previous value, not value − proceeds);
- a split between two trades leaves the flows and the return unchanged;
- a ticker without any price row is valued at its transaction prices and counted in the
  quality note;
- a fee lowers the return by the expected amount, on a buy and on a sell;
- a Yahoo dividend is gross gain on its ex-date on the shares held the day before; a manual
  one counts only where Yahoo has none; a ticker without coverage of the range is counted in
  the quality note;
- IRR: a known textbook case; a period under a year is not annualised; no solution gives
  `None`;
- annualised TWR for a period over a year;
- the Boeing case as a regression: a small first holding that falls before larger buys
  keeps its full contribution.

TypeScript: the model functions that pick annualised vs period figures and the MWR gap.

## Follow-ups (not in this spec)

- Fees in cost basis, unrealized and realized gain across the app.
- Withholding tax rates per country (net dividends).
- A total-return option for a custom index ticker (adjusted close).
