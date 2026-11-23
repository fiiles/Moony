# Stock transactions report — design

Date: 2026-11-23. Status: **approved by the owner** (2026-11-23, in chat: write the spec, then
implement it right away). The calls listed in §7 stand until he reverses one.

The owner asked for a desktop-only report "Akciové transakce": statistics and the detail of stock
trades for a period (for example the current year), mainly to see the year's balance for taxes, with
an optional and clearly explained holding-period test ("časový test": in Czechia a gain is exempt
when the shares were held for more than three years; the rule is not universal).

A clickable prototype with fictional data was built and revised with him the same day:
`docs/specs/2026-11-23-stock-transactions-report-prototype.html`. It is the visual reference for
everything in §4. Where the prototype and this text differ, this text wins.

His rulings on the prototype (in chat, 2026-11-23):

1. The summary reads: proceeds, acquisition cost, result, fees, result after fees.
2. The holding period has presets and a "Vlastní" choice with the length typed in months.
3. The acquisition cost can be switched between the oldest buys (FIFO) and the weighted average.
4. The first table view is "Výsledek prodejů", not "Prodeje".
5. "Všechny transakce" lists dividends too.
6. "Blíží se časový test" shows the nearest lots and opens a modal with every one of them.
7. "Exportovat CSV" belongs to the "Všechny transakce" table, not to the page head.
8. (During the build.) The page names no country, Czechia included, in tooltips or anywhere else.

## 1. Problem

Moony knows every stock trade (`investment_transactions`: buy or sell, quantity, price, currency,
day, optional fee) and every gross dividend (`services::stock_dividends::payouts`), but it has no
view of them by period. Someone preparing a tax return has to add up the year's sales by hand, find
what the sold shares cost and, where a holding-period exemption applies, work out which sold shares
were held long enough.

The realized gain the app shows today (`services/cost_basis.rs`) is the weighted-average result of a
whole position over its whole life, without fees. It cannot answer "what did I realize in 2026" or
"which part of this sale is exempt", because a holding period belongs to a purchase, not to a
position.

## 2. Goals, non-goals, anti-goals

**Goals**

- For a chosen period: what was sold for how much, what it cost, the fees, the result, and the
  dividends received, each in the report currency.
- An optional holding-period test that splits the sold shares into taxable and exempt ones.
- The detail behind every number: each sale down to the purchases it consists of.
- Open purchases and the day each of them passes the test.

**Non-goals**

- Computing a tax, a tax rate or a tax return.
- Country rules beyond the holding period: the Czech 100 000 Kč yearly proceeds exemption, loss
  carry-over, wash-sale rules.
- Central-bank rates of a particular country (ČNB daily or yearly rate). The report converts like
  the rest of Moony (ADR 0012).
- Crypto, bonds or any other asset.
- Separate broker accounts. Lots are matched across a whole position (one ticker), as Moony has no
  broker concept for stocks.
- The phone app, the MCP server, an export of the "Výsledek prodejů" view.

**Anti-goals** (would make the report a failure even if it works)

- Presenting a figure as the tax due, or wording that reads as tax advice.
- A holding-period rule that is on by default or cannot be turned off.
- Adding the exempt and the taxable result together into one "taxable" figure, or letting an exempt
  loss lower a taxable gain.
- Changing the realized gain shown elsewhere in the app.

## 3. Approaches

**A. A Rust service derives the whole report on every read (chosen).** A new service replays the
transactions into purchase lots, matches the sales, converts through `services/fx.rs` and returns
one typed response. It follows rule 4 (business logic in services, tested on an in-memory
database), rule 10 (conversion only through `fx.rs`) and the app's habit of deriving cost figures
instead of storing them.

**B. The frontend computes it from transactions and rates, as the prototype does.** Rejected:
business logic and currency conversion would live in React, outside the tested layer, and the
figures could drift from the Rust ones.

**C. Teach `cost_basis.rs` about lots.** Rejected: its weighted-average semantics are read all over
the app (positions, history, MCP) and stay as they are (ADR 0001). The report reuses its replay
order and its conversion rules but keeps the lot logic to itself.

**D. Store lots in a table.** Rejected: lots are a pure function of the transactions; a stored copy
would have to follow every edit, import, split and sync merge.

## 4. Decisions

### Page

**D1. Placement.** Route `/reports/stock-transactions`, page `src/pages/StockTransactions.tsx`,
sidebar item `stockTransactions` (`nav.stockTransactions`, cs "Akciové transakce") in **Nástroje**
directly after *Analýza akcií*, icon `Receipt`, `prefKey: 'investments'` like *Analýza akcií*.
Desktop only: nothing is added under `src/mobile/`.

