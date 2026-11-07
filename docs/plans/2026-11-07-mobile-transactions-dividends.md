# Phone: manual transaction, undo import, dividends — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** close audit items A1–A3: "Přidat transakci" and "undo an import" on the phone account,
and received dividends (rows, filter, entry, realized note) on the phone stock position.

**Architecture:** phone-native sheets (`AmountField`/`DateField`, the `CostSheet` pattern) over
existing desktop mutations. Pure form and ledger logic in `src/mobile/lib` with Vitest tests.
No backend change.

**Tech Stack:** React 19, TypeScript, TanStack Query, react-i18next, Vitest, Tailwind.

**Spec:** `docs/specs/2026-11-07-mobile-transactions-dividends-design.md`

## Global Constraints

- Never call `invoke()` outside `src/lib/tauri-api.ts` (AGENTS rule 1).
- Writes go through the existing hooks: `useBankTransactionMutations().createTransaction` and
  `useStockDividendMutations()`. Undoing an import calls `bankAccountsApi.deleteImportBatch` and
  then `refreshMoneyFlows(queryClient)` (rule 7).
- Every new user-facing string goes in all ten locales (`en cs de es it nb pl sv ja zh`), or
  `src/i18n/locales.test.ts` fails. Reuse the desktop keys named in the spec; add a mobile key only
  where no desktop string exists.
- Phone fields:
  - `AmountField` for amounts (never `Input type="number"`); `DateField` for days;
  - plain text fields use the `TEXT_FIELD` classes from `src/mobile/sheets/CostSheet.tsx`
    (48 pt, `text-m-field`). Move that constant to a shared module if a second sheet needs it.
- A day is `isoToUtcDay(iso)` (`src/mobile/lib/property-detail.ts`) and today is `todayIso()`.
  Never use `new Date(iso)` arithmetic.
- An amount is read with `parseAmountInput` (`src/mobile/lib/amount-input.ts`). Its errors are
  `common:validation.amountRequired`, `validation.invalidAmount` and `validation.amountPositive`,
  as `CostSheet` uses them.
- Sheet footers use `FOOTER_GHOST` / `FOOTER_PRIMARY` from `src/mobile/components/import/styles`.
- Gates before every commit: `npm run typecheck`, `npm run lint`, `npm run format:check`, and
  `npm test` for the touched test files. No `src-tauri` change.

## Review Focus

1. The amount is typed with a comma ("1 234,50") on a Czech keypad: it must save 1234.5, not
   refuse it or save 1.
2. The ledger with the filter on "Dividendy" when the position has no dividends: an empty-state
   line, no crash. A chart mark tapped while "Dividendy" is selected must switch the filter to a
   view that holds the trade (`revealRow`).
3. A superseded manual dividend: dimmed and struck through, with Smazat but no Upravit. A Yahoo
   row: no menu at all.
4. An import with `linkedCount = 0` (all its rows were duplicates): the label and the title are
   `readiness.deleteRecord`, and the description still reads correctly with count 0.
5. Reopening any of the new sheets after a cancel starts from a clean form (no stale amount, no
   stale error).

---

### Task 1: Přidat transakci

**Files:**
- Create: `src/mobile/lib/manual-transaction.ts`, `src/mobile/lib/manual-transaction.test.ts`
- Create: `src/mobile/sheets/AddTransactionSheet.tsx`
- Modify: `src/mobile/sheets/CategoryChoiceSheet.tsx` (optional `noneLabel?: string`,
  `noneSub?: string`, replacing the "Podle pravidel" label/sub when given; the import preview is
  unchanged)
- Modify: `src/mobile/pages/finance/AccountDetail.tsx` (a `'addTransaction'` sheet state; `···`
  item `bank_accounts:transaction.add` right after Upravit účet)

**Interfaces:**
- Produces:
  - `interface ManualTxForm { type: 'debit' | 'credit'; amountText: string; dateIso: string; counterparty: string; description: string; counterpartyIban: string; categoryId: string | null }`
  - `emptyManualTxForm(today: string): ManualTxForm`
  - `manualTxErrors(form: ManualTxForm): { amount?: 'required' | 'invalid' | 'notPositive'; date?: 'required' }`
  - `manualTxInput(form: ManualTxForm, accountId: string, currency: string): InsertBankTransaction | null`

