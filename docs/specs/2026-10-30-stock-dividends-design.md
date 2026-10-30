# Stock dividends as position transactions — design

Date: 2026-10-30. Status: **approved in substance** by the owner through the hand-off brief
`docs/plans/2026-10-30-stock-dividends-handoff-prompt.md` (owner decisions 1–2 and the
requester's proposal); the refinements below (D1–D16) are the implementer's calls, made
autonomously as the brief asks. The plan is `docs/plans/2026-10-30-stock-dividends.md`.

## 1. Problem

Moony knows two stock transaction types, `buy` and `sell`. A dividend the user received can
only be noted as the yearly estimate (`dividend_data`, `dividend_overrides`), which is a
forecast, not a record. The GIPS return engine
(`docs/specs/2026-10-30-gips-stock-performance-design.md`) needs the real payouts: one record
per payout, **gross** (before withholding tax), with its currency and day. The owner decided the
payouts come from a new stock transaction type entered by the user, imported from broker files
or written over MCP, not from Yahoo.

The main risk is the code that reads `investment_transactions.type`: dozens of places in Rust,
SQL and TypeScript treat "not buy" as a sell (`CASE WHEN type = 'buy' THEN quantity ELSE
-quantity END`, `type === 'sell' ? …`). A third value in that column would be counted as a sell
and break quantities, value history, the portfolio and returns.

## 2. Goals and non-goals

Goals:

- Record, edit and delete a received dividend on a stock position (detail page), shown in the
  position's transaction list next to buys and sells, with the sum of received dividends (gross,
  and net when a tax was withheld).
- Import dividends from broker files: Trading 212 (tax in the dividend row), XTB, Interactive
  Brokers and Degiro (tax as a separate row, paired with its dividend), Moony's own table
  (export and import) and custom mappings. Unpaired tax is a warning in the preview, not an
  error. Re-importing a file never duplicates a dividend.
- Write dividends over MCP through the same service as the UI and the import (ADR 0007).
- Nothing that counts buys and sells changes: quantity, cost basis, average price, realized and
  unrealized gain, value history, portfolio history, splits, stock performance.
- Demo profile has a few dividends.

Non-goals (from the brief): the return computation (TWR, MWR, contribution) — the GIPS work
reads this data model later; automatic dividends from Yahoo; the yearly estimate; cash flow and
bank accounts (a dividend on a stock is not a bank movement); fees. Mobile shows no dividends
yet (it never reads the new table, so it cannot mistake one for a sale).

## 3. Decisions

**D1. Own table, not a third `type` value.** Dividends live in `investment_dividends`, one row
per payout, tied to the position by `investment_id` (FK to `stock_investments`, `ON DELETE
CASCADE`). Every existing reader of `investment_transactions` (40+ places across history,
positions, cost basis, splits, dedup, dated facts, FX, MCP, import, TS helpers, mobile) keeps
seeing only buys and sells, so none of them can miscount a dividend, today or in code written
later. The alternative (`type = 'dividend'`) would force a dividend into columns that do not fit
it (`quantity`, `price_per_unit`) and make every one of those readers a regression risk forever.
In the UI a dividend still behaves as a transaction of the position (D9). Recorded in ADR 0022.

**D2. Columns.**

| Column | Type | Meaning |
|---|---|---|
| `id` | TEXT PK | UUID |
| `investment_id` | TEXT NOT NULL | FK `stock_investments(id)` ON DELETE CASCADE |
| `paid_date` | INTEGER NOT NULL | UTC midnight (unix seconds) of the **pay date** |
| `gross_amount` | TEXT NOT NULL | the payout's total before withholding tax, > 0, in `currency` |
| `withholding_tax` | TEXT NOT NULL DEFAULT '0' | tax withheld from this payout, 0 ≤ tax ≤ gross, in `currency` |
| `currency` | TEXT NOT NULL | ISO code of both amounts, one Moony can convert (`fx::is_supported`) |
| `note` | TEXT NULL | free text, trimmed, empty → NULL |
| `import_batch_id` | TEXT NULL | FK `stock_import_batches(id)` ON DELETE SET NULL (like trades) |
| `external_id` | TEXT NULL | `<source>:<broker id>` (`mcp:<id>` over MCP), duplicate rule 1 |
| `created_at` | INTEGER NOT NULL DEFAULT (unixepoch()) | |

Indexes on `investment_id`, `import_batch_id`, `external_id`. Migration `027_stock_dividends`.
Net amount = gross − tax, never stored.

**D3. Total per payout, not per share.** A split never changes it; the stock-split rewrite
(`stock_splits`) and the import's split conversion do not touch dividends.

