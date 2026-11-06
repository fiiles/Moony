# Spending plans on the phone — implementation plan

Spec: `docs/specs/2026-11-06-mobile-spending-plans-design.md` (decisions M1–M16). Branch
`feat/mobile-spending-plans` in the worktree `../Moony-tauri-mobile-plans`. Frontend only: no
Rust, no migration, no `shared/schema.ts` change.

**Every task:** read `AGENTS.md`, the spec, `docs/design-system/README.md` §13 (Mobile, read the
parts on the components you touch) and the neighbouring phone files named in the task before you
write. Copy the phone's idioms: `Screen`/`NavBar`, `EntityRow`, `Sheet`, `text-m-*` type roles,
token colors only (`no-colour-literals.test.ts`, `type-roles.test.ts`). Every new user-facing
string goes into all ten locales (`en cs de es it nb pl sv ja zh`). The Czech wording is the
source; the other languages are translated. Reuse `spending_plans` keys when the desktop already
has the sentence. New phone keys go under `plans.*` in `mobile.json`. Commit at the end of the
task with a Conventional Commit message ending in
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, after `npm run lint`,
`npm run typecheck`, `npm run format:check` and `npm test` pass. Check
`git branch --show-current` = `feat/mobile-spending-plans` before committing. Do not start a dev
server or simulator: the controller verifies on the simulator.

## Task 1 — Routes and pure helpers (TDD)

Files: `src/mobile/shell/routes.ts` (+ `routes.test.ts`), new `src/mobile/lib/plan-rows.ts` +
`plan-rows.test.ts`.

- Routes: `plans: '/cashflow/plans'` and `planDetail: '/cashflow/plans/:id'`, plus
  `plansPath(year?: number)` (`/cashflow/plans?year=N`, bare when no year) and
  `planPath(id: string)`. Both belong to the Cashflow tab (`tabForPath`).
- `plan-rows.ts`. Build on `src/utils/spending-plans.ts` and never duplicate it.
  - `plansCardSummary(overview: SpendingPlansOverview): PlansCardSummary | null`. Returns null when
    `overview.plans` is empty. Otherwise `{ year, currency, planned, spent, left, over, near }`:
    planned/spent/left are numbers or null (from `totals`, with `left` = `remainingActive`);
    over/near count **active** plans by `planRow(...).status`. Spec M17 applies: `planned`/`spent`
    are the whole year (closed plans included), while `left` covers active plans only. The UI labels
    them as two separate facts.
  - `planListSections(rows: PlanRow[], query: string, locale: string): { active: PlanRow[]; closed: PlanRow[] }`
    uses `overviewRows(rows, 'active' | 'closed', query, locale)`.
  - `SEARCH_FROM = 8`, the plan count at which list and picker show a search field (shown when
    count > 8 for the list and ≥ 8 offered for the picker, as the spec says).
  - `pickerGroups(index: SpendingPlanSummary[], bookingYear: number, query: string, locale: string): { year: number; rows: PlanRow[] }[]`.
    Open plans only, each plan as `planRow`. The booking year's group comes first, then the other
    years newest first. Rows within a group are sorted by name in `locale`. The name search goes
    through `matchesPlanSearch`. Empty groups are dropped.
  - `filterPlanPayments(txs: BankTransaction[], month: string | null, category: string | null): BankTransaction[]`.
    `month` is "YYYY-MM" compared with `monthOfDay(tx.bookingDate)`. `category` is a
    `categoryKey` (so `UNCATEGORIZED_KEY` matches `categoryId === null`).
  - `takeOverInserts(source: SpendingPlan[], targetNames: string[], toYear: number, locale: string): InsertSpendingPlan[]`.
    Uses `takeOverRows` and keeps the `selected` rows; each insert copies name, amount, currency,
    icon, color and note, with `year = toYear`.
  - `yearChoices(year: number): number[]` returns `[year - 1, year, year + 1]`.
- Tests cover each helper, including null spend, closed plans, cross-year picker order,
  uncategorized filter, take-over skipping closed and same-named plans (case/whitespace-insensitive
  via `samePlanName`).

## Task 2 — Plan form sheet

File: new `src/mobile/sheets/PlanFormSheet.tsx`, plus locales.

- Props: `{ open, onOpenChange, plan?: SpendingPlan | null, defaultYear: number, onSaved?: (plan: SpendingPlan) => void }`.
  Create mode applies when `plan` is null.