- [ ] **Step 1: Write the failing tests** in `manual-transaction.test.ts`:
  - `emptyManualTxForm('2026-11-07')` → type `'debit'`, dateIso `'2026-11-07'`, empty texts,
    categoryId `null`.
  - `manualTxErrors`:
    - `amountText ''` → `{ amount: 'required' }`;
    - `'abc'` → `'invalid'`;
    - `'0'` and `'-5'` → `'notPositive'`;
    - `'1 234,50'` → no amount error;
    - `dateIso ''` → `{ date: 'required' }`.
  - `manualTxInput({ ...form, amountText: '1 234,50', dateIso: '2026-11-07', counterparty: '  Albert ', description: '', counterpartyIban: '', categoryId: 'c1' }, 'acc', 'CZK')` →
    `{ bankAccountId: 'acc', type: 'debit', amount: '1234.50' /* whatever parseAmountInput returns for it */, currency: 'CZK', bookingDate: isoToUtcDay('2026-11-07'), description: null, counterpartyName: 'Albert', counterpartyIban: null, categoryId: 'c1', status: 'booked' }`.
    Assert `amount` equals `parseAmountInput('1 234,50')`.
  - `manualTxInput` → `null` when there is any error.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/manual-transaction.test.ts`. Expected: FAIL
  (module missing).
- [ ] **Step 3: Implement the lib.** Trim the texts; empty → `null`.
- [ ] **Step 4: Run the test.** Expected: PASS.
- [ ] **Step 5: Build `AddTransactionSheet`** (`open`, `onOpenChange`, `accountId`, `currency`,
  `categories`), as spec §A1 describes:
  - `fill full` sheet;
  - field order Segment → Částka (autofocus once open) → Datum → Protistrana → Popis → Kategorie
    row → IBAN;
  - errors shown after the first save attempt;
  - the form resets on every opening (the `openedFor` pattern from `ExtraPaymentSheet`).
  - The category row:
    - is a `PickRow`-like button labelled `bank_accounts:transaction.category`, whose value is the
      category name (`useCategoryName`) or `mobile:account.ledger.noCategory`;
    - opens `CategoryChoiceSheet` with `noneLabel = mobile:account.ledger.noCategory` and
      `noneSub` = a new mobile key `account.addTx.noCategorySub` ("Zařadíte ji později", in all ten
      locales).
  - Save calls `createTransaction.mutate(input, { onSuccess: close })`. The primary button is
    disabled while the mutation is pending.
- [ ] **Step 6: Wire it into `AccountDetail`.**
- [ ] **Step 7: Gates** (typecheck, lint, format:check, `npx vitest run src/mobile src/i18n`).
- [ ] **Step 8: Commit** `feat(mobile): add a manual transaction from the account menu`.

### Task 2: Vrátit import

**Files:**
- Create: `src/mobile/hooks/use-undo-import.ts`
- Modify: `src/mobile/sheets/ImportHistorySheet.tsx` (prop `onUndo: (batch: CsvImportBatch) => void`; row menu; `to ?? from`; docstring)
- Modify: `src/mobile/pages/finance/AccountDetail.tsx` (`pendingUndo` state + `ConfirmSheet`)
- Modify: `src/i18n/locales/*/mobile.json` (one key: `account.importHistory.menu`, the row menu's
  accessible name, e.g. cs "Nabídka importu")

**Interfaces:**
- Produces: `useUndoImport(): UseMutationResult<number, Error, string>`. On success it awaits
  `refreshMoneyFlows`, then toasts `bank_accounts:readiness.batchDeleted` with the count. On error
  it toasts `messages.error` with `translateApiError`.

- [ ] **Step 1: Write the hook.** Copy the desktop's `deleteBatchMutation`
  (`src/pages/BankAccountDetail.tsx:359`) without the page state.
- [ ] **Step 2: Add the row menu.** In `ImportHistorySheet`, each `EntityRow` gets
  `onClick`/`onLongPress` that open a `DropdownMenu` whose trigger is the `sr-only` anchor from
  `TradeRow`. The menu has one `RowMenuItem variant="danger"` with an `Undo2` icon, labelled
  `readiness.deleteImported` if `linkedCount > 0`, else `readiness.deleteRecord`. Selecting it
  calls `onUndo(batch)`.
- [ ] **Step 3: Wire the confirm sheet in `AccountDetail`.** `onUndo` closes the history sheet,
  then sets `pendingUndo`. Use `leaveAfterSheet` from `@/mobile/hooks/use-leave-after-sheet` so
  that the second sheet opens after the first has gone. The `ConfirmSheet`:
  - title and confirm label are the menu label;
  - its description is the desktop's joined sentence (`BankAccountDetail.tsx:1381-1393`, same keys
    and params, period `fmt.day(from) – fmt.day(to ?? from)` or `readiness.unknownDate`);
  - `onConfirm` → `undo.mutate(batch.id, { onSuccess: () => setPendingUndo(null) })`;
  - `isPending` comes from the mutation.
- [ ] **Step 4: Gates**, then commit `feat(mobile): undo an import from Historie importů`.

### Task 3: Dividends in the position ledger and stats

**Files:**
- Modify: `src/mobile/lib/position-series.ts` + `position-series.test.ts`
- Modify: `src/mobile/hooks/use-position.ts` (dividends query), `src/mobile/hooks/use-position-ledger.ts`
- Create: `src/mobile/components/wealth/DividendRow.tsx`
- Modify: `src/mobile/components/wealth/PositionLedger.tsx`, `PositionStats.tsx`, `src/mobile/pages/PositionDetail.tsx`
- Modify: `src/i18n/locales/*/mobile.json` (one key: `wealth.position.dividend.menu`, the dividend
  row menu's accessible name, e.g. cs "Nabídka dividendy")

**Interfaces:**
- Consumes: `StockDividend`, `StockDividendList` (`@shared/schema`).
- Produces:
  - `type LedgerFilter = 'all' | 'buy' | 'sell' | 'dividend'` (replaces `TxFilter`; keep
    `TxFilter` as an alias if crypto code imports it);
  - `type LedgerEntry<T extends LedgerTx> = { kind: 'trade'; id: string; date: number; tx: T } | { kind: 'dividend'; id: string; date: number; dividend: StockDividend }`;
  - `ledgerEntries<T>(transactions: readonly T[], dividends: readonly StockDividend[], filter: LedgerFilter): LedgerEntry<T>[]`.
    It sorts newest first and stays stable for the same day: trades before dividends in input
    order, as the desktop does. `'buy'`/`'sell'` drop the dividends; `'dividend'` drops the trades.
  - `revealRow` takes `(transactions, dividends, filter, id, fold?)` and works on `ledgerEntries`.
  - `usePosition(...)` returns `dividends: StockDividend[]`, `dividendsFromYahoo: boolean`,
    `dividendCurrency: string`.
  - `usePositionLedger(transactions, dividends = [])` returns entries in `rows`/`shown`.
  - `PositionLedger` takes optional `dividends`, `onEditDividend?: (d: StockDividend) => void` and
    `deleteDividend?: { mutate; isPending }`. When `dividends` is given, it adds the fourth segment
    option and counts dividends.
  - `PositionStats` takes optional `dividendsReceived?: number | null`.

- [ ] **Step 1: Write the failing tests** in `position-series.test.ts`:
  - `ledgerEntries`, `'all'`: the trade and the dividend are merged newest first. On the same day
    the trade comes first.
  - `ledgerEntries`, `'dividend'`: only dividends. `'sell'`: only sells.
  - `ledgerEntries`, `'dividend'`, no dividends: `[]`.
  - `revealRow` with filter `'dividend'` and the id of a buy → `filter: 'all'`.
  - `revealRow` with a buy whose index in `'all'` is ≥ 8 → `expand: true`.

  Update the existing `filterTransactions`/`revealRow` tests to the new signature, and keep
  `filterTransactions` if other code still uses it.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/position-series.test.ts`. Expected: FAIL.