**D4. Pay date only.** The ex-date is not stored (every broker gives the pay date; the GIPS
engine books dividends on their pay date per the owner's ruling of 2026-10-30).

**D5. The dividend currency may differ from the position currency.** A broker can credit a USD
dividend in the account's currency (XTB credits in the account currency), and London listings
pay in GBP while quoted in pence. The amount is a fact in its own currency (ADR 0012) and is
converted only through `services/fx.rs`. `GBX`/`GBp` amounts are converted to GBP on entry
(÷100), as for prices.

**D6. Wire contract.** `StockDividend` (`id, investmentId, paidDate, grossAmount,
withholdingTax, netAmount, currency, note, createdAt`), `InsertStockDividend` (`paidDate,
grossAmount, withholdingTax?, currency, note?`) with `validate()`, and `StockDividendList`
(`currency, fxQuality, items, totalGross, totalNet`): the totals are every payout converted to
the report currency at its pay day's rate (ten-day walk-back, then today's quotes flagged
`estimated`, else left out and named in `fxQuality`; never 1:1), the cost-basis rule. `netAmount`
is derived on read so the UI never subtracts money strings.

**D7. Commands** (thin, `services/stock_dividends.rs` holds the logic): `get_stock_dividends
(investmentId, reportCurrency)`, `create_stock_dividend(investmentId, data)`,
`update_stock_dividend(id, data)`, `delete_stock_dividend(id)`. A dividend changes no value, so
no history rebuild is queued; the commands emit `recalculation-complete` like other stock
mutations so open pages refetch.

**D8. Validation** (`InsertStockDividend::validate`, all `validation.*` keys): finite gross > 0;
finite tax with 0 ≤ tax ≤ gross (absent = 0); supported currency (`GBX`/`GBp` accepted and
normalised); `paidDate` a supported UTC calendar day (`validation::utc_day`); note ≤ 500 chars.

**D9. Detail page.** "Zapsat dividendu" in the page's more-actions menu and a "+ Dividenda"
button in the transaction card's header open a dialog: pay date, currency (defaults to the
currency of the yearly estimate, else the position's price currency), gross amount, withholding
tax (optional), note. Dividend rows join the position's transaction list, badged "Dividenda":
quantity "—", the price column reads "daň X · čistě Y" when tax > 0, the total is the gross
converted at the pay day's rate (native gross when no rate); edit and delete as on trades (the
delete confirmation names the date and gross). The transaction filter gets a "Dividendy" option.
The "Pozice" side card lists "Přijaté dividendy" (gross in the report currency) and "Dividendy
po dani" (net) when any tax was withheld. The Stats row keeps the yearly estimate unchanged, and
the trade-event chart markers stay buy/sell only.

**D10. Mutations** (frontend hook `use-stock-dividend-mutations.ts`): invalidate
`['stock-dividends']`, `['investment']`, `['investments']`, `['investment-transactions']`,
`['stocks-analysis']`, `['stock-performance']`, `['portfolio-metrics']`, `['cashflow-report']`, then `recordSnapshot()` and invalidate `['portfolio-history']` (rule 7;
the snapshot is unchanged in value but the rule is uniform).

**D11. Import: two new type-value actions.** `TypeValueAction` gains `dividend` and
`withholdingTax`. The keyword classifier suggests them (dividend/dividenda/dividende/dividendo/
dywidenda/utdelning/…, withholding tax/srážková daň/quellensteuer/dividend tax/…), the type-value
table offers them, and `DirectionMode::QuantitySign` files read them only through an optional
type column. A dividend row needs an instrument, a day and a positive amount; a tax row an
instrument, a day and an amount.

**D12. Import: where the amounts are.** `StockImportConfig` gains three optional columns
(`serde(default)`, so saved formats keep working): `amountColumn` (the row's total amount: the
gross dividend, or the tax), `taxColumn` (a tax withheld from the dividend in the same row) and
`taxCurrencyColumn` (its currency when it differs from the row's), plus `cashCurrency` (the
currency of dividend and tax amounts when the file names none, e.g. XTB, whose amounts are in
the account currency). The gross of a dividend row is `|amount|` when the amount column is set
and filled, else `quantity × price` (Trading 212 gives shares × gross per share; its `Total` is
net in the account currency and is not read). The currency is the row's currency column /
fixed currency, else `cashCurrency`; without either the row is an error asking for the account
currency (the review step shows a currency picker then).

