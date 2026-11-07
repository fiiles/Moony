# Mobile wealth parity (audit A9–A11, A13–A15, B7, B8, B10, B12)

Frozen snapshot, 2026-11-07. Source: `docs/audits/2026-11-06-mobile-ux-audit.md`. The desktop is the
reference ("desktopová verze je referenční bod").

## Why

The phone's Majetek pages dropped small things the desktop says or does: a coin without a rate
source is a dead end, the year's return reads as exact where the desktop doubts it, a missed split
cannot be fixed, a held position's trades are not on the watch chart, property notes are read-only,
an asset's yield is invisible. Two badge rules differ (a tiny change prints "▼ 0,0 %" in red), the
same badge means 30 days on one screen and "since purchase" on the next, "oceněno" is a touch date,
and axis labels repeat ("11,2 mil." twice).

## Goals

- A9 Crypto: the desktop's unpriced-coins banner (`crypto:unpricedBanner.*`) with its action.
- A10 Stocks: one `InlineAlert` under the year's return with the desktop's quality texts.
- A11 Position: "Zapsat split" in `···` and the applied splits listed under the ledger.
- A13 Watch detail: the "Nákupy a prodeje" chip and trade marks on the price line.
- A14 Property: notes edited in place (`updateNotes`).
- A15 Other asset: yearly yield (amount and type) on the detail.
- B7 Direction by `changeDirection`, glyph hidden from VoiceOver, spoken sign instead.
- B8 Each change badge names what it measures.
- B10 "oceněno" is the newest valuation's day.
- B12 No two ticks of an axis print the same label (desktop charts and the phone chart).

## Non-goals

No `src-tauri` change, no migration, no new desktop dialog. No change to Cashflow, shell or the
Overview beyond what B7 needs in `Hero.tsx`.

## Design

**A9.** `unpricedOpenCoins(rows)` (desktop helper) feeds an `InlineAlert` between the stats and the
list; its action opens the desktop's `CryptoSymbolDialog` for the first unpriced coin, as the
desktop does (the rest go through each coin's `···`).

**A10.** `performanceNotes()` (pure, `src/mobile/lib/performance-notes.ts`, tested) lists the
sentences in the desktop's order and words: `common:fxQuality.missing/estimated` and
`reports:stocksAnalysis.performance.unpriced/dividendGap/suspectTrade`. `useTwrThisYear` returns
them; `PortfolioStats` shows one `InlineAlert` (Info icon, sentences separated by line breaks)
when the return is shown and there is a note.

**A11.** `PositionMenu` gets "Zapsat split" (`stocks:detail.menu.recordSplit`) that asks the page to
open the desktop's `StockSplitsDialog` (it renders as a sheet in the phone shell). `AppliedSplits`
lists the applied splits under the ledger as a muted note with the `splits.appliedLine` text and a
"Spravovat splity" action that opens the same dialog.

**A13.** The held position's transactions (same query key as the desktop) become
`positionTradeEvents` marks on `Hero`; a `Chip` with the desktop's glyphs and text switches them
(per session, `useRemembered`). Marks before the period start are dropped, as on the position page;
the legend gains buy and sell while they show. The tooltip is `useTradeEventTip`.

**A14.** `PropertyNotes` mirrors `WatchNotes`' editor (text area, Zrušit, Uložit) over the desktop's
`useRealEstateMutations().updateNotes`. Plain text, as on the desktop.

**A15.** A one-stat row under the stats: the yearly yield in the yield's own currency with the
type text of the desktop's table ("5 % z hodnoty"); nothing when the yield is zero.

**B7.** `changeDirection(ratio, 1)` in `ChangeLine` and `Hero`; the ▲/▼ is `aria-hidden`, a
`sr-only` "+" or "−" is read instead, exactly as the desktop's `ChangeBadge`.

**B8.** **(call, revised after review)** Majetek class rows (stocks, crypto, cash) keep the amount
and say "za 30 dní" as the trailing text of the badge. Position rows (Akcie, Krypto, Ostatní
aktiva) keep the badge with the signed amount as on the desktop's table; the base is named once
per list by a caption beside the sort pill (Akcie, Krypto) or above the rows (Ostatní aktiva),
"zisk/ztráta od nákupu" (`mobile:wealth.gainSincePurchase`), and each row says "od nákupu" to VoiceOver
(`mobile:wealth.sincePurchase`, `ChangeLine.srLabel`). Cost if wrong: the caption takes a little of
the sort row's width; the class rows lose the amount that shared the line with the 30 days.

**B10.** `latestValuationDay()` (pure, tested, ordering as `OTHER_ASSET_PRICES`) and
`useValuedDays()` (the detail's valuation queries, fallback `updatedAt`). Used by the other-assets
list row and by Majetek's class row. **(call)** The desktop's "Aktualizováno" column is
`updatedAt`; the phone says "oceněno", which is a statement about the price, so the valuation day
wins. Cost if wrong: a row shows an older day than the desktop's column.

**B12.** `distinctTickLabels()` (pure, tested) picks the fewest compact decimals (1 to 3) that make
all ticks of the axis different; `useTickFormatter` applies it to `MoonyLineChart`, `MoonyBarChart`,
`MoonyBandChart` and `ScrubChart` when no custom `formatTick` is given. A custom `formatTick`
(the desktop watch chart) is kept and its labels size the gutter too; `MoonyLinesChart` is
untouched. The gutter follows the labels: `tickGutter()` (pure, tested; the measured per-glyph
widths of the 10 px axis font) keeps `AXIS_WIDTH` unless the
widest label needs more, and `useTickFormatter` returns it as `axisWidth` for the YAxis, the time
axis gutter, ScrubChart's plot width and the bar chart's label step.

## Not done / limits

- A9's banner opens the picker for the first coin only (desktop parity).
- The split dialog is the desktop's, not a purpose-built phone sheet **(call)**: it already renders
  as a sheet and carries every action; cost if wrong: it is less thumb-friendly than a native sheet.