- [ ] **Step 3: Implement the lib changes.** Then run the test. Expected: PASS.
- [ ] **Step 4: Add the query to `usePosition`.**
  - Key `['stock-dividends', id, currencyCode, fxRevision]`, fn `investmentsApi.getDividends(id, currencyCode)`.
  - `enabled` follows the page's `live` flag, like the other queries.
  - `dividendCurrency` = `holding.dividendCurrency ?? holding.originalCurrency`.
  - Feed `useTransactionFx` the trades plus `dividends.map(d => ({ transactionDate: d.exDate }))`.
    Check its parameter type; the desktop does this at `StockDetail.tsx:175-183`. A dividend
    older than the first trade must still get a rate.
- [ ] **Step 5: Build `DividendRow`** per spec §A3:
  - `id={`tx-${dividend.id}`}`;
  - the same `hot` tint as `TradeRow`;
  - convert with `convertAt(gross, dividend.currency, currency, dividend.exDate)`;
  - the menu only for `source !== 'yahoo'`;
  - a Yahoo row gets no `onClick`.
- [ ] **Step 6: Update `PositionLedger`.**
  - Render entries, a trade through `TradeRow` and a dividend through `DividendRow`.
  - Add the dividend delete `ConfirmSheet`: `stocks:detail.dividendRecords.confirmDelete.*`, date
    `fmt.day(exDate)`, amount `moneyText(Number(grossAmount), currency)`.
  - Crypto's call site is unchanged.