**D2. Period.** A calendar year (default: the current year; a stepper from the year of the first
stock transaction to the current year) or "Vlastní období" with Od and Do, both days included. Days
are UTC days as everywhere else. The period is page state and is not stored.

**D3. Structure, top to bottom** (prototype):

1. Page head: eyebrow, title, one sentence, the period controls. No primary button.
2. Hero card: the result after fees of all sales of the period as the dominant number, a change
   badge with the result as a percentage of cost plus fees, the count of sales in profit and in
   loss; with the test on, the taxable result and the exempt result in the hero's aside. Under it
   the settings strip (D6, D7) and the summary table (D8).
3. Four stats (D9).
4. Two cards side by side: buys and sells over time (D10), result by ticker (D11).
5. "Blíží se časový test" (D12), only with the test on.
6. The table card with two views (D13, D14).

### Lots and cost

**D4. Lots.** Per stock position (`stock_investments`; the ticker is unique, so a position is a
ticker; rows are matched by `investment_id`), transactions are replayed in the order `cost_basis.rs`
uses (by day, buys before sells on the same day, then stored order). That ordering is private there
today: expose it or mirror it, and pin with one test that both replays agree. Days are whole UTC
days; there is no order inside a day beyond buys first, so a sale can consume a lot bought the same
day. Every buy opens a lot (day, quantity,
price, currency, fee). Every sale consumes the oldest open lots first; each consumed piece is a
*match* (sale, lot, quantity). A lot can be consumed by several sales and a sale can consume several
lots. Quantities are in the stored units (split-adjusted rows, as stored today).

Matches always follow this FIFO order, whatever D6 says, because the holding period of a sold share
needs its purchase day.

**D5. Amounts of a match**, all in the report currency, each converted at its own day (D15):

- proceeds = quantity × sale price, at the sale day;
- sale fee = the sale's fee × quantity ÷ the sale's quantity, at the sale day;
- acquisition cost and buy fee by the method of D6;
- result = proceeds − acquisition cost; result after fees = result − buy fee − sale fee.

A fee is in its trade's currency (as in the CSV export). A trade without a fee has none.

**D6. Cost method**, a two-way switch "Nabývací cena":

- **Nejstarší nákupy** (`fifo`, the default): acquisition cost = quantity × the lot's price at the
  lot's day; buy fee = the lot's fee × quantity ÷ the lot's quantity, at the lot's day.
- **Vážený průměr** (`average`): acquisition cost = quantity × the running average cost per share of
  everything the position held before the sale, each buy converted at its own day, a sale releasing
  cost at that average (the semantics of `cost_basis.rs`). Buy fees are pooled and released the
  same way.

The method changes what a match cost, never which lots it consists of: with the average, a sale
still has one match per consumed lot (for its purchase day, quantity and class), each costing its
quantity × the average at that sale, so the matches of a sale add up to the sale. It also sets the
cost used for the unrealized result in D12 (with the average: the open quantity × the position's
current average cost and pooled fee per share). The tooltip next to the switch explains both and says that the
rest of Moony uses the weighted average without fees, so its realized gain can differ.

### Holding-period test

**D7. The test** is a switch "Časový test", **off by default**, with a length: presets 6 months,
1 year, 2 years, 3 years (preselected), 5 years, 10 years, and "Vlastní…" with a whole number of
months from 1 to 600. A match is **exempt** when the sale day is later than the purchase day plus
the length in calendar months (a day that does not exist in the target month becomes that month's
last day); otherwise it is **taxable**. A purchase on 29. 11. 2023 with three years is exempt from
30. 11. 2026.

With the test off there are no classes anywhere on the page: one summary row, a "Držení" column
instead of "Časový test", no class filter, no D12 card.

An info button beside the switch opens an explanatory tooltip (a wide, wrapping variant of `.tip`,
shown on hover and focus, pinned by a click, closed by Esc): what the test is, that the length
differs by country and some countries have no such rule, that it should be on only when it applies
to the user, how to enter another length, that Moony computes no tax and that an exempt loss does
not lower the taxable result. **No country is named anywhere on the page** (owner, 2026-11-23,
during the build): not in this tooltip, not in the cost-method or currency tooltips, in no
language. The three-year preset is preselected as a plain default without attribution.

