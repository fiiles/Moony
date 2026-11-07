# Phone: manual transaction, undoing an import, received dividends

Date: 2026-11-07 · Audit items A1, A2, A3 (`docs/audits/2026-11-06-mobile-ux-audit.md`) ·
Reference: the desktop (owner, 2026-11-06: "desktopová verze je referenční bod") · Owner delegated
the decisions; the calls marked **(call)** are mine.

## Why

Three gaps put the phone behind the desktop and break a promise of the mobile spec:

- **A1:** a cash account, or a bank that exports nothing, can only be kept as balances. Mobile spec
  §8.4 lists "Přidat transakci" in the account `···`, and §8.6 describes the sheet.
- **A2:** a wrongly imported statement cannot be taken back. Spec §8.6 says "Undo stays in
  Historie importů", but the sheet is read-only.
- **A3:** received dividends are counted but invisible. The position's "Realizovaný výnos" includes
  them while its note says "zatím žádný prodej", and a dividend cannot be listed or entered. This is
  the only place where the phone's numbers contradict their own text.

## Goals

1. **Přidat transakci** in the account `···`, as a phone-native sheet (spec §8.6), writing what the
   desktop's `AddTransactionModal` writes.
2. **Undo an import** from Historie importů, with the desktop's confirmation sentence and the
   desktop's refresh.
3. **Dividends in a stock position:**
   - The realized note names the dividend part.
   - The ledger lists dividends and has a "Dividendy" filter.
   - Manual dividends can be entered, edited and deleted.
   - A ticker that Yahoo covers explains why nothing can be entered by hand, as the desktop does.

## Non-goals

- App shortcut or Android long press to open "Přidat transakci" directly (spec §8.6). That needs
  native plumbing and is owner-side Phase 7.
- Editing a manual bank transaction (the desktop cannot either).
- Dividend marks on the position chart (the desktop has none).
- TWR quality notes (A10), splits (A11), crypto. Crypto has no dividends.
- Any backend change. Every command used here already exists.

## Anti-goals

- A phone form that refuses a comma, or zooms the page. Use `AmountField` and `DateField`
  (README §13), never `Input type="number"`.
- A number the phone shows differently from the desktop. The dividend sums and conversions are
  the backend's and the shared `convertAt`'s.
- A destructive action without the desktop's confirmation text.

## Design

### A1 · Přidat transakci (`AddTransactionSheet`)

Entry: account `···`, a second item "Přidat transakci" (`bank_accounts:transaction.add`) right after
Upravit účet, as on the desktop.

Fields, top to bottom (spec §8.6 order, desktop fields):

1. **Odchozí · Příchozí** `Segment` (`transaction.debit` / `transaction.credit`). Starts on Odchozí,
   like the desktop's default `debit`.
2. **Částka** (`AmountField`, unit = account currency). Gets the focus when the sheet opens.
3. **Datum** (`DateField`, default today, no max, like the desktop).
4. **Protistrana** (`transaction.counterparty`, placeholder `counterpartyPlaceholder`).
5. **Popis** (`transaction.description`, placeholder `descriptionPlaceholder`).
6. **Kategorie**: a row that opens `CategoryChoiceSheet` (stacked). With no choice, the row says
   "Bez kategorie" and the transaction stays uncategorized, as on the desktop. The backend runs no
   rules for a manual entry, so the choice sheet's "Podle pravidel" first row must read as "Bez
   kategorie" here. Add an optional `noneLabel` prop; the import preview keeps its current label.
7. **Číslo účtu / IBAN protistrany** (`transaction.counterpartyIban`). Optional and last.

Footer: Zrušit · **Přidat transakci**.

- Validation: the amount is required and > 0 (`validation.amountRequired` /
  `validation.amountPositive`, read with `parseAmountInput`), and a date is required. Errors appear
  under the field after the first save attempt.
- Save calls `useBankTransactionMutations().createTransaction`, which already runs
  `refreshMoneyFlows` (rule 7) and toasts. Its payload matches the desktop's:
  - `bankAccountId`, `type`, `amount` (string), `currency` = account currency;
  - `bookingDate` = `isoToUtcDay(date)`;
  - `description`, `counterpartyName`, `counterpartyIban` and `categoryId`, with empty → `null`;
  - `status: 'booked'`.
- The sheet closes on success. On a refusal it stays open; the toast comes from the mutation.
- Every opening starts over. `fill`/`full` height, since the form is long.

**(call) Currency:** account currency only, with the unit fixed inside the field. A transaction in
another currency is entered on the computer. The same rule is already used by `CostSheet`. Cost
if wrong: adding a unit picker later.

### A2 · Vrátit import (Historie importů)

- Each row of `ImportHistorySheet` opens a row menu on tap or long press, with a way in for a
  keyboard (the `TradeRow` pattern).
- The menu has one danger item. Its label is the desktop's: `readiness.deleteImported` when
  `linkedCount > 0`, else `readiness.deleteRecord`.
- Choosing it closes the history sheet, then opens a `ConfirmSheet` owned by `AccountDetail`
  (desktop: closes the dialog, then confirms). The confirm sheet carries:
  - the same title and confirm label;
  - the description `readiness.deleteBatchDescription` (count = `linkedCount`, file, period or
    `readiness.unknownDate`) plus `readiness.deleteBatchPromoted` when `promotedCount > 0`.
