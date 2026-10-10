# Bank account detail: balance records, sections by data, last month with movements

Approved by the owner on 2026-10-10 after a design conversation, five prototype rounds and
an independent UX review. The owner's final direction (round 5) replaced the account-type
layouts of round 4 with sections that appear only when their data exist. Visual reference:
`docs/specs/2026-10-10-bank-account-detail-prototype.html` (scenarios A–F, design notes
under "Poznámky k návrhu"). The prototype is a snapshot like this spec; two final owner
decisions came after it and are not in it: both head buttons are dark primary buttons, and
the flows-chart arrows shift the whole 12-month window instead of stepping the selection.

## Problem

- **The balance is not historized.** One number plus a date (`balance`, `balance_as_of`)
  is all the account stores. The detail chart walks back from it through transactions
  (`reconstructBalance`). A savings account the user maintains by hand, without
  importing transactions, gets a flat line; the balance history that would actually be
  interesting is lost on every update.
- **Categorize-only checking accounts get a nonsense chart.** Many users import
  transactions only to categorize them and never maintain the balance. The stored
  balance is 0, yet the reconstructed line wanders around zero as if it meant something.
- **"This month" is usually empty.** Stats and the ledger default to the current calendar
  month, but people import statements after a month ends, often a quarter or two months
  at once. On open, the page shows nothing, and the user has to change the preset first.

## Decisions

1. **Balance history from explicit updates.** A new append-only table of dated balance
   records. A record is created when the user records a balance by hand or confirms the
   statement balance after a CSV import. Nothing is recorded automatically.
2. **Sections appear by data, nothing is inferred and nothing depends on the account
   type.** The balance section is shown only when the account has a recorded, nonzero
   balance. The flows section is shown only when the account has a booked transaction in
   the last 12 calendar months. The ledger is shown when the account has any transaction.
   An account can show one, two or all three; a brand-new account shows one empty state.
3. **The flows chart is a 12-month window that two arrows shift by one month.** It ends
   with the current month by default and can never go past it; going back reaches months
   older than the last twelve. The month whose net the card shows is the newest month with
   movements inside the window; clicking a bar selects another month of the window.
4. **The ledger keeps today's period filtering unchanged** (presets, "Celé období",
   "Vlastní" from–to), independent of the flows chart. Only its initial value changes:
   the month of the newest transaction instead of the current month.
5. **No flows statistics.** The income / expense / net cards are deleted. The balance
   section keeps its three cards (change this year, average per month, yearly interest).
6. **No reconstruction and no event rings on bank charts.** The line through records is
   the whole balance story; the gap between the last record and today shows staleness.
7. **Both actions always in the head, both as dark primary buttons.** "Zaznamenat
   zůstatek" and "Importovat výpis" are visible on every account in the same style; the
   "···" menu holds the rest and never changes. This page deliberately has two primary
   buttons (owner decision, noted in the design system).
8. **Balance history, import history and interest tiers are modals** opened from the
   "···" menu, not cards on the page.
9. **The balance field leaves the edit form.** The only way to change a balance is to
   record one. The create form keeps an optional opening balance with a date.

## Data model

### Table `bank_balance_records` (migration `013_balance_records`)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | UUID |
| `bank_account_id` | TEXT NOT NULL | `REFERENCES bank_accounts(id) ON DELETE CASCADE` |
| `amount` | TEXT NOT NULL | Balance in the account currency, canonical decimal string |
| `currency` | TEXT NOT NULL | Always the account currency; stored so a later currency change on the account cannot silently relabel history |
| `as_of` | INTEGER NOT NULL | UTC day start (unix seconds), never in the future |
| `source` | TEXT NOT NULL | `manual` \| `statement` |
| `source_batch_id` | TEXT | `REFERENCES csv_import_batches(id) ON DELETE SET NULL`; the record survives a batch delete |
| `recorded_at` | INTEGER NOT NULL | `unixepoch()` |

`UNIQUE (bank_account_id, as_of)`. One day has one true balance; recording the same day
again replaces the earlier record (same `id` kept, `amount`, `source`, `source_batch_id`,
`recorded_at` updated). Index on `(bank_account_id, as_of DESC)`.

**Backfill in the same migration.** Every account with `balance_as_of IS NOT NULL` gets one
record: `amount = balance`, `currency = currency`, `as_of = balance_as_of`,
`source = 'manual'`, `recorded_at = updated_at`. An undated balance (`balance_as_of IS
NULL`) is not backfilled: it stays an undated number on the account until the user records a
dated balance, exactly as today.

