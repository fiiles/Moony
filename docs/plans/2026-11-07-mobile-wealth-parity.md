# Plan: mobile wealth parity

Spec: `docs/specs/2026-11-07-mobile-wealth-parity-design.md`.

1. **B12 axis labels.** `src/utils/chart-axis.ts` `distinctTickLabels` (+ tests),
   `src/components/charts/use-tick-formatter.ts`; used in the four charts. Test: chart-axis.test.
2. **B7 / B8 badges.** `ChangeLine.tsx`, `Hero.tsx` (`changeDirection`, aria); `PositionRow`,
   `OtherAssets`, `Wealth` pass `trail`; key `mobile:wealth.stocks.sincePurchase` in ten locales.
3. **B10 valued day.** `utils/other-assets.ts` `latestValuationDay` (+ test),
   `hooks/use-other-asset.ts` `useValuedDays`, `use-wealth.ts`, `OtherAssets.tsx`.
4. **A9 crypto banner.** `pages/Crypto.tsx`.
5. **A10 quality notes.** `lib/performance-notes.ts` (+ test), `use-stock-positions.ts`, `Stocks.tsx`.
6. **A11 splits.** `PositionMenu.tsx`, `AppliedSplits.tsx`, `PositionDetail.tsx`.
7. **A13 watch marks.** `WatchDetail.tsx`.
8. **A14 property notes.** `components/property/PropertyNotes.tsx`, `PropertyDetail.tsx`.
9. **A15 yield.** `OtherAssetDetail.tsx`.
10. Gates: typecheck, lint, format:check, full vitest; self-review; report.