- The mutation is `bankAccountsApi.deleteImportBatch` → `refreshMoneyFlows` → toast
  `readiness.batchDeleted`, and the sheet closes. On an error the toast is
  `messages.error` + `translateApiError`. It lives in a small mobile hook so the page stays thin.
- Remove the docstring line that says undo stays on the desktop.
- Fix in passing: a range without `to` shows `from – from`, as the desktop does.

### A3 · Dividends in a stock position

**Data.** `usePosition` also reads `['stock-dividends', id, currencyCode, fxRevision]` →
`investmentsApi.getDividends(id, currencyCode)`. This is the desktop's key, so
`useStockDividendMutations` and the dividend refresh invalidate it.
The dated-rate window (`useTransactionFx`) gets the dividends' ex-dates too
(`{ transactionDate: exDate }`, as the desktop's `datedFacts` does), so a dividend older than the
first trade still converts.

**Realized note.** `PositionStats` gets an optional `dividendsReceived: number | null`.
- When it is > 0, the realized stat's note is the desktop's join: "N prodejů · z toho dividendy X",
  or "zatím žádný prodej · z toho dividendy X" (`detail.sells`/`detail.noSells` +
  `detail.realizedDividends`).
- The value comes from `position.dividendsReceived`.
- The sixth stat (dividends of a year) appends " · ručně" (`detail.manual`) when
  `holding.isManualDividend`, as on the desktop.

**Ledger.**
- `PositionLedger` takes optional dividends. With them:
  - the segment has a fourth option "Dividendy" (`detail.ledger.filter.dividends`);
  - the count is trades + dividends;
  - the rows are trades and dividends merged newest first, with same-day order stable.
- The fold, the flash from a chart mark and `revealRow` work on the merged entries. A mark only
  ever points at a trade.
- Crypto passes no dividends and is unchanged.

**Dividend row** (`DividendRow`):
- Chip: `Coins` in the gain tone.
- Title: `detail.dividendRecords.badge`.
- Sub-line: the day, then a second part:
  - Yahoo: `detail.dividendRecords.perShare` with `priceDecimals`;
  - manual: its note, or nothing;
  - superseded: `detail.dividendRecords.superseded`.
- Value: "+ X" gross in the report currency at the ex-date's rate (`convertAt`), gain tone.
  - Without a rate, the native gross stands in and the sub-line says `wealth.noRate` (as
    `TradeRow` does).
  - Superseded rows are dimmed (opacity 60 %) with the value struck through.
- Value sub-line: the native gross with 2 decimals; for Yahoo, "Yahoo Finance"
  (`detail.dividendRecords.sourceYahoo`) instead.
- Menu (tap / long press), for manual dividends only:
  - Upravit (not when superseded);
  - Smazat dividendu, which opens a `ConfirmSheet` with
    `detail.dividendRecords.confirmDelete.*` (date, gross in its currency).
- A Yahoo row has no menu and no tap, as on the desktop, where it has no actions.

**Zapsat dividendu.**
- `PositionMenu` gets a first item "Zapsat dividendu" (`detail.dividendRecords.add`, `Coins`),
  followed by a separator, as on the desktop. It calls the page's `onAddDividend`.
- The page owns `DividendSheet` (new / edit). The sheet:
  - title `addTitle` / `editTitle`, description `description` with the ticker;
  - **Ex-date** (`DateField`, hint `exDateHint`, default today);
  - **Hrubá částka** (`AmountField`, hint `grossHint`);
  - **Poznámka** (textarea, ≤ 500 characters, `validation.dividendNoteTooLong`);
  - footer Zrušit · Zapsat dividendu / Uložit dividendu.
- When the list says `fromYahoo`, an `InlineAlert` shows `yahooTitle`/`yahooBody`, the fields are
  disabled and save is disabled (desktop D16).
- Writes go through `useStockDividendMutations` (create/update/delete, rule 7 already).
- `exDate` = `isoToUtcDay`. Empty note → `null`.
- **(call) Currency:** a new dividend is in `holding.dividendCurrency ?? holding.originalCurrency`,
  fixed. An edited one keeps its own. Same reasoning as A1. The code is passed as is (a `GBX`
  unit means pence; the backend stores it in GBP), as the desktop modal does.
- The existing "Odhad dividendy" (the yearly estimate) stays where it is, below the separator.
- The ledger filter stays as chosen after a write. "Dividendy" with no rows shows the ledger's
  empty line (`detail.transactions.empty`).

## Constraints

- AGENTS rules 1, 7 and 8. Every new user-facing string goes in all ten locales. Reuse the
  desktop's keys wherever a desktop string exists.
- Phone fields per README §13: 48 pt and ≥ 16 px text. Sheets use the existing `Sheet` (heights
  are now capped below the Dynamic Island).
- Pure logic goes in `src/mobile/lib` with Vitest tests. Components and hooks are not unit-tested
  (testing policy).
- No backend or schema change.

## Left to the builder

The file split inside `src/mobile`, the exact names of the helpers, and the class names. Follow
`CostSheet`/`ExtraPaymentSheet` for the forms and `TradeRow` for the rows.