- A full `Sheet` with the footer "Zrušit" + primary ("Vytvořit plán" / "Uložit změny"). Fields, in
  order:
  - Název.
  - Plánovaná částka: `AmountField`, unit = the currency.
  - Měna: a row that opens `CurrencyPickerSheet`.
  - Rok: `Segment` of `yearChoices(defaultYear)`; on edit the choices are built around the plan's
    year.
  - Ikona and Barva: reuse the desktop's `IconColorPicker` if it fits 360 pt. Otherwise build a
    phone grid on the same icon list and palette exported by
    `src/components/common/IconColorPicker.tsx` / `CategoryIcon`.
  - Poznámka: optional, 500 chars.
  - The FX hint the desktop shows ("Platby v jiné měně se do plánu převedou kurzem ECB ke dni
    platby").
- Validation and defaults copy `src/components/spending-plans/PlanFormDialog.tsx`: zod keys
  `validation.*`, `NAME_MAX` 100, positive amount, icon `wallet`, `DEFAULT_COLOR`, main currency.
  A taken name (`validation.spendingPlanNameTaken`) shows under Název.
- Writes go through `useSpendingPlanMutations().createPlan / updatePlan`. Create toasts the way the
  desktop does. After the write the sheet calls `onSaved(plan)` and closes.
- Look at `BudgetLimitSheet.tsx`, `OtherAssetAddSheet.tsx` and `CostSheet.tsx` for the phone's form
  idioms (field labels, `AmountField`, keyboard, sheet footer).

## Task 3 — Plans list page and the Cashflow entry

Files: new `src/mobile/pages/Plans.tsx`, `src/mobile/components/plans/` (row, card, year bar as
needed), `MobileShell.tsx` (route), `Cashflow.tsx`, `CashflowMenu.tsx`, plus locales.

- **Cashflow root (M2, M3):** call `useSpendingPlansOverview(currentYear)`. When
  `plansCardSummary` is not null, render `PlansCard` after the bars card and after the budget note
  when shown. It must look unlike the budget note: a card (`.m-card` look, like the bars card)
  with a small square plan glyph (lucide `ClipboardList` in a rounded square). Content: title
  "Výdajové plány 2026", the line "Zbývá X z Y", a `ProgressBar` of spent/planned (tone `loss`
  when spent > planned), and the status line "2 nad plánem · 1 blízko plánu" (omitted when both
  counts are 0). Missing totals (null) show "—" with no bar. Wording per spec M17: the value is
  "Zbývá v aktivních plánech X" (`spending_plans` `stats.remaining`), and the bar and the line
  "Utraceno S z P na rok N" are the whole year. The same wording applies to the list hero. The whole card links to
  `plansPath(year)`. `CashflowMenu` gets an item "Výdajové plány" (lucide `ClipboardList`) after
  Spravovat rozpočty, linking to `plansPath()`.
- **Plans page (M4–M7):**
  - `Screen` with back `NavTextButton` "Cashflow" (`useBack(routes.cashflow)`, as in
    `Budgets.tsx`) and trailing `+` that opens `PlanFormSheet` (`defaultYear` = the page's year;
    on create, push the new plan's detail).
  - Year from `?year=` (desktop `yearFromSearch` rules), range = the desktop's (min of first year
    with plans and N−1 .. current+1). A ‹ N › bar in the `PeriodBar` look; steps replace the URL.
  - Hero: `CashflowHero`-like block with "Zbývá" = `totals.remainingActive` in `overview.currency`
    and the note "Utraceno X z Y". `ScaledReportNote` for `fxQuality`.
  - Sections: active plans (head "Aktivní plány", note = count), then "Uzavřené". A `SearchField`
    appears above them when the year has > 8 plans.
  - Row `PlanRowItem` per M5: `EntityRow` with `PlanChip` as chip, a progress bar under the row
    like `CategoryRow`'s bar. The long press opens a `RowMenu` with Upravit (form sheet), Uzavřít
    or Obnovit plán (`setClosed`), Smazat plán (`ConfirmSheet` with `spending_plans`
    `delete.title` / `delete.withPayments` / `delete.noPayments`).
  - Empty year: `ScaledEmptyState` with the desktop's `empty.*` sentences. Actions: take-over
    (when N−1 has plans; a `ConfirmSheet` "Převzít n plánů z roku N−1?" that runs
    `createPlans.mutate(takeOverInserts(...))` and toasts like the desktop) and "Nový plán".
    Count the take-over from `useSpendingPlansOverview(year - 1, enabled)` as the desktop does. If
    every N−1 plan is closed or already exists, offer only "Nový plán".
  - Loading: skeleton rows. Error: `ScaledReportError`.
- Register `routes.plans` and `routes.planDetail` in `MobileShell.tsx`. The detail page arrives in
  Task 4; until then, route it to a placeholder that Task 4 replaces.

## Task 4 — Plan detail page

File: new `src/mobile/pages/PlanDetail.tsx` (+ components under `src/mobile/components/plans/`),
plus locales.

- `useRoute(routes.planDetail)`, `useSpendingPlanDetail(id)`, `useSpendingPlanTransactions(id)`.
- Nav: back "Plány N" → `plansPath(plan.year)`; title = plan name. Trailing `···` (`NavIconButton`
  + `DropdownMenu` + `RowMenuItem`, as in `CashflowMenu`) with Upravit, Uzavřít/Obnovit plán and
  Smazat plán. Smazat goes through `ConfirmSheet` → `useDeleteAndLeave` with home =
  `plansPath(year)`, keys `['spending-plan', id]` and `['spending-plan-transactions', id]`, and
  remove = `spendingPlansApi.remove` (keep the desktop's toast `toast.deleted`). Read
  `use-delete-and-leave.ts` and one caller (e.g. `OtherAssetDetail.tsx`) first.
- Hero (M8): left / "Přečerpáno o" (abs) in plan currency, tone `loss` when over. Note
  "Utraceno X z Y · 68 %". `ProgressBar`. Closed plan: `InlineAlert` "Plán je uzavřený" with an
  Obnovit button. A null spend shows "—" plus `ScaledReportNote` for `fxQuality`.
- **Útrata po měsících:** bars of `planMonthSpend(detail, axis, category)` over
  `planMonthAxis(plan.year, months)`, in a card. Build a small single-series component in the
  `CashflowBars` look (`s3` bars, 3 px rounded tops, zero baseline, a short month name under every
  third bar, one transparent tap button per month). Do not stretch `CashflowBars` itself. A tap
  selects or clears the month filter. The selected month is full opacity and the others are muted.
- **Kategorie:** `planCategoryRows(detail, month)`. Each row: round `CategoryChip`, name (via
  `useCategoryName`; "Bez kategorie" for the uncategorized key), "N plateb", amount, a share bar.
  A tap selects or clears the category filter; the selected row is marked (check or tint).
- **Platby:** `filterPlanPayments(all, month, category)` grouped by day like the account ledger
  (look at `AccountDetail.tsx` / `CashflowCategory.tsx` for `DayHead` + `LedgerRow`). A tap opens
  `TransactionSheet` (with category picker wiring as `CashflowCategory.tsx` does). When the plan
  has no payments, an empty line: "Platby přidáte v detailu platby (Výdajový plán) nebo na
  počítači."
- Filter chips (M9): one small removable chip per active filter, above the section it narrows
  ("Březen ×" above Kategorie and Platby, "Ubytování ×" above the bars and Platby). Use the
  `FilterSheet`/`RowBadge` styling of the phone if one fits; otherwise build a small chip on the
  tokens.
- Edit opens `PlanFormSheet` with the plan.

## Task 5 — Tag a payment from the transaction sheet

Files: `src/mobile/sheets/TransactionSheet.tsx`, new `src/mobile/sheets/PlanPickerSheet.tsx`,
the three `TransactionSheet` callers (`AccountDetail.tsx`, `CashflowCategory.tsx`,
`CategorizeQueue.tsx`, and the new `PlanDetail.tsx`), plus locales.

- `TransactionSheet` gets a row **Výdajový plán** under Kategorie (M11). The value is
  `PlanChip size="sm"` + name, or "Bez plánu". The plan's name comes from `useSpendingPlanIndex()`
  by `tx.spendingPlanId`. A tap opens `PlanPickerSheet` over the transaction sheet. Do it the way
  the category picker is stacked; check how `onPickCategory` is handled by the callers and whether
  the picker can live inside `TransactionSheet` itself, which needs no caller change. Prefer
  inside.
- `PlanPickerSheet` (M12): props `{ open, onOpenChange, tx: Pick<BankTransaction,'id'|'spendingPlanId'|'bookingDate'> | null, onAssigned?: (planId: string | null) => void }`.
  It uses `pickerGroups(index, year of bookingDate, query, locale)`. Rows: `PlanChip`, name, sub
  "zbývá X z Y" / "přečerpáno o X" (plan currency), and a check on the current plan. "Bez plánu"
  goes first when the payment has a plan. "Nový výdajový plán…" goes last and opens
  `PlanFormSheet` (`defaultYear` = the booking year); `onSaved` assigns the payment to the new
  plan. The search field shows when ≥ 8 plans are offered. A choice calls
  `useAssignWithUndo().assign([tx], plan)`, closes, and reports
  `onAssigned(planId)` so the transaction sheet's caller can patch its row
  (`onChange({ spendingPlanId })`). Look at `CategoryPickerSheet.tsx` for list/search/check
  idioms.
- In `PlanDetail`, untagging a payment removes it from the list after the refetch. Nothing extra
  is needed beyond the invalidation `useAssignWithUndo` already does.

## Task 6 — Docs

- `docs/design-system/README.md` §13: a short subsection "Výdajové plány" naming the Cashflow
  card, the list row, the detail and the two sheets, with the square-vs-round chip rule.
- `CHANGELOG.md` `[Unreleased]` → `### Added`: one entry "Spending plans on the phone" in the
  file's style.
- `docs/architecture/overview.md` or the phone docs only if they list phone screens (check; do
  not invent a section).

## Order

1 → 2 → 3 → 4 → 5 → 6 (one implementer at a time: tasks share `mobile.json` in ten locales and
`MobileShell.tsx`). Each task gets a spec-compliance review; fix rounds as needed. The controller
then verifies on the simulator, a final whole-branch review (opus) runs, its fixes go in, and the
branch is merged into local `main`.
