# Stock dividends — design

Date: 2026-10-30. Status: **approved by the owner**. The first version of this spec (dividends
entered by hand, imported from broker CSV files with their withholding tax, written over MCP)
was built and shown to the owner on 2026-10-30, who then ruled (in chat, same day):

1. Yahoo Finance is the primary source of dividends; a dividend is entered by hand only when
   Yahoo has none for the ticker, and the app warns that the ticker's dividends come from Yahoo.
2. No dividends through the CSV import (a dividend imported and then fetched from Yahoo would
   count twice).
3. Every dividend carries its source label: Yahoo Finance or manual.
4. Gross only: no withholding tax (rates differ by country, treaty and broker; the GIPS return
   uses gross dividends anyway).
5. Every dividend is dated on its ex-date (Yahoo knows only the ex-date; GIPS books the income on
   it).
6. A manual dividend for a ticker Yahoo has dividends for is refused, not only warned about.
7. MCP may read dividends but not write them.
8. The sum of a position's dividends is part of its realized return (later the same day).

This spec describes the design after those rulings. The plan is
`docs/plans/2026-10-30-stock-dividends.md`; ADR 0022 records the storage decision.

## 1. Problem

The GIPS return engine (`docs/specs/2026-10-30-gips-stock-performance-design.md`) needs the
dividends each position received: gross, in their currency, on their day. Moony only knows the
yearly dividend estimate (`dividend_data`, `dividend_overrides`), a forecast. Stock transactions
are `buy`/`sell` rows in `investment_transactions`, and 40+ readers derive quantities as "buy
adds, anything else subtracts", so a dividend must never become a row there.

## 2. Decisions

**D1. Yahoo events, payouts derived.** `stock_dividend_events` stores what Yahoo reports per
ticker: `(ticker, ex_date)` primary key, `amount` per share (decimal TEXT, pence converted to
pounds like prices), `currency`, `fetched_at`. A position's payout for an event is `amount ×` the
shares it held at the end of the day before the ex-date (`investments::held_quantity_at`), for
events after its first transaction up to today; events with no shares held give nothing. The
payout is derived on every read and never stored, so a backdated buy or sell corrects it.
Yahoo's amounts are split-adjusted, like Moony's applied splits (a pending split leaves the
shares before it in old units, so those payouts are off until the split is applied).

