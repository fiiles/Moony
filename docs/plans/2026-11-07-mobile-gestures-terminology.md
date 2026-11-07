# Plan: phone gesture rule and terminology

Spec: `docs/specs/2026-11-07-mobile-gestures-terminology-design.md`. Branch
`feat/mobile-audit-gestures`. Gates before every commit: `npm run typecheck`, `npm run lint`,
`npm run format:check`, the touched `npx vitest run …`.

## Task 1: row intents (pure, TDD)

- `src/mobile/lib/row-intent.ts`: `ROW_INTENTS`, `type RowIntent`, `intentState(base, intent)`
  (the history state of a push carrying an intent beside the back label), `readIntent(state)`
  (`RowIntent | null`, unknown values ignored), `withoutIntent(state)` (the state minus the intent,
  other keys kept).
- `src/mobile/lib/row-intent.test.ts`.
- `src/mobile/hooks/use-row-intent.ts`: `useRowIntent(handlers, ready)` — once `ready`, reads
  `history.state`, replaces the entry without the intent, runs the handler; `useOpenWithIntent()`
  returns `(path, intent, from?) => setLocation(path, { state })`.

## Task 2: EntityRow menu and swipe tone

- `RowMenu`: `shown` prop (visible 44 pt `···`, `ink-4`), one trigger for every row.
- `EntityRow`: `menu` prop `{ label, children, shown?, open?, onOpenChange? }` replacing
  `onLongPress` + `accessory`; the long press opens it; the trigger slides with the row; a shown
  `···` reserves 48 px at the row's end. `swipeLeftDestructive` paints the right panel `loss`.
- Migrate every row: LedgerRow, QueueRow, AgendaRow, DrilldownRow, Budgets, CustomRuleRow,
  LearnedPayeeRow, CategoriesSection, LoanEvents, BondRow, TradeRow, DividendRow, DocumentRow,
  ExpenseCategoryRow, PlanRowItem, PropertyCosts, PropertyFinance, ImportHistorySheet.

## Task 3: the swipe rule on rows

- QueueRow: swipe left = Kategorie (picker), swipe right = Podrobnosti, menu shown.
- DrilldownRow: swipe right = Podrobnosti; menu Podrobnosti + Kategorie (the ledger's words).
- Destructive panels: Budgets (Smazat), CustomRuleRow, LearnedPayeeRow, CategoriesSection.
- `shown` per the spec's table.

## Task 4: main-row menus

- Finance (accounts, loans, policies), PositionRow (stocks, crypto), WatchedRow, Wealth property
  rows, OtherAssets rows: `menu` with the spec's items through `useOpenWithIntent`; account import
  through `useImportSheet`.
- Details consume intents: AccountDetail (record, addTransaction), LoanDetail (balanceCheck,
  rateChange), PolicyDetail (edit, end), PositionDetail / CryptoDetail (buy, sell), WatchDetail
  (target, buy), PropertyDetail (revalue), OtherAssetDetail (buy, sell, revalue).

## Task 5: terminology

- Script-applied locale edits (all ten locales): reworded values, new keys
  (`stocks|crypto:detail.confirmDeleteTransaction.descriptionBuy|Sell`,
  `otherAssets:confirmDeleteTransaction.descriptionBuy|Sell`, `otherAssets:detail.menu.edit|delete`),
  removed mobile keys now served by desktop keys.
- Code: Overview, Wealth, Finance, ClassHero, PositionHeroCard, WatchDetail, OtherAssetDetail
  (`dashboard:changePeriod.*`, `dashboard:stats.*`); AccountDetail ledger head; Cashflow moves;
  TransactionSheet labels and Zavřít; FilterSheet reset; search fields; Stocks stats; ExpensesSection
  + desktop ExpensesByCategoryCard (`budgeting:manage.title`); LoanMenu, PolicyDetail,
  OtherAssetMenu labels; trade delete sentence in PositionLedger, StockDetail, CryptoDetail,
  OtherAssetTransactionsModal; InvestmentsTable price label.
- Gate: `npx vitest run src/i18n`.

## Task 6: docs

- README §13: the gesture rule (new "Gestures" block), the swipe actions and row menu rows updated,
  `.m-swipe__panel--danger` and `.m-row__more` in `mobile.css`, a swiped destructive row and a
  visible `···` in `mobile.html`.

## Task 7: wrap-up

- `git merge main` once the finance branch lands (resolve keeping both sides), full `npx vitest run`,
  self-review of `git diff main...HEAD`, report `.superpowers/audit-report.md`.