**D8. Summary table** "Příjmy, nabývací cena a výsledek", columns: Příjmy z prodeje, Nabývací cena,
Výsledek, Poplatky, Výsledek po poplatcích. Rows with the test on: **Podléhá dani** (pieces held at
most the length), **Osvobozeno časovým testem** (held longer), **Celkem**. With the test off: one
row "Všechny prodeje". Fees here are the buy and sale fees of the sold pieces (D5). When the test
is on and the taxable result after fees is negative, an inline alert says that the taxable sales
ended in a loss and the exempt gain is not added to it. Under the table one line states the
conversion rule and the chosen cost method and that dividends are not part of the result.

### Other widgets

**D9. Stats.** Příjmy z prodeje (count of sales, count of tickers); Nakoupeno (volume of buys,
counts); Zaplacené poplatky (the fees of every trade in the period, buys included, and their share
of the traded volume; this is deliberately not the fee figure of D8); Dividendy (hrubé) (gross
payouts with an ex-date in the period, their count, an info tooltip saying they are taxed apart and
stay outside the result).

**D10. Nákupy a prodeje v čase.** Bars of buy volume (`s1`) and sell volume (`s3`) per month; a
period longer than 24 months is drawn per year. Hover shows the bucket's buys, sells and result. A
click on a bucket filters the table card to it and dims the others; a second click, or the × of the
`FilterBadge` in the table card, clears it. Changing the period clears it.

**D11. Výsledek podle titulu.** One row per sold ticker with its result after fees, sorted from the
largest gain to the largest loss, a bar from a common zero line, the signed amount in gain or loss
colour. With the test on, a sub-line: "celé osvobozeno", "celé podléhá dani" or the taxable part.
Six rows, the rest behind "Zobrazit všech N titulů".

**D12. Blíží se časový test** (test on only). Works on open lots (the unconsumed quantity of each
lot). A lot passes on the day after its purchase day plus the length. The card lists the four
nearest lots that have not passed: date chip (dark when at most seven days away), name and
quantity, purchase day, unrealized result = quantity × current price in the report currency (as the
positions service values it) − its cost by the method of D6. A foot line gives the share of the
open positions' value that has already passed. A button "Zobrazit všech N" (or "Zobrazit detail")
opens the modal "Časový test otevřených pozic" (800 px): tabs "Čeká na test · N" and "Už prošlo ·
M", a scrolling table (day, title with quantity, purchase day and price, value today, unrealized
result), footer with Zavřít and the same foot line. The current price and its conversion are exactly what the
positions service uses for the position (its price resolver and the current quotes); the report
adds no staleness rule of its own. A lot without a current price shows "—" for value and result
and is left out of the share. A position without any transaction has no lot and does not appear.

**D13. Výsledek prodejů** (default view). One row per sale of the period, newest first: day, title
(name, ticker and quantity), proceeds (with "n × price" in the trade currency), Výdaje (acquisition
cost plus fees, "z toho poplatky" under it, a tooltip on the head), result after fees with a change
badge, and "Časový test" (badge Podléhá dani / Osvobozeno / Zčásti osvobozeno, with the holding time
or "n z m ks osvobozeno"). A row expands to its matches: purchase day, quantity and purchase price
(or, with the average method, the average cost per share), the same amounts, its own badge and
holding time. Controls: search by ticker or name; with the test on a filter Vše · Podléhá dani ·
Osvobozeno, under which a sale shows only its matching pieces and their sums. The last row totals
what is shown.

**D14. Všechny transakce.** Every buy, sale and dividend of the period, newest first: day, type
(▲ Nákup, ▽ Prodej, ○ Dividenda), title, quantity, price per share in the trade currency, volume in
the report currency with the rate under it, fee. A dividend counts in the period of its ex-date and is what
`stock_dividends::payouts` counts (never a superseded manual one). Its row shows the shares held
(at the end of the day before the ex-date, the rule the payout itself uses) and the amount per
share when the payout comes from a Yahoo event, and "—" for both when it was entered by hand. Controls: search, a filter Vše · Nákupy · Prodeje · Dividendy and **Exportovat CSV**, which
saves exactly the rows shown (period, bucket, type, search). The existing stock export command
takes no filter and writes the importer's format, so this file is built in the frontend from the
rows the page already holds (a pure, tested function in `src/utils/`) and saved the way the other
exports save their CSV text; file `stock_transactions_<from>_<to>.csv`, columns Date, Type, Ticker, Name, Quantity, Price, Currency,
Rate, Amount, Report currency, Fee. It is a document for a person, not a file the importer reads.

### Rules across the page