- [ ] **Step 7: Update the stats.**
  - `PositionStats`: the realized note is
    `[sells > 0 ? detail.sells : detail.noSells, dividendsReceived > 0 ? detail.realizedDividends({ amount }) : null]`
    joined by `' · '`. The amount uses the stat money format.
  - `useDividendStat`: append `` ` · ${ts('detail.manual')}` `` when `holding.isManualDividend`
    and there is a dividend.
- [ ] **Step 8: Wire `PositionDetail`.**
  - Pass `dividends`, `deleteDividend` from `useStockDividendMutations()`, and
    `amountOf(view.position?.dividendsReceived)`.
  - `onEditDividend` is a no-op placeholder that Task 4 replaces. Leave it out instead if that is
    cleaner: the row then shows Smazat only.
- [ ] **Step 9: Gates**, then commit `feat(mobile): received dividends in the position ledger and stats`.

### Task 4: Zapsat / upravit dividendu

**Files:**
- Create: `src/mobile/lib/dividend-form.ts` + `dividend-form.test.ts`
- Create: `src/mobile/sheets/DividendSheet.tsx`
- Modify: `src/mobile/components/wealth/PositionMenu.tsx` (`onAddDividend` prop; first item
  `Coins` + `stocks:detail.dividendRecords.add`, then a separator)
- Modify: `src/mobile/pages/PositionDetail.tsx` (state `{ open, dividend: StockDividend | null }`;
  `onAddDividend`, `onEditDividend`)

**Interfaces:**
- Produces:
  - `interface DividendForm { dateIso: string; amountText: string; note: string }`
  - `dividendForm(dividend: StockDividend | null, today: string): DividendForm`. An edit takes the
    stored values: `isoDateFromUtcTimestamp(exDate)` and `String(Number(grossAmount))`.
  - `dividendErrors(form): { amount?: 'required' | 'invalid' | 'notPositive'; date?: 'required'; note?: 'tooLong' }`.
    `note` is checked after trimming; 500 characters are allowed.
  - `dividendInput(form, currency): InsertStockDividend | null`, with `exDate = isoToUtcDay`, the
    parsed `grossAmount`, the trimmed note or `null`.

- [ ] **Step 1: Write the failing tests:**
  - the new form defaults to today and empty fields;
  - the edit form round-trips: `grossAmount '12.50'` → `amountText '12.5'`;
  - errors cover empty, invalid and 0 amounts and a 501-character note;
  - `dividendInput` gives `'1 234,5'` → `parseAmountInput` output, `exDate` from `isoToUtcDay`,
    and `note '  '` → `null`.
- [ ] **Step 2: Run the test.** Expected: FAIL. **Step 3:** implement. **Step 4:** run again.
  Expected: PASS.
- [ ] **Step 5: Build `DividendSheet`** (`open`, `onOpenChange`, `investmentId`, `ticker`,
  `currency`, `dividend: StockDividend | null`, `fromYahoo: boolean`) per spec §A3:
  - with `fromYahoo`, an `InlineAlert` and disabled inputs and save;
  - the currency is `dividend?.currency ?? currency`;
  - save goes through `createDividend` / `updateDividend`, closing on success;
  - the form resets per opening.
- [ ] **Step 6: Wire `PositionMenu` and `PositionDetail`.** The row's Upravit opens the sheet with
  that dividend.
- [ ] **Step 7: Gates**, then commit `feat(mobile): enter and edit a received dividend`.
