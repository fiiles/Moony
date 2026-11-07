# Mobile finance and settings parity (audit A6–A8, A12, B3, B4, B6, C9, C11, E)

Date: 2026-11-07. Source: `docs/audits/2026-11-06-mobile-ux-audit.md`. Reference: the desktop
("desktopová verze je referenční bod"). The owner delegated every decision; the ones taken here are
marked **(call)** with what each costs if wrong.

## Why

The phone shell leaves a set of desktop features out without a §7 reason (export, country, integrity,
attachment status, deleting a balance record, auto-categorization, interest, the getting-started card,
loan reminders), shows rows that cannot be put away (Údržba's statement and uncategorized rows), counts
"Nezařazené" without a way back from "Nechat", carries desktop copy that does not fit a phone
(Připomínky), and has a list of small inconsistencies and code leftovers.

## Goals

1. **A6 Nastavení › Záloha a přenos**: "Exportovat všechna data (JSON)" through the Files picker, the
   database integrity check, and the encrypted-attachments status (desktop `DataSection`,
   `AttachmentsStatusRow`). **Profil** gets the country (desktop `ProfileCard`).
2. **A7 Historie zůstatku**: delete a record by swipe left or the long-press menu, confirmed in a
   `ConfirmSheet` with the desktop's `balanceHistory.delete*` copy.
3. **A8 account `···`**: "Automatická kategorizace" (`useAutoCategorize`, the desktop's
   `handleAutoCategorize`) and "Úročení" (the desktop's `InterestDialog`, shown under the desktop's
   rule: zones, or a rate above 0).
4. **A12 Začínáme**: the desktop's checklist (`gettingStartedItems`) as a phone card on Přehled (empty
   or not, dismissible, hidden when done); the empty Přehled's button opens the add-account dialog on
   Finance directly (`/finance?add=account`).
5. **B3** `loan_balance_check` and `loan_fixation_expired` become Údržba rows with the desktop's title
   and sub-line, the milestone menu, and a verb that opens the loan with its event dialog
   (`/finance/loans/:id?event=balance_check|rate_change`).
6. **B4** the phone-only rows (`statement_due`, `uncategorized`) get a `···` with Skrýt and Připomenout
   za měsíc, kept on the device; an account whose newest balance was recorded by hand after its last
   import is kept manually and gets no statement row.
7. **B6** one set of "Nezařazené" for every count (the queue's), "Nechat" gets a "Vrátit" toast and the
   queue page a line "N odloženo · Vrátit do fronty" (new backend `unsnooze_categorization`).
8. **C9** the Připomínky lead in phone words that also names the notifications.
9. **C11** recent moves open their record; "Nejbližší platby" include loan payments; the jump marks on
   Přehled's chart get the desktop legend; one transaction-day formatter; Toky keeps its place while
   loading; Údržba starts unfolded; TransactionSheet's footer is Zavřít + Hotovo with the delete as a
   text action in the body; the position detail gets "Aktualizovat ceny" in `···`.
10. **E** Finance `···` › Export writes the desktop's loans and policies CSV; Rozpočty lose the
    "ukončen" label and list the earlier limit intervals; the stale comments go; `STALE_BALANCE_DAYS`
    is pinned to the Rust constant by a test.

## Non-goals

- Error states, lock return, tab memory, dirty sheets, notification targets, Android backup (the
  navigation/resilience branch). Charts and wealth beyond the one `···` item (the wealth branch).
- Pull-to-refresh: it needs a gesture in the shared `Screen` (shell area); the price refresh is in the
  menu instead. Listed as not done.
- A bank-transactions CSV: the desktop has none either.

## Design

### A6

- Backend: `stage_data_export` (mobile only) writes `export_all_data` to `Documents/moony-export.json`,
  keeps it out of device backups and returns the `ExportSummary`; the name joins
  `TEXT_EXPORT_FILE_NAMES`, so `discard_text_export` and the interrupted-export sweep remove it.
  Frontend: stage → `save({ defaultPath })` → discard, the desktop's toasts (`data.exportSaved` with the
  table and row counts; no path). iOS only, as the other exports (`exportsOnThisDevice`).
- `verify_database` and `get_attachment_status` exist on both shells; the status lines move into a pure
  `attachmentStatusLines()` (`src/utils/attachment-status.ts`) used by the desktop row and the phone.
  The phone leaves out the leftovers' "Zobrazit" (it reveals a path in Finder).
- Profil: a row "Země" opening the first run's `CountryPickerSheet`; saving sends only `country`, as the
  desktop does (it refreshes the reminders that depend on it).

### B3–B6 (Údržba)

`buildUpkeepRows` adds one row per `loan_balance_check` / `loan_fixation_expired` milestone (after the
valuations, the desktop's upkeep order) and takes a `dismissals` map (device storage,
`upkeep-dismissals.ts`): `statement_due:<account>:<lastImport>` and `uncategorized` keys, each `{ until:
day | null, count? }`. Skrýt on a statement row lasts until the next import (the key carries the
import), on the uncategorized row until more payments wait than when it was hidden; Připomenout za
měsíc lasts 30 days. A toast with Vrátit undoes either.

`unsnooze_categorization({ transactionIds?, accountId? })` clears `categorize_snoozed_until` for the
given ids, or for every uncategorized payment of one account or all accounts.

## Decisions

- **(call)** The JSON export is staged under the fixed name `moony-export.json`, like the phone's CSVs,
  not the desktop's dated name. Cost if wrong: the person renames the file in Files.
- **(call)** Integrity check and attachment status live on Záloha a přenos (the desktop's "Data a
  zálohy" card holds them with the backup), not a new page. Cost: one more scroll on that page.
- **(call)** B4: phone-local dismissals (device storage, not the backend's `milestone_states`): the
  rows are the phone's own and the desktop never shows them, so a state on the computer would mean
  nothing there. Cost: a second phone would show the row again.
- **(call)** B4: a manual balance newer than the last import means "kept by hand" and suppresses the
  statement row. Cost: someone who records a balance between statements loses the reminder until the
  next import is 35 days old again — acceptable, the monthly notification still asks.
- **(call)** B6: the ledger's "Nezařazené" filter stays "every payment without a category" (it is the
  desktop's filter and also shows pending and put-off rows, with their badges); every *count* uses the
  queue's set, and the queue page names how many are put off with a way to bring them back. Cost: the
  filter can list more rows than the count says; the queue line explains the difference.
- **(call)** Údržba starts unfolded: every row has a verb, and the folded line made each one a tap
  further. The fold stays and is remembered for the session. Cost: a long card pushes the hero down.
- **(call)** One transaction-day text: Dnes / Včera, else "10. 10." with the year when it is not this
  year (`shortDayOptions`), in the ledger's day heads, the queue, the Cashflow category and Přehled.
  Cost: the ledger's heads lose the short month name.
- **(call)** Rozpočty: earlier intervals are listed under their budget as quiet rows ("Dřívější limit")
  with edit and delete-interval in their menu, always shown (the desktop folds them behind a chevron;
  a phone list has room and no hover). Ended budgets stay dimmed and last, as on the desktop, without
  a label. Cost: a budget with many past limits makes a long list.
- **(call)** Finance › Export offers "Úvěry (CSV)" and "Pojistky (CSV)" (the desktop list pages'
  exports); accounts have no CSV on the desktop either.
- **(call)** "Nejbližší platby" stay under the policies and now include each running loan's next
  payments from its plan (`amortizationSchedule(row.plan)`), in the loan's currency. Cost: the section
  sits under Pojištění although it names loans too.