**Invariant.** `bank_accounts.balance` and `balance_as_of` mirror the newest record by
`as_of` whenever the account has at least one record. Net worth, the account list, portfolio
snapshots, the MCP server and the stale-balance milestone keep reading the columns and do not
change. When the last record is deleted the columns become `'0'` / `NULL`.

### Services (`src-tauri/src/services/bank_accounts.rs`)

- `record_balance(conn, account_id, &InsertBalanceRecord) -> Result<BankAccount>`: validates
  (finite amount, currency equals the account currency, `as_of` a UTC day start not in the
  future, `source_batch_id` belongs to the account), upserts the record for that day inside
  a transaction, then refreshes the two columns from the newest record. Replaces
  `set_balance_anchor`. Two safeguards of the old anchor path change:
  - *Older than the current balance* is no longer an error. An older day is history; the
    columns only change when the new record is the newest.
  - *Unknown current date* keeps its confirmation, but only while the account has no
    dated record at all: `confirm_unknown_as_of` must be true to replace an undated
    nonzero balance with the first dated record. Once a dated record exists the flag is
    ignored.
- `list_balance_records(conn, account_id) -> Vec<BankBalanceRecord>`, newest first.
- `delete_balance_record(conn, record_id) -> Result<BankAccount>`: deletes and refreshes
  the columns. No edit operation; a wrong record is deleted and recorded again.
- `create_account`: when `balance_as_of` is given, the opening balance is written through
  `record_balance` (source `manual`) so the first record exists. Without a date the
  behaviour is unchanged (undated balance, no record).
- `update_account` **ignores** `balance` and `balance_as_of` and no longer resets
  `balance_as_of` when the balance "changes". The frontend edit form stops sending them.
  The MCP `bank_account_create` tool keeps its optional `balance` and gains an optional
  `balanceAsOf`; without it the balance is stored undated as today.
- CSV import "set the statement balance as the new balance" (`NextSteps.tsx`,
  `useBalanceAnchor`) calls `record_balance` with `source = 'statement'` and the batch id.
- Stale-balance milestone (`services/milestones/accounts.rs`): the staleness day of an
  account is `balance_as_of` when present, else `updated_at` as today.

### Wire contract (`shared/schema.ts`, 7-step procedure)

- New `BankBalanceRecord { id, bankAccountId, amount, currency, asOf, source:
  'manual' | 'statement', sourceBatchId, recordedAt }` and `BalanceRecordSource`.
- `InsertBalanceAnchor` renamed to `InsertBalanceRecord { amount, currency, asOf,
  confirmUnknownAsOf?, sourceBatchId? }`; `source` is derived server-side (`statement` when a
  batch id is given, else `manual`).
- Commands: `get_balance_records(account_id)`, `record_bank_account_balance(account_id,
  data)` (replaces `set_bank_account_balance_anchor`), `delete_balance_record(record_id)`.
  `tauri-api.ts`: `bankAccountsApi.getBalanceRecords`, `recordBalance`, `deleteBalanceRecord`.
- `InsertBankAccount` keeps `balance` and `balanceAsOf` (create only; the doc comment says
  update ignores them).
- Register the new Rust types in `bindings.rs`; regenerate `generated-types.ts`.
- `docs/DEPRECATED.md`: the "Retired statement balance path" entry points to
  `recordBalance`; `setBalanceAnchor` and `reconstructBalance` on the detail page join the
  table.

## Page design (`src/pages/BankAccountDetail.tsx`)

Design system detail archetype: back link, eyebrow, H1, action row, hero, stats, ledger,
modals. The aside column is not used on this page (the account-level views are modals).

### Head (every account, every state)

- Eyebrow unchanged (bank · currency · account type). Description line: the account
  number only. The interest note and "N transakcí dnes" leave the description.
- Actions, always the same: **Zaznamenat zůstatek** and **Importovat výpis** (the CSV
  import dialog; "Import CSV" is renamed everywhere on this page), both as dark primary
  buttons of equal weight, then "···".
- "···" menu, identical for every account: Upravit účet · Přidat transakci · Historie
  zůstatku · Historie importů · Úročení (only when the account has an interest rate or
  zones) · Pravidla kategorizace · ─ · Smazat účet.
- Top-bar status: "Poslední import {date}" when the account has imports, else "Poslední
  pohyb {date}" when it has transactions, else "Zůstatek k {date}" when it has a dated
  balance, else "Zatím bez dat".