**D2. Fetching.** `price_api::refresh_dividends` (login, stocks page, import, MCP refresh, the
detail page's refresh button) fetches a ticker's chart from its first transaction day to today
when `stock_dividend_coverage` (`ticker`, `covered_from`, `fetched_at`) is missing, older than a
day, or starts after the first transaction (a backdated buy). The response's dividends are
upserted and its splits recorded as usual. A failed fetch keeps what is stored. The yearly
estimate logic is unchanged.

**D3. Manual dividends.** `investment_dividends` (`id`, `investment_id` FK ON DELETE CASCADE,
`ex_date`, `gross_amount` TEXT > 0, `currency`, `note`, `created_at`) holds payouts the user
enters for a ticker Yahoo has no dividends for (`has_yahoo_dividends` = any event stored). The
service refuses create and update with `validation.dividendFromYahoo` when Yahoo has dividends
for the ticker. A manual dividend entered before Yahoo had the ticker's dividends stays, marked
`superseded`, is listed dimmed and left out of every sum; it can be deleted, not edited.

**D4. One source per ticker.** Yahoo's payouts count when the ticker has any Yahoo event,
otherwise the manual ones. A payout is therefore never counted twice.

**D5. Gross, ex-date, own currency.** No tax anywhere. The currency of a payout is the event's
(Yahoo's quote unit) or the one the user chose; conversion only through `services/fx.rs`
(ADR 0012) at the ex-date's rate.

**D6. Wire contract.** `StockDividendSource` (`yahoo` | `manual`); `StockDividend` (`id` — the
record's id, or `yahoo:<ticker>:<exDate>` — `investmentId, source, exDate, amountPerShare?,
shares?, grossAmount, currency, note, superseded`); `InsertStockDividend` (`exDate, grossAmount,
currency, note?`) with `validate()`; `StockDividendList` (`currency, fxQuality, items,
totalGross, fromYahoo`). `totalGross` sums the counted payouts converted at each ex-date's rate
(ten-day walk-back, then today's quotes flagged `estimated`, else left out and named; never
1:1).

**D7. Commands.** `get_stock_dividends(investmentId, reportCurrency)`,
`create_stock_dividend(investmentId, data)`, `update_stock_dividend(id, data)`,
`delete_stock_dividend(id)`; thin, logic in `services/stock_dividends.rs`. A dividend changes no
quantity or value: no history rebuild; `recalculation-complete` is emitted.

**D8. Validation.** Finite gross > 0; supported currency (`GBX`/`GBp` → GBP ÷100); a calendar
day; note ≤ 500 characters.

**D9. Detail page.** Dividend rows join the position's transaction list with a "Dividenda" badge
and a source badge ("Yahoo Finance" / "Ruční"). A Yahoo row shows the shares held and "X na
akcii", no actions. A manual row shows its note, edit and delete; a superseded one is dimmed,
struck through, reads "Nahrazeno daty z Yahoo Finance, nezapočítává se" and can only be deleted.
The filter has "Dividendy". "Zapsat dividendu" (menu) and "+ Dividenda" (ledger header) open the
dialog (ex-date, currency, gross, note); for a ticker Yahoo has dividends for it shows why
nothing can be entered and disables saving. The "Pozice" card shows "Přijaté dividendy
(hrubě)" in the report currency. The refresh button also refreshes the dividends.

**D10. Mutations.** `use-stock-dividend-mutations.ts`: `['stock-dividends']`, `['investment']`,
`['investments']`, `['investment-transactions']`, `['stocks-analysis']`,
`['stock-performance']`, `['portfolio-metrics']`, `['cashflow-report']`, then `recordSnapshot()`
and `['portfolio-history']` (rule 7). Trades and imports also invalidate `['stock-dividends']`
(payouts follow the shares), and so does every dividend refresh.

**D11. No CSV import, no MCP write.** The stock CSV import keeps skipping dividend rows. MCP has
a read tool `stock_dividends` (source, ex-date, per share, shares, gross, superseded) and
`investment_detail` lists the position's dividends.

**D13. Realized return.** A stock position's `realizedGain` (positions service, so the list, the
totals, the detail page, MCP and the phone agree) is the sale gain plus the gross dividends it
received up to today, each at its ex-date's rate; `dividendsReceived` carries that part on the
wire and the detail page's "Realizovaný výnos" notes "z toho dividendy X". Dividends that exist
but cannot be converted leave the realized return unknown (`null`) and name their currency.

**D12. Demo profile.** The seed stores Yahoo-shaped events for its four tickers.

## 3. Data for the GIPS engine

`services::stock_dividends::payouts(conn, today) -> Vec<Payout { investment_id, ticker, ex_date,
gross, currency, source }>`: every counted payout of every position, oldest first, superseded
manual ones excluded. Convert with `fx::History::convert_on(gross, currency, report, ex_date)`.
Per position: `for_investment(conn, investment_id, today)`.

## 4. Testing

Rust: validation; payout = per share × shares held the day before (a buy on the ex-date gets
nothing, events before the first trade and after today give nothing); manual counted only
without Yahoo dividends, superseded afterwards, refused create/update; CRUD; quantity untouched;
cascade; totals at the ex-date's rate, estimated, unconvertible, superseded excluded; fetch
window (missing, stale, backdated buy, widest coverage kept, amount replaced); chart parsing
(pence, exchange day, negative dropped); MCP read. TS: components and hooks are out of scope
(testing standard).
