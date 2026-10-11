# Stocks analysis: performance first, one tag per group, custom periods

Approved by the owner on 2026-10-11 after four prototype rounds. Visual reference:
`docs/specs/2026-10-11-stocks-analysis-prototype.html` (open it through the `docs` launch
config; switch on "Poznámky k návrhu" for the numbered decisions). The prototype is a frozen
snapshot like this spec; it fakes its numbers from synthetic daily prices. Where the two
differ, this spec wins. Two prototype-only things are not built: the allocation ring toggle
(the ring is dropped) and the proto bar.

## Problem

- **The TWR chart is too small.** It gets half the width and 190 px next to the allocation
  ring, so three or four lines tangle.
- **The time axis has no dates.** Six labels are spread evenly and show month names only, so
  months repeat and the year is missing.
- **The page cannot answer the usual questions.** Did my money do as well as my
  investments? How much of the growth did I deposit? Which position made the return? How deep
  was the worst drop? Would an index ETF have done better?
- **A position can carry several tags of one group.** The ring splits such a position, the
  tag table counts it twice and the TWR line counts it in full, so the same tag shows three
  different values.
- **The controls are not self-explanatory.** The owner did not understand the chip filters
  and the "compare by" switch on first sight.

## Decisions

1. **A tag group is one partition of the portfolio.** A position has at most one tag per
   group. Tags without a group stay free, overlapping labels: they filter and appear in the
   tags table, but the portfolio is never split by them.
2. **Legacy conflicts are fixed by a migration**, not by an on-page warning. For every
   position and group the first assigned tag stays (earliest `created_at` of the assignment,
   then tag name); the other assignments of that group are deleted. Tags themselves stay.
3. **The rule is enforced on every write.** The assign dialog offers one choice per group and
   the choice replaces the position's current tag of that group. The tag service rejects a
   second tag of a group. Moving a tag into a group is rejected when a position would then
   carry two tags of that group, and the dialog names those positions.
4. **Performance is the hero.** A full-width card holds three figures (time-weighted return
   of the whole portfolio, money-weighted return, change of value split into net deposits and
   market), the controls, the TWR chart (280 px) with a drawdown panel under it on the same
   time axis, and a table under the chart that doubles as its legend. The stats row and the
   allocation ring are removed.
5. **"Rozdělit podle" (formerly "Porovnat podle")** chooses how the selected positions are
   split into lines and rows: by a tag group, or by the automatic groups Sektor and Měna. A
   sentence under the control says what the lines add up.
6. **Automatic groups.** Sektor comes from the quote data (`stock_data.sector`; an ETF has
   none and falls into "Bez sektoru"), Měna from the position's quote currency. An automatic
   group is offered only when it splits the held positions into at least two parts (counting
   "none" as a part). Default group: the first tag group by creation date, else Sektor, else
   Měna, else none (only the portfolio line).
7. **"Které pozice analyzovat"** is a flat card around the chip filters with a live summary
   and a rule sentence. One tag or "Vše" per group, AND across groups. The hero figures and
   the dashed portfolio line always stay whole-portfolio and the sentence says so when a
   selection is active.
8. **Periods:** 3 měsíce · Letos · Rok · 3 roky · Vše · Vlastní. Vlastní shows Od/Do date
   inputs (the bank ledger pattern) and a range can also be dragged in the chart. Values in
   tables are shown as of the end of the period; unrealized gain and dividends stay as of
   today and their headers say so. The page remembers the period, the custom range and the
   chosen group per device.
9. **Index comparison** is optional: S&P 500 (through CSPX.L) or MSCI World (through IWDA.L),
   fetched from Yahoo on demand when the user turns it on, never stored.
10. **Contribution and deposits cards.** "Příspěvek k výnosu" shows how many percentage points
    of the portfolio return each position (or each part of the chosen group) brought;
    "Vklady a zhodnocení" shows net deposits against market change per week, month, quarter
    or year.

## Backend

### Migration `015_one_tag_per_group`

Deletes every assignment `(investment_id, tag_id)` whose tag belongs to a group in which the
same investment has another assignment that is older (`created_at` lower) or equally old with
a tag name sorting earlier. Ungrouped tags are untouched. No schema change; the schema
snapshot does not change. `docs/architecture/database.md` notes the rule on
`stock_investment_tags`.

### Tag service (`src-tauri/src/services/stock_tags.rs`)

