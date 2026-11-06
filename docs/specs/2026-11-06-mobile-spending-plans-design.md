# Spending plans on the phone — design

Date: 2026-11-06. Status: written for autonomous development at the owner's request ("vyviň to
autonomně"). Builds on the desktop feature (`docs/specs/2026-10-29-spending-plans-design.md`,
"desktop spec" below) and the phone app (`docs/specs/2026-10-16-mobile-app-design.md`, "phone
spec"). Visual rules: `docs/design-system/README.md` §13 (Mobile).

## 1. Why

Spending plans (*výdajové plány*) exist on the desktop only. The backend, migration 026 and all
eleven `spending_plans` commands already run on the phone. The phone shows nothing of them.

Asked what he would do with plans on the phone, the owner answered: **mostly check them**, i.e.
see how much is left in a plan and which plans are running over. Tagging payments matters less.

## 2. Goals

1. From the Cashflow tab, see at a glance how this year's plans are doing. One tap leads to the
   plan list.
2. Plan list for a year: what is left overall, then every plan ordered by risk, with what is left
   in each.
3. Plan detail: planned, spent and left; spend per month; spend per category; the payments.
4. Tag or untag one payment with a plan from the transaction sheet. A payment can be checked in a
   plan's detail and removed there if it does not belong.
5. Create, edit, close or reopen, and delete a plan. Take over last year's plans into an empty
   year.

## 3. Non-goals (this version)

- Multi-select in the account ledger ("Přidat do plánu" for several rows at once).
- The cross-account "Přidat platby do plánu" picker from a plan's detail.
- The take-over with per-plan selection and per-plan amounts. The phone copies every open plan of
  last year as it is, and amounts are edited afterwards.
- A plan column or chip in the ledger rows, the "Kam jdou peníze z plánů" flow card, and plan
  widgets on Přehled.
- Any backend, migration or wire-contract change. If a gap turns up, it is reported and not
  patched here.

These items are deferred, not rejected. The desktop keeps all of them.

## 4. Anti-goals

The desktop spec's anti-goals apply unchanged:

- A plan never touches a payment's category.
- No converted total without its currency.
- No foreign amount counted 1:1.
- Plans never look like category budgets.

On the phone this means: plan chips are **rounded squares** (`PlanChip`), category chips are round,
and the Cashflow entry for plans is visually distinct from the budget summary note.

## 5. Decisions

All calls in this section are mine. The owner delegated them.

| # | Decision |
|---|---|
| M1 | Plans live under the **Cashflow** tab: list `/cashflow/plans?year=N`, detail `/cashflow/plans/:id`. |
| M2 | Two ways in: (a) a **plans card** on the Cashflow root, (b) "Výdajové plány" in Cashflow's `···` menu, after Spravovat rozpočty. (a) is the main one because checking is the main job. (b) always works, also with no plans. |
| M3 | The plans card shows the **current calendar year**, not the selected period. It sits right after the bars card (and after the budget note, when shown). It appears only when the current year has at least one plan. Content: square-chip-styled title "Výdajové plány 2026", "Zbývá X z Y" (active plans, report currency), a progress bar of spent / planned, and a status line "2 nad plánem · 1 blízko plánu", omitted when all plans are ok. Tap: the list for that year. |
| M4 | The list keeps the desktop overview's order (`overviewRows`: risk first, then name). Active plans come first in one section. **Uzavřené** plans follow in their own section, dimmed. A search field shows once the year has more than 8 plans. |
| M5 | List row: `PlanChip`, name, sub-line "Utraceno X z Y" (plan currency) and a progress bar. The value is "zbývá X", or "přes o X" in `loss`. A status word goes under the value ("Nad plánem" / "Blízko plánu"), or "Uzavřený" for a closed plan. Tap: detail. Long press: menu Upravit · Uzavřít/Obnovit · Smazat. No swipe actions, so a plan is never deleted by a stray swipe. |
| M6 | List hero: **Zbývá** across active plans (report currency, `remainingActive`), with the note "Utraceno X z Y". The year stepper is a ‹ 2026 › bar in the PeriodBar look. Nav: back "Cashflow", trailing `+` (new plan). |
| M7 | Empty year: empty state with "Nový plán". When N−1 has plans it also shows "Převzít n plánů z roku N−1" (primary). The take-over asks once in a confirm sheet, then creates every **open** plan of N−1 that has no same-named plan in N, with the same amount, currency, icon, color and note (`createMany`). |
| M8 | Detail: nav back "Plány N", title = plan name, trailing `···` with Upravit · Uzavřít/Obnovit plán · Smazat plán. The phone puts actions in a menu, unlike desktop O13: a phone nav bar has room for one control. Hero: **Zbývá** / **Přečerpáno o** in plan currency, note "Utraceno X z Y · 68 %", progress bar. A closed plan shows an inline alert "Plán je uzavřený" with Obnovit. |
| M9 | Detail sections, in order: **Útrata po měsících** (single-series bars of net spend per month over `planMonthAxis`; a tap selects a month, a second tap clears it), **Kategorie** (round category chip, name, "N plateb", amount, share bar; a tap selects the category, a second tap clears it), **Platby** (ledger rows; a tap opens the transaction sheet). The month filter narrows Kategorie and Platby. The category filter narrows the bars and Platby. Each active filter shows as a removable chip above the section it narrows, "Březen ×" / "Ubytování ×". |
| M10 | Payments in the detail are shown with their native amounts (as on the desktop). A foreign payment's sub-line names its currency the way the ledger already does. |
| M11 | Transaction sheet: a new row **Výdajový plán** under Kategorie, showing the plan (square chip + name) or "Bez plánu". A tap opens the **plan picker sheet**. |
| M12 | Plan picker sheet: open plans of the payment's booking year first, then open plans of other years (newest first, under a year heading). Each row: chip, name, "zbývá X z Y". The current plan is checked. Other options: "Bez plánu" (when the payment has a plan) and "Nový výdajový plán…" (opens the plan form for the booking year; on save the payment is tagged). Closed plans are not offered. With 8 or more plans offered, the sheet shows a search field. The choice uses `useAssignWithUndo` (toast with Vrátit zpět). |
| M13 | Plan form: a mobile `Sheet` (full height) with Název, Plánovaná částka (`AmountField`) + Měna (`CurrencyPickerSheet`), Rok (segment N−1 · N · N+1 around the list's year), Ikona and Barva (the shared icon set and palette from `IconColorPicker`, laid out for 360–430 pt), Poznámka (optional). Defaults and validation follow the desktop form. A taken name shows under the field. |
| M14 | Deleting a plan asks in `ConfirmSheet`, using the desktop's sentences (payments stay, they only lose the tag). From the detail, delete-and-leave returns to the list (`useDeleteAndLeave`). |
| M15 | Plan writes refresh only the plan keys (`useSpendingPlanMutations` / `useAssignWithUndo`). They change no money state (desktop rule). The transaction sheet's copy of the row follows the new `spendingPlanId` through `onChange`. |
| M17 | **Which totals (builder check).** The overview's `totals.planned` and `totals.spent` cover *all* plans of the year, closed ones too; only `remainingActive` is active-only, and there is no per-plan planned amount in the report currency (converting it on the phone would break the "only `fx.rs` converts" rule). So the card and the list hero say two separately labeled facts: the big value is **"Zbývá v aktivních plánech"** = `remainingActive` (desktop key `stats.remaining`), and the note and the progress bar are the **whole year**, "Utraceno S z P na rok N" = `totals.spent` / `totals.planned`. M3's and M6's "Zbývá X z Y" is read this way. |
| M18 | **Removing a payment from a plan** goes through its transaction sheet: Výdajový plán → "Bez plánu" (toast with Vrátit zpět). The sheet stays open and shows "Bez plánu". The plan detail's list drops the row on the refetch that the assignment's invalidation triggers. Ledger rows get no new action. |
| M16 | Strings: reuse the `spending_plans` namespace wherever the desktop already has the sentence. New phone-only strings go into `mobile` (`plans.*`), in all ten locales. |

## 6. Data and code reuse

- Queries: `useSpendingPlansOverview(year)`, `useSpendingPlanDetail(id)`,
  `useSpendingPlanTransactions(id)`, `useSpendingPlanIndex()` (picker).
- Writes: `useSpendingPlanMutations`, `useAssignWithUndo`.
- Logic: `src/utils/spending-plans.ts` (`planRow`, `overviewRows`, `planStatus`, `planMonthAxis`,
  `planMonthSpend`, `planCategoryRows`, `takeOverRows`, `monthOfDay`, `categoryKey`). Phone-only
  pure helpers go in `src/mobile/lib/plan-rows.ts` with tests: the Cashflow card summary, picker
  grouping, list sections, and filter application to payments.
- Components: `PlanChip`, the phone's `EntityRow`, `ProgressBar`, `SectionHead`, `Sheet`,
  `ConfirmSheet`, `RowMenu`, `LedgerRow`, `Screen`/`NavBar`, `ScaledEmptyState`/
  `ScaledReportError`/`ScaledReportNote`.

## 7. Quality bar

- Widths 360, 390 and 430 pt, light and dark, Dynamic Type through the existing `text-m-*` roles
  (the `type-roles` and `no-colour-literals` tests stay green).
- A value that could not be converted shows "—" with the report-quality note, never 0 (desktop
  §7).
- Every phone gate green: `npm run lint`, `typecheck`, `format:check`, `npm test` (locale parity
  included).
- Checked on the iOS simulator against the demo profile: create a plan, tag a payment from the
  transaction sheet, see it in the detail, filter by month and category, untag, close, reopen,
  delete, take over.

## 8. Left to the builder

Exact spacing and the skeletons, following the neighbouring phone pages. Whether the single-series
month bars reuse `CashflowBars` with one series or get a small component of their own. Wording of
new strings, kept close to the desktop's.