**D15. Currency.** The response declares its report currency (the profile's currency, rule 10).
Every amount is converted at its own day through `fx::History` with the usual walk-back; a
conversion that had to use current quotes marks the result estimated, one that is impossible leaves
the amount out and names the currency, shown with the existing `ConversionQualityNote`. A sale with
an amount that could not be converted (its proceeds, a fee, or the cost of one of its matches) is
listed with "—" in the affected cells and stays out of every sum of D8, D10's result, D11 and D13;
its proceeds, when they convert, still count in D9 and D10's volume. With the average method an
unconvertible buy makes the cost unknown for every later sale of that position that was held
together with it (the rule of `cost_basis.rs`); with the oldest-buys method only for the matches of
that lot.

**D16. Sold more than bought.** New writes cannot oversell, but stored data can (legacy rows, a
position without transactions). The part of a sale with no lot left has an unknown cost. The
covered part is matched and counted as usual; the uncovered quantity produces no match and stays
out of D8, D11 and the sums of D13 with its proceeds and its share of the sale fee, so every
column keeps describing the same shares. The sale's row shows the covered sums and the note "Chybí
nákup pro n ks", and one compact alert above the table card names the tickers. The whole sale
still counts in D9 (proceeds, count) and in D10's volume: that money did come in.

**D17. Settings are stored**, not the period. `app_config` key `stock_transactions_report` holds
`{ costMethod, holdingTestEnabled, holdingTestMonths }` (defaults `fifo`, `false`, `36`) and joins
`APP_CONFIG_SYNCED_KEYS`, so a second desktop agrees (that array has a fixed length and feeds the
sync snapshot and capture SQL: grow it and update the sync tests that list the keys). No migration.

**D18. Contract.** One read command, `get_stock_transactions_report`, with the period, the cost
method and the optional test length; the response carries the summary rows, stats, buckets, ticker
results, sales with their matches, ledger rows, open lots and the conversion quality. A second pair
of commands reads and writes D17. Types go through the wire-contract procedure (rules 2 and 3). The
frontend only filters and totals what it was given (search, bucket, class, type); those pure
functions live in `src/utils/` with tests.

**D19. Empty states.** No stock transactions at all: an empty state that links to Akcie. A period
without sales: the hero reads 0 with "V tomto období jste nic neprodali.", the ticker card and the
results view show their empty states, the ledger still lists the buys and dividends.

**D20. Design system.** Existing components only, plus two additions recorded in
`docs/design-system/README.md`: the explanatory tooltip variant of D7 and the "Report (stock
transactions)" row in the page archetypes. Change badges, table rules, the milestone date chip, the
filter badge, the modal footer and the copy rules apply as written there. Checked at 1080 px and in
both themes.

## 5. Testing

Rust service tests on an in-memory database with the hand-written minimal schema
(`docs/standards/testing.md`), at least:

- FIFO matching: a sale across two lots; a lot consumed by two sales; same-day buy and sale.
- Both cost methods on the same history, with fees, in two currencies with different day rates.
- The test boundary: sold on the last non-exempt day and on the first exempt day; a purchase on the
  29th–31st with a length that ends in a shorter month; a custom length.
- Period boundaries, both days included; a sale in the period whose lots were bought before it.
- Sums: taxable + exempt = total for every column; fees of D8 against the paid fees of D9.
- Open lots and their pass day; a lot partly consumed.
- An oversold position, a missing rate, an estimated rate, a position without transactions.
- Dividends in the period, Yahoo and manual.

Frontend: tests for the filter-and-total utilities. Components and hooks are out of scope by policy.

## 6. Documents to write with the feature

- ADR 0036: lots matched oldest-first exist only in this report; the cost basis elsewhere stays the
  weighted average (ADR 0001).
- `docs/design-system/README.md` (D20), the feature list in the README, all ten locales (a new
  `stockTransactions` namespace and `nav.stockTransactions`).

## 7. Calls made in this spec that the owner has not ruled on

1. The test is off for everyone at first, also for a Czech profile, with three years preselected.
2. The settings (method, test, length) are stored and synced; the period is not.
3. The export saves the rows the ledger shows; "Výsledek prodejů" has no export.
4. The modal's "Už prošlo" tab.
5. A sale that sells more than was bought, or cannot be converted, stays out of the sums with a
   note (D15, D16).
6. (Superseded by ruling 8: no country is named at all.)
7. No MCP tool for the report.

## 8. Left to the builder

The shape of the response types, how the service is split into files, whether the dividend payout
type is extended with shares held and amount per share or the report derives them, the chart
component (the existing bar chart wrapper or a variant of it), and the exact wording in the nine
languages other than Czech.