**D13. Import: pairing tax rows.** A `withholdingTax` row is paired with the dividend of the
same instrument and the same UTC day: the tax amounts of that (instrument, day) are summed with
their signs and the absolute sum becomes the dividend's tax (a refund row cancels a charge, as
IBKR reports corrections). It attaches to the first dividend row of that pair in file order.
A tax row without such a dividend, or in another currency than its dividend, is a **warning**:
the row is listed as skipped with `importWizard.row.taxUnpaired` / `taxCurrencyMismatch`, the
preview counts it in `unpairedTaxes`, and the review step shows a warning banner. A paired tax
row is listed as skipped with `importWizard.row.taxPaired` (detail = the dividend's line). An
in-row tax (`taxColumn`) in another currency than the dividend is the same warning on that row
and the dividend is imported without tax. A tax larger than its gross makes the dividend row an
error (`importWizard.row.taxExceedsGross`). A negative dividend row (a broker reversal) cancels the first dividend of the same instrument, day, currency and amount; both are listed as skipped (`importWizard.row.dividendReversed` / `dividendReversal`, detail = the other line) and a re-post imports as the payout. A reversal without such a dividend, or a zero amount, is skipped (`importWizard.row.dividendNotPositive`). A tax row with no instrument (a tax on interest) is not a dividend tax and is skipped.

**D14. Import: decisions per dividend row** (after the trades' simulation, so a position the
file's own buys create counts): skipped instrument → skipped; no ticker → error; no position in
Moony and none created by the file's accepted trades → error
`importWizard.row.dividendWithoutPosition`; duplicate → duplicate (imported only via
`importAnywayLines`); else new. Duplicates: rule 1, the same `external_id` already stored for a
dividend of the ticker; rule 2, a stored dividend of the ticker with the same pay day, the same
numeric gross and the same currency, each stored row absorbing at most one file row. Dividend
rows are never split-converted. Accepted dividends are written in the same SQL transaction as
the trades (after them), stamped with the batch id; `stock_import_batches.trade_count` counts
trades and dividends; undoing the batch deletes its dividends too. The instruments panel lists
the instruments of dividend rows (`dividendCount` next to `tradeCount`), so an ISIN-only
dividend file can be resolved like a trade file. Preview rows gain `kind`
(`trade`/`dividend`/`withholdingTax`), `amount` and `tax`.

**D15. Adapters.**
- *Trading 212*: `Dividend (…)` actions map to `dividend` (classifier); `taxColumn` = `Withholding
  tax`, `taxCurrencyColumn` = `Currency (Withholding tax)`.
- *XTB*: the type values `Dividend`/`DIVIDENT` → `dividend`, `Withholding Tax` → `withholdingTax`;
  `amountColumn` = `Amount`; the account currency is not in the file, so `cashCurrency` is asked
  in the review step (remembered with a saved format).
- *Interactive Brokers*: the Flex Query **Cash Transactions** section (`Type`, `Amount`,
  `CurrencyPrimary`, `Symbol`/`ISIN`, `Date/Time` or `SettleDate`, `TransactionID`): `Dividends`
  and `Payment In Lieu Of Dividends` → `dividend`, `Withholding Tax` → `withholdingTax`, others
  skipped. Detected next to the existing Trades detection (same source `ibkr`).
- *Degiro*: the **Account statement** (`Date, Time, Value date, Product, ISIN, Description, FX,
  Change, , Balance, , Order Id`, translated headers, currency then amount under `Change`):
  descriptions classified by keyword (`Dividend` → dividend, `Dividend Tax`/`Dividendbelasting`/
  `Dividendensteuer`/… → tax); the trade lines of the statement are skipped (trades come from
  the Transactions export). Detected by structure, like the Transactions export.
- *Moony table*: export writes dividends as `Type = dividend` with empty quantity and price and
  two new trailing columns `Amount` and `Withholding tax`; the adapter reads both optional
  columns (`Částka`, `Srážková daň` in Czech) and maps `dividend`/`dividenda`.

**D16. MCP.** A write tool `stock_dividends_create` (bulk, ≤ 200 rows, all-or-nothing, the
shared service writer, possible duplicates skipped and reported, `importAnyway` and
`externalId` like trades, an undoable batch) and a read tool `stock_dividends` (optionally by
ticker); `investment_detail` lists the position's dividends. A dividend needs an existing
position.

## 4. Data for the GIPS engine

For each dividend: `SELECT d.investment_id, i.ticker, d.paid_date, d.gross_amount, d.currency
FROM investment_dividends d JOIN stock_investments i ON i.id = d.investment_id`. `gross_amount`
is decimal TEXT in `currency` (parse as f64), `paid_date` is the UTC midnight of the pay day.
Convert with `fx::History::convert_on(gross, currency, report, paid_date)`.

## 5. Testing

Rust service tests (in-memory DB): validation; CRUD; totals per currency with day rates,
estimated and missing; a dividend changes neither the stored quantity nor
`recalculate_investment_metrics`, `replayed_quantities`, `Quantities::load` of stock
performance, the history plan, nor a split rewrite; cascade on position delete; import: parse
(amount column, quantity×price, in-row tax, tax rows, signs), pairing (paired, unpaired, other
currency, refund, exceeding), position rule, duplicates (id, values, one-to-one, import anyway),
write + undo, every adapter's dividend rows; MCP bulk tool. TS: none beyond existing helpers
(components and hooks are out of scope per `docs/standards/testing.md`).