The tag write logic moves out of `commands/stock_tags.rs` (which becomes thin):

- `set_investment_tags(conn, investment_id, tag_ids)`: `NotFound` for an unknown investment,
  `Validation("validation.tagNotFound")` for an unknown tag,
  `Validation("validation.oneTagPerGroup")` when two of the tags share a group. Assignments
  that stay keep their `created_at`; only removed ones are deleted and only new ones inserted.
- `update_tag(conn, id, data)`: when `group_id` changes to a group, returns
  `Validation("validation.tagGroupConflict")` if any investment carrying this tag already
  carries another tag of the target group.
- `create_tag`, `delete_tag` and the group functions move as they are. `InsertStockTag` and
  `InsertStockTagGroup` get `validate()` (trimmed name required, at most 60 characters):
  `validation.tagNameRequired`, `validation.tagNameTooLong`.

### Analysis items get the sector

`StockInvestmentWithTags` gains `sector: Option<String>` from `stock_data.sector` (trimmed,
empty is `None`). The currency of the Měna group is `priceCurrency`, falling back to
`averagePriceCurrency`.

### `get_stock_performance` (`src-tauri/src/services/stock_performance.rs`)

One command replaces `get_stock_twr` (removed with `twr_series`, its wrapper and the
`TwrDataPoint` / `TwrSeries` / `StockTwr` types once the page no longer uses them).

Request `StockPerformanceRequest { fromTs, toTs, reportCurrency, series: [{ key,
investmentIds, currency? }] }`. Validation: `fromTs < toTs`, at most 64 series, non-empty
unique keys.

The scope is every held position (quantity > 0), as the old portfolio series. A daily engine
(the chain-linking of the old `compute_twr_for_tickers`, generalised) computes for every held
position and every day `s` of the clamped range, in the report currency:

- `base_p(s) = V_p(s−1) + F_p(s)` and `gain_p(s) = V_p(s) − V_p(s−1) − F_p(s)`, where `V` is
  the value converted at its day's rate and `F` the flow (quantity change × the day's native
  price, converted at that day). A holding whose conversion fails drops out of that day only
  and is counted in `fxQuality`, as today.
- Portfolio daily return `R(s) = Σ gain / Σ base`, growth `G(s) = Π (1 + R)`, cumulative TWR
  `G − 1`.
- For a set `S` of positions: its own TWR chains `Σ_S gain / Σ_S base`; its contribution to
  the portfolio return is `C_S(t) = Σ_{s ≤ t} G(s−1) · Σ_S gain(s) / Σ_all base(s)`. The
  contributions of a partition of the held positions add up exactly to the portfolio TWR.
- A series with `currency` set is computed with values and flows in that currency (the native
  return of a currency group) and has no contribution.

Response `StockPerformance`:

| Field | Meaning |
|---|---|
| `currency`, `fxQuality` | Report currency and conversion quality |
| `dates` | Emitted days `YYYY-MM-DD`, at most 400 (every n-th day on long ranges, the last day always) |
| `portfolio` | Cumulative portfolio TWR in percent per date (first 0) |
| `values` | Portfolio value per date (decimal TEXT; `null` when no held position converts that day) |
| `netFlows` | Net deposits (buys − sells) cumulated since the first date, decimal TEXT |
| `mwr` | Money-weighted return from the first date to each date (modified Dietz, cumulative, percent): `(V_t − V_0 − F_t) / (V_0 + Σ F_s · (t − s + 1)/(t − t_0))`, 0 when the denominator is not positive |
| `series` | `{ key, twr[], contribution[] \| null }` per requested series, on the same dates |
| `positions` | Per held position: `investmentId`, period `twr`, `contribution` (percentage points), `startValue`, `endValue` (decimal TEXT or `null`) |

### `get_stock_benchmark`

`get_stock_benchmark(ticker, fromTs, toTs, reportCurrency)` fetches daily closes with
`price_api::get_historical_stock_prices_yahoo` (a few days before `fromTs` for the base) and
converts them in `stock_performance::benchmark_series(conn, prices, …)` at each day's rate.
Response `StockBenchmark { ticker, currency, fxQuality, dates, twr }` with daily points
(carry-forward over non-trading days). Offline or unknown ticker returns empty `dates`; the
UI then says the index could not be loaded. Ticker validation: 1–24 characters of
`A–Z 0–9 . - ^ =`.

## Frontend