### Section 1: balance (when the newest balance record exists and is not 0, or the account
has an undated nonzero balance)

- Hero card. Label "Zůstatek k {date}" (the newest record's `as_of`); value in the account
  currency through `NativeBalance` (secondary ≈ main currency as today); delta = change
  against the previous record with arrow and sign, quiet note "od {previous date}". One
  record: quiet note "první záznam". Undated legacy balance: label "Zůstatek · datum
  neznámé", no delta, no chart.
- No aside in the hero head (no horizon control).
- Chart (180 px) only with at least two records: x from `max(first record, today − 365
  days)` to **today**, time axis ends with "Dnes"; straight segments between records; when
  a record lies before the window start, the series starts with the interpolated value at
  the window start; the line stops at the newest record so the gap to today is visible;
  the newest point is the white ring of §8; every record is a small filled dot
  (`chart-line`, white halo) with the tooltip "{amount} · {date} · ručně | z výpisu".
  Legend: Zůstatek · Záznam zůstatku. No threshold note, no "úplnost výpisů" note.
- Stats under the hero (3, `Stats columns={3}`):

| Stat | Value | Note |
|---|---|---|
| Změna letos | newest record − value at 1 January (step semantics: the latest record on or before that day); "—" when none | "proti záznamu k {date}" / "bez záznamu před začátkem roku" |
| Průměrně za měsíc | (newest − base) / months, base = latest record on or before 365 days before the newest record, else the oldest record; months = max(1, round(days / 30.44)) | "za posledních 12 měsíců" / "za {n} měsíců od prvního záznamu" |
| Roční úrok | yearly interest from zones or the simple rate on the newest balance; "—" without a rate | "{effective rate} efektivně · pásmové úročení" / "{rate} p. a." / "bez úročení" |

### Section 2: flows (when the account has a booked transaction in the last 12 calendar
months, the current month included)

- One card. Left: value block "Bilance · {selected month}" with the signed net amount
  (credits − debits of the booked rows of that month, account currency, tone gain/loss)
  and the quiet line "+ {credits} příchozí · − {debits} odchozí · {n} pohybů". When this
  card is the first card on the page it is the hero (42 px value, decorative orb); when
  the balance section sits above it, the value is compact (22 px) so the page keeps one
  dominant number. Right: the two arrows ‹ › only. The month name lives in the label.
- Chart: a window of 12 calendar months ending with `windowEnd` (default: the current
  month); bar pairs credits `s1` / debits `s3`, zero baseline; the selected month full
  strength, the others muted (opacity .35); a month without a single booked row carries
  the caption "bez importu"; clicking a month selects it; hover tooltip month · příchozí ·
  odchozí · bilance. Legend Příchozí · Odchozí. Rows in another currency are converted at
  the day rate as today; the existing readiness notes for converted and missing currencies
  stay under the card.
- Arrows shift the window, not the selection: ‹ sets `windowEnd` one month back, › one
  month forward. › is disabled when `windowEnd` is the current month; ‹ is disabled when
  the window already starts at or before the month of the oldest booked transaction.
- Selected month: the newest month with a booked row inside the window, recomputed after
  every shift; a bar click overrides it until the next shift. The initial window is the
  default one, so the card opens on the newest month with movements. The selection is
  independent of the ledger filters.
- A selected month without rows shows the value "—" and the quiet line "bez pohybů".
- No stats under this section.

### Section 3: ledger (`Transakce` card, when the account has any transaction)

- Head: title, sub ("Seskupené podle dne · nejnovější nahoře"), a small outline "Přidat
  transakci" button, the kind segment Vše · Příchozí · Odchozí, the category select and
  the search field. The second row keeps today's period select (Tento měsíc · Minulý
  měsíc · Toto čtvrtletí · Minulé čtvrtletí · Tento rok · Minulý rok · Celé období ·
  Vlastní with the two date inputs) and "Zařadit automaticky" with the uncategorized
  badge for the current filters. Nothing about the filtering changes.
- Initial period: the month of the newest booked transaction, expressed as the matching
  preset ("Tento měsíc" or "Minulý měsíc") or otherwise as "Vlastní" with that month's
  UTC range. A `?category=` deep link opens "Celé období" with that category, and
  `handleImported` selects the imported range, both as today.
- Table, grouping, sorting, paging, row detail dialog, delete: unchanged.

### Empty state (no section applies)

A card: icon wallet, "Účet zatím nemá žádná data", "Zaznamenejte zůstatek z výpisu nebo
naimportujte výpis z banky." No buttons; the head already carries both actions.

### Section combinations

| Account | Shown |
|---|---|
| Nonzero balance, no transactions (savings by hand) | balance section only |
| Transactions in the last 12 months, balance 0 or unrecorded (categorize-only checking) | flows card as hero, ledger |
| Nonzero balance and recent transactions | balance section, flows card (compact), ledger |
| Transactions only, all older than 12 months | ledger only |
| Nonzero balance and old transactions | balance section, ledger |
| Nothing | empty state |

### Modals

- **Historie zůstatku** (560 px): sub "{n} záznamů · poslední {date}"; table Datum ·
  Částka · Změna (vs the previous record, signed, gain/loss) · Zdroj (outline badge
  "ručně" / "z výpisu") · row "···" with "Smazat záznam" (confirm dialog, toast "Záznam
  smazán"); body scrolls above ~420 px with a sticky head. Empty: "Zatím žádný záznam
  zůstatku." Footer: ghost "Zavřít" only.
- **Historie importů**: the existing import-history table (date, file, imported,
  duplicates, errors, "Smazat importované transakce…") moves from the inline toggle card
  into a wide modal like the CSV import wizard. Footer: "Zavřít".
- **Úročení** (440 px): sub "Pásmové úročení podle výše zůstatku" or "Jednoduché
  úročení"; zone rows "0 – 500 000 Kč  4,10 %"; the effective rate and yearly interest in
  one line; caption "Efektivní sazba je roční úrok z aktuálního zůstatku dělený
  zůstatkem." Footer: "Zavřít". Zones are still edited in the account form.
- **Zaznamenat zůstatek** (440 px): "Částka" with the account currency as unit, empty
  with a placeholder; "K datu" date input, default today, max today; hint "Zůstatek z
  výpisu nebo z bankovnictví. Importované transakce ho nemění."; when the chosen day
  already has a record: "Záznam ze dne {date} bude nahrazen."; when the account has an
  undated nonzero balance and no record: a checkbox "Nahradit zůstatek bez data" that
  sets `confirmUnknownAsOf`. Footer ghost "Zrušit", primary "Zaznamenat zůstatek"; toast
  "Zůstatek zaznamenán". Validation errors inline, mapped from `validation.*` keys.

### Account form

- Create: "Počáteční zůstatek" (optional, empty = no record) and "K datu" (default today).
- Edit: no balance field. A read-only line "Zůstatek {amount} k {date}" with a link
  "Zaznamenat nový" that closes the form and opens the record dialog.

### Removed from today's page

Balance line reconstruction and its anchor logic, large-movement events and the threshold
note, the horizon segment (`TimePeriodSelector` on this page), the "this month / last
month" queries and the three month stats, the inline zones card and inline import-history
card, the "Odhad z dostupných zaúčtovaných pohybů…" readiness note, the interest and
today-count parts of the description. `reconstructBalance`, `significantMovements` and
`movementThreshold` are deleted when no other caller remains.

### Mutations

Record and delete a balance: invalidate `['bank-account', id]`, `['bank-accounts']`,
`['bank-balance-records', id]`, `['portfolio-metrics']`, `['cashflow-report']`, the
milestones query, then `recordSnapshot()` and invalidate `['portfolio-history']`
(rule 7, reference `use-bank-account-mutations.ts`).

## Copy (every string in `cs/` and `en/`)

New keys in `bank_accounts.json` (Czech shown; English equivalents alongside):
`hero.balanceAsOf` "Zůstatek k {{date}}", `hero.balanceUnknownDate` "Zůstatek · datum
neznámé", `hero.sincePrevious` "od {{date}}", `hero.firstRecord` "první záznam",
`flows.netIn` "Bilance · {{month}}", `flows.line` "{{credits}} příchozí · {{debits}}
odchozí · {{count}} pohybů" (plural forms), `flows.noMoves` "bez pohybů", `flows.prev`,
`flows.next`, `chart.record` "Záznam zůstatku", `chart.noImport` "bez importu",
`stats.changeThisYear`, `stats.vsRecordAt`, `stats.noRecordBeforeYear`,
`stats.avgPerMonth`, `stats.last12Months`, `stats.sinceFirstRecord` (plural),
`stats.yearlyInterest`, `stats.effectiveZoned`, `stats.simpleRate`, `stats.noInterest`,
`actions.recordBalance` "Zaznamenat zůstatek", `actions.importStatement` "Importovat
výpis", `menu.balanceHistory` "Historie zůstatku", `menu.importHistory`, `menu.interest`
"Úročení", `balanceHistory.*` (title, sub, columns, sources, empty, delete, deleted),
`interest.*` (title, zonedSub, simpleSub, effectiveLine, caption), `recordDialog.*`
(title, amount, amountPlaceholder, asOf, hint, replaces, confirmUnknown, submit,
success), `form.openingBalance`, `form.openingBalanceAsOf`, `form.currentBalance`,
`form.recordNew`, `empty.noDataTitle`, `empty.noDataDescription`, `status.noData`,
`validation.balanceCurrencyMismatch` and `validation.balanceAnchorUnknown` kept,
`validation.balanceAnchorOlder` removed. The existing `filters.*` keys of the ledger stay.

Sentence case, true minus, "pohyb/pohyby/pohybů" for movements in the hero and the flows
card, "transakce" only in the ledger card.

## Edge cases

- **Stale record** (newest record months old): the line stops, the axis runs to "Dnes",
  the label carries the date. No extra sentence; the stale-balance milestone nudges.
- **Transaction added or imported after the newest record**: the balance number does not
  change (records are explicit). The flows card shows the month's net next to it.
- **Quarter imported at once**: the flows card opens on the newest month, the two older
  months are one arrow away; the ledger's "Celé období" shows everything.
- **Record of exactly 0**: the balance section hides (a zeroed savings account is not
  tracked); the record stays in the history modal.
- **Many records** (weekly for years): the chart window is one year, so at most ~52 dots.
- **Same-day replace**: the dialog says so before the user confirms.
- **Deleting the newest record**: the columns fall back to the previous record; deleting
  the last one leaves `'0'` / `NULL` and the balance section disappears.
- **Undated legacy balance**: shown with "datum neznámé", no chart, stats "—"; the first
  dated record (manual or statement, with confirmation) replaces it.
- **Foreign currency**: amounts through `NativeBalance` and `useFormat().money` with the
  account currency, never a hard-coded unit; other-currency rows converted at their day
  as today, with the existing converted / missing notes.
- **Credit card and investment accounts**: same rules; a credit card balance is whatever
  the user records (debt is the user's sign convention, unchanged from today).
- **Pending rows**: excluded from the flows card and the default month, as today.

## Design system amendments (`docs/design-system/README.md`)

- §7 Detail row: "bank account detail: sections by data — balance hero with dated records
  and 3 balance stats, a flows card with a 12-month bar window shifted by arrows and a
  selected month, the ledger; balance history, import history and interest tiers are
  modals from the ··· menu."
- §6 Button rule "One primary per page": add the exception "the bank account detail head
  has two (Zaznamenat zůstatek, Importovat výpis), an owner decision of 2026-10-10".
- §9: bank account charts draw no movement events; dated balance records are drawn as
  dots on the line. The "400 EUR / 10 %" sentence is removed.
- §8 unchanged (axis ends "Dnes", white ring on the last point).

## Testing

- Rust (`services/bank_accounts.rs`, in-memory schema): record upsert and same-day
  replace; newest record drives the columns regardless of insertion order; older record
  leaves the columns alone; delete refreshes the columns and clears them after the last
  record; currency mismatch, future day and foreign batch rejected; undated balance needs
  `confirm_unknown_as_of` only before the first dated record; `update_account` ignores
  balance fields; `create_account` with a date writes a record. Migration test: backfill
  creates exactly one record per dated account and none for undated ones.
- TypeScript (`src/utils/`, Vitest): `balanceSeries` (window start interpolation, line
  ends at the newest record, fewer than two records → empty), `valueAt` step semantics,
  `changeThisYear`, `averageMonthlyChange` (365-day base, short history, min one month),
  `monthlyFlows` (12-month window for a given end month, empty months, foreign rows at
  day rate), `flowsWindow` (shift limits: never past the current month, never before the
  oldest transaction; newest month with rows inside the window as the selection),
  `newestMonth` (newest booked row, pending ignored) and its ledger preset mapping,
  `sectionVisibility` (the six combinations above).
- Out of scope per policy: React components and hooks, thin commands, `tauri-api.ts`.

## Out of scope

- Automatic balance snapshots, forward extrapolation after the newest record, editing a
  record in place, a balance history on the account list page, changes to net worth or
  portfolio history, a per-account "what I track" setting, any change to the ledger's
  filter controls.