### Page structure (`src/pages/StocksAnalysis.tsx` + `src/components/stocks-analysis/`)

1. Page head (unchanged) and the conversion quality note.
2. **ScopeCard** "Které pozice analyzovat": chip rows per tag group (by creation date) and a
   "Štítky" row for tags without a group. From the fourth row on, the rest hide behind
   "Další filtry"; an active filter of a hidden row shows as a removable chip. Summary right
   ("Všech 9 pozic · 1 095 435 Kč", or "2 z 9 pozic · 136 500 Kč" + "Zrušit výběr"). Footer:
   rule sentence left, untagged count + "Přiřadit štítky →" right.
3. **PerformanceCard** (hero material):
   - Figures: "Časově vážený výnos · celé portfolio" (display size), with best and worst
     drawn part, or the gap to the index when one is on; "od D. M. RRRR do dnes · N pozic".
     "Peněžně vážený výnos" (stat size) with "zohledňuje, kdy jste vkládali" and the gap to
     the TWR in p. b. ("stejně jako časově vážený" when it rounds to zero). "Změna hodnoty"
     with "čisté vklady" and "zhodnocení trhu".
   - Toolbar: "Rozdělit podle" segmented (manual groups, a divider, automatic groups; a select
     when there are more than four options), "Srovnat s indexem" menu button, "Období"
     segmented. Under it: the split caption left, the custom range inputs right.
   - Chart: TWR lines (up to four parts in s1–s4 by value, the portfolio dashed
     `chart-line`, the index dotted `ink-4`), end labels in the right gutter, drawdown panel
     ("PROPAD OD MAXIMA", the portfolio as an area), one date axis. Hover: crosshair,
     tooltip sorted by value, and the hero figures and the board rewrite to that day. Drag:
     sets a custom period.
   - Board (legend table): group label, Výnos za období, Proti portfoliu, Příspěvek, Max.
     propad, Podíl na hodnotě (výběru), Hodnota (or "K D. M. RRRR"); rows per part, the
     portfolio (reference), the index, "Bez štítku/sektoru"; undrawn parts with a hatched
     swatch. Hover highlights the line; clicking a manual tag row sets that filter. Caption
     under it (rule of the group).
4. **ContributionCard** "Příspěvek k výnosu" (`Pozice` / `Podle skupiny`) and **FlowsCard**
   "Vklady a zhodnocení" side by side (`1fr 1fr`, equal height).
5. **TagsTable**: Štítek (+ group name or "bez skupiny"), Pozice, Hodnota, Podíl, Výnos za
   období, Nerealizovaný zisk (dnes), Dividenda ročně.
6. **HoldingsTable**: Akcie (ticker · currency · sector or ETF), Štítky, Hodnota, Výnos za
   období, Nerealizovaný zisk, tag action; subtitle with the concentration ("5 největších
   tvoří 77 % · efektivně 6,6 pozice").

### Data flow

- Queries: `stock-tags`, `stock-tag-groups`, `stocks-analysis` (positions with tags and
  sector), `stock-performance` (one request per page state: every tag series, the parts of
  the chosen group, native currency series when the group is Měna), `stock-benchmark` (only
  when an index is chosen). Previous data stays visible while the next period loads.
- Pure logic in `src/utils/stock-performance.ts` (tested): period bounds, range validation,
  date axis ticks, drawdown, flow buckets, comparison groups and series specs, board rows,
  contribution rows, concentration.
- Every query key that was `stock-twr` becomes `stock-performance` (investment mutations,
  stock import, history recalculation, price refresh, tag dialogs).

### Copy

All strings in all ten locales (`reports` namespace, `stocksAnalysis.*`). The Czech copy of
the prototype is the reference. Keys of the removed stats row, ring and old TWR card are
deleted from every locale.

## Testing

- Rust: migration (keeps the oldest per group, tie by name, leaves ungrouped tags and other
  groups); tag service validation; performance engine (contributions of a partition sum to
  the portfolio TWR; a deposit is not return; MWR differs from TWR when a deposit precedes a
  drop; native-currency series; clamping; emission limit); benchmark conversion and
  carry-forward.
- TypeScript: every function in `src/utils/stock-performance.ts`.
- Manual: the demo profile (copied into the worktree) on port 1422.

## Out of scope

Dividend calendar (payouts are not recorded), the Czech three-year time test, an MCP tool for
performance, storing index history.
