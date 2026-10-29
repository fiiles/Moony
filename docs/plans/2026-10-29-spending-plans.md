# Spending Plans Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship "Výdajové plány": yearly named spending plans that payments are tagged with next to their category, with an overview, a detail (months, categories, payments) and tagging from the account ledger and from a cross-account picker. Desktop only.

**Architecture:** New `spending_plans` table plus a nullable `bank_transactions.spending_plan_id` (migration 026). A new Rust domain (`models/` + `services/` with all logic and in-memory tests + thin `commands/`) computes spend in the plan currency through `services/fx.rs`. The frontend adds two routed pages, a plan form, a take-over dialog, an add-payments dialog, a plan picker and a plan column with bulk selection in the existing account ledger.

**Tech Stack:** Tauri 2, Rust (rusqlite, specta), React 19 + TypeScript, TanStack Query, react-hook-form + zod, Tailwind tokens, react-i18next (10 locales), Vitest (node env, pure functions only).

**Spec:** `docs/specs/2026-10-29-spending-plans-design.md` (+ visual reference `docs/specs/2026-10-29-spending-plans-prototype.html`). Read both before any task; the prototype's Czech copy is the source copy.

## Global Constraints

- AGENTS.md "ten rules" apply to every task (invoke only in `tauri-api.ts`; 7-step type contract; per-field `#[serde(rename = "camelCase")]`; thin commands, `AppError`, `validate()`; append-only migrations; all ten locales; money TEXT, epoch INTEGER, TEXT UUID).
- Near threshold: spent ≥ 90 % of planned and not over (`NEAR_THRESHOLD = 0.9`); over: spent > planned.
- Spent = Σ booked tagged payments (debit − credit), each converted to the plan currency at its booking day with `fx::History::convert_on_tracked`; pending excluded from spent but included in `paymentCount` / `lastPaymentDate` (spec C3). Never 1:1, never 0 for unknown.
- Overview totals in the report currency via `fx::current()`; per-plan values in the plan currency.
- Assigning to a closed plan fails with `validation.spendingPlanClosed` unless `restore = true` (spec C5).
- Name unique per year, Unicode case-insensitive, checked in the service (`validation.spendingPlanNameTaken`).
- Tagging does not change money/asset state: no `recordSnapshot`, no `portfolio-metrics` / `cashflow-report` invalidation (say so in the mutation hook comment).
- Sidebar: Peníze group, after Cashflow, `ClipboardList`, routes `/spending-plans` and `/spending-plans/:id`, no `prefKey`.
- Plan chips are rounded squares; category chips stay round. One shared icon set (category `ICON_MAP` + `mountain, tree-pine, tree-palm, snowflake, party-popper, book-open, stethoscope, tent`).
- Detail's Platby list: ledger look, category as ink icon + name (no color); the Kategorie widget shows colored category chips.
- Logging: nothing personal above `log::debug!`.
- Work happens in the worktree `../Moony-tauri-spending-plans` on branch `feat/spending-plans`; commits are Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; never push.

## Review Focus

1. A plan in EUR with CZK payments and one payment in a currency without a rate: spent excludes the unconvertible one and `fxQuality` names it; the UI shows the missing marker (Task 3, Task 8).
2. Undo right after removing payments from a now-closed plan restores them (`restore = true`), while a normal assign to a closed plan is rejected (Task 2).
3. Plan names differing only by Czech case/diacritics case (`Dovolená` / `DOVOLENÁ`) collide; `Dovolena` (no diacritic) does not (Task 2).
4. A payment of a 2026 plan booked in February 2027 appears in the 2026 plan's month axis as "úno 27" and in its totals (Tasks 3, 7, 8).
5. Ledger selection survives nothing it should not: changing page, period or search clears it, so a bulk action never touches invisible rows (Task 10).

---

### Task 1: Migration 026 and the plan id on ledger rows

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const `MIGRATION_026`, entry `("026_spending_plans", MIGRATION_026)`)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `src-tauri/src/models/bank_transactions.rs` (`BankTransaction.spending_plan_id`)
- Modify: `src-tauri/src/services/bank_history.rs` (`TRANSACTION_COLUMNS`, `transaction_from_row`)
- Modify: `src-tauri/src/services/bank_accounts.rs` (the `BankTransaction { .. }` built after insert: `spending_plan_id: None`)
- Modify: `shared/schema.ts`, `shared/generated-types.ts` (regenerated), any TS object literal of `BankTransaction` the typechecker flags
- Modify: `docs/architecture/database.md` (catalog row for `spending_plans`, amended `bank_transactions` row)

**Interfaces:**
- Produces: table `spending_plans` and column `bank_transactions.spending_plan_id` exactly as spec §6; `BankTransaction.spending_plan_id: Option<String>` serialized as `spendingPlanId` (always present, like `flowKind`); TS `spendingPlanId: string | null`.

- [ ] **Step 1: Failing test** in `bank_history.rs` tests (or the module that tests `get_transactions`): `ledger_rows_carry_spending_plan_id` — insert a plan row and a transaction with `spending_plan_id = 'plan-1'`, `get_transactions` → `rows[0].spending_plan_id == Some("plan-1")`; a second untagged row → `None`. Add the column to that test module's hand-written schema.
- [ ] **Step 2:** `cd src-tauri && cargo test ledger_rows_carry_spending_plan_id` → FAIL (no field).
- [ ] **Step 3:** Write migration 026 (spec §6 SQL verbatim), append to `all_migrations()`, add the field (append `spending_plan_id` as the last column of `TRANSACTION_COLUMNS`, after the subquery, index 25).
- [ ] **Step 4:** `cargo test regenerate_schema_snapshot -- --ignored`, then `cargo test` → PASS; snapshot diff shows only the new table, index and column.
- [ ] **Step 5:** Register nothing new yet; mirror `spendingPlanId: string | null` in `shared/schema.ts`'s `BankTransaction`; `cargo test generate_bindings -- --ignored`; `npm run typecheck` → clean (fix flagged literals with `spendingPlanId: null`).
- [ ] **Step 6:** Update `docs/architecture/database.md`. Commit `feat(db): spending plans table and plan id on bank transactions`.

### Task 2: Plan CRUD, close, assignments (service + models)

**Files:**
- Create: `src-tauri/src/models/spending_plans.rs`, `src-tauri/src/services/spending_plans.rs`
- Modify: `src-tauri/src/models/mod.rs`, `src-tauri/src/services/mod.rs`

**Interfaces:**
- Produces (models, all `Serialize, Deserialize, specta::Type`, per-field renames):
  - `SpendingPlan { id, name, year: i32, amount: String, currency, icon, color, note: Option<String>, closed_at: Option<i64> "closedAt", created_at: i64 "createdAt" }`
  - `InsertSpendingPlan { name, year: i32, amount: String, currency, icon, color, note: Option<String> }` + `validate()`; keys: `validation.spendingPlanNameRequired` (empty after trim), `…NameTooLong` (> 100 chars), `…YearInvalid` (outside 2000–2100), `…AmountInvalid` (not a finite decimal > 0; use `models::validation::finite_number`), `validation.currencyInvalid` (`fx::normalize` is `None`), `…IconInvalid` (not 1–40 of `[a-z0-9-]`), `…ColorInvalid` (not `#RRGGBB`), `…NoteTooLong` (> 500). Validation normalizes: trimmed name, uppercase currency.
  - `SpendingPlanAssignment { transaction_id "transactionId", plan_id: Option<String> "planId" }`
- Produces (service, `&Connection`):
  - `create_plan(conn, data: &InsertSpendingPlan) -> Result<SpendingPlan>`
  - `create_plans(conn, items: &[InsertSpendingPlan]) -> Result<Vec<SpendingPlan>>` (one transaction, all-or-nothing, uniqueness also within the batch)
  - `update_plan(conn, id: &str, data: &InsertSpendingPlan) -> Result<SpendingPlan>` (bumps `updated_at`)
  - `get_plan(conn, id: &str) -> Result<SpendingPlan>` (`NotFound`)
  - `set_plan_closed(conn, id: &str, closed: bool) -> Result<SpendingPlan>`
  - `delete_plan(conn, id: &str) -> Result<()>` (clears tags then deletes, one transaction)
  - `set_transaction_plans(conn, assignments: &[SpendingPlanAssignment], restore: bool) -> Result<()>` (one transaction; unknown transaction or plan → `NotFound`; closed target and `!restore` → `validation.spendingPlanClosed`)
  - `pub(crate) fn name_key(name: &str) -> String` = trimmed `to_lowercase()` (Unicode).

- [ ] **Step 1: Failing tests** (`setup_test_db()` with `spending_plans`, a minimal `bank_transactions` incl. `spending_plan_id`, `status`, `tx_type`, `amount`, `currency`, `booking_date`, `category_id`, `bank_account_id`): `create_and_get_plan_roundtrip`; `validate_rejects_each_bad_field` (one assert per key above); `name_unique_per_year_unicode_case` (`Dovolená` 2026 then `DOVOLENÁ` 2026 → `spendingPlanNameTaken`; `Dovolená` 2027 ok; `Dovolena` 2026 ok); `update_keeps_own_name`; `create_plans_is_all_or_nothing` (second item duplicates first → error, count stays 0); `close_and_reopen`; `delete_clears_tags`; `assign_move_remove`; `assign_to_closed_rejected_unless_restore`; `assign_unknown_ids_not_found`.
- [ ] **Step 2:** `cargo test spending_plans` → FAIL.
- [ ] **Step 3:** Implement models and service.
- [ ] **Step 4:** `cargo test spending_plans` → PASS; `cargo clippy -- -D warnings` clean.
- [ ] **Step 5:** Commit `feat(spending-plans): plan CRUD, closing and payment assignment`.

### Task 3: Spend aggregates, overview, detail, plan payments, candidates

**Files:**
- Modify: `src-tauri/src/models/spending_plans.rs`, `src-tauri/src/services/spending_plans.rs`

**Interfaces:**
- Consumes: Task 1 columns, Task 2 models; `fx::History::load(conn, start_day, end_day)`, `History::convert_on_tracked(&mut Quality, amount, from, to, day)`, `fx::Quotes::convert`, `fx::Quality` (`track`, `merge`), `ConversionQuality::from(&Quality)`, `reporting::money_in_currency(f64, &str)`, `bank_history::{TRANSACTION_COLUMNS, transaction_from_row}`.
- Produces (models):
  - `SpendingPlanSummary { plan: SpendingPlan, spent: Option<String>, payment_count: i64 "paymentCount", last_payment_date: Option<i64> "lastPaymentDate", fx_quality: ConversionQuality "fxQuality" }` — `spent` is `None` only when there are booked payments and none converted; `Some("0.00"-style)` when there are none.
  - `SpendingPlanTotals { planned, spent, remaining_active "remainingActive": Option<String> }`
  - `SpendingPlansOverview { year: i32, currency: String, fx_quality "fxQuality", plans: Vec<SpendingPlanSummary>, totals: SpendingPlanTotals, years_with_plans "yearsWithPlans": Vec<i32> }` (years ascending)
  - `SpendingPlanMonth { month: String /* "YYYY-MM" */, spent: String }`, `SpendingPlanCategory { category_id "categoryId": Option<String>, spent: String, count: i64 }`
  - `SpendingPlanCell { month: String, category_id "categoryId": Option<String>, spent: String, count: i64 }`
  - `SpendingPlanDetail { summary: SpendingPlanSummary, months: Vec<SpendingPlanMonth>, categories: Vec<SpendingPlanCategory>, cells: Vec<SpendingPlanCell>, account_count "accountCount": i64 }` — months ascending, only months with booked payments; categories by spent desc; `cells` = the month × category breakdown the frontend sums for the filtered views (chart with a category filter, Kategorie with a month filter), so every converted number comes from the backend.
  - `PlanCandidateFilters { plan_id "planId", date_from "dateFrom": Option<i64>, date_to "dateTo": Option<i64>, account_id "accountId": Option<String>, only_unassigned "onlyUnassigned": bool, query: Option<String>, limit: Option<i64> }`, `PlanCandidatesResult { transactions: Vec<BankTransaction>, truncated: bool }`
- Produces (service): `plan_index(conn) -> Result<Vec<SpendingPlanSummary>>` (every plan of every year with its spent; feeds the ledger chips and the plan picker), `overview(conn, year: i32, report_currency: &str, quotes: &fx::Quotes) -> Result<SpendingPlansOverview>`, `detail(conn, id: &str) -> Result<SpendingPlanDetail>`, `plan_transactions(conn, id: &str) -> Result<Vec<BankTransaction>>` (newest first, incl. pending), `candidate_transactions(conn, f: &PlanCandidateFilters) -> Result<PlanCandidatesResult>` (excludes the plan's own rows; `query` matches counterparty, description and category name case-insensitively; default limit 500, max 2000 → `validation.paginationInvalid` beyond; newest first).
- Totals: `planned` = Σ plan amounts converted to report currency; `spent` = Σ converted spent; `remainingActive` = Σ over active plans of (amount − spent), converted. A plan that cannot be converted is left out of all three and recorded in the overview `fx_quality`; a plan with `spent = None` is left out of `spent` and `remainingActive`.

- [ ] **Step 1: Failing tests** (seed `exchange_rate_history` in the test schema or build `History::from_days`; use the repo's existing test helper pattern from another fx-using service test): `spent_nets_credits` (−6000, −12000, +3500 → `14500.00`); `pending_not_counted_but_listed` (count 2, spent from booked only, `plan_transactions` has both); `foreign_payment_converted_at_its_day` (EUR plan, CZK payment, rate of that day); `missing_rate_excluded_and_reported` (payment in `XYZ` → excluded, `missing_currencies == ["XYZ"]`); `months_bucket_including_next_year` (2026 plan with 2027-02-10 payment → months contain `"2027-02"`); `categories_group_uncategorized` (`None` bucket, counts); `cells_sum_to_months_and_categories`; `plan_index_lists_all_years`; `overview_totals_in_report_currency` (one CZK plan, one EUR plan, report CZK with `fx::set_current_for_test`-style quotes passed in); `overview_years_with_plans`; `candidates_filters` (other account, period, only-unassigned, query by category name, excludes own, `truncated` at limit).
- [ ] **Step 2:** `cargo test spending_plans` → new tests FAIL.
- [ ] **Step 3:** Implement. One `History::load` per call over the min..max booking day of the rows involved.
- [ ] **Step 4:** `cargo test spending_plans` → PASS; clippy clean.
- [ ] **Step 5:** Commit `feat(spending-plans): spend in plan currency, overview, detail and candidates`.

### Task 4: Commands, bindings, wire types, API wrapper, validation copy

**Files:**
- Create: `src-tauri/src/commands/spending_plans.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!`, own comment group), `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts`, `src/i18n/locales/*/common.json` (10 files)

**Interfaces:**
- Commands (thin, `db.with_conn`, `validate()` first): `get_spending_plan_index()`, `get_spending_plans_overview(year: i32, report_currency: String)` (passes `fx::current()`), `get_spending_plan_detail(id)`, `get_spending_plan_transactions(id)`, `search_plan_candidate_transactions(filters: PlanCandidateFilters)`, `create_spending_plan(data)`, `create_spending_plans(items: Vec<InsertSpendingPlan>)`, `update_spending_plan(id, data)`, `set_spending_plan_closed(id, closed: bool)`, `delete_spending_plan(id)`, `set_transaction_spending_plans(assignments: Vec<SpendingPlanAssignment>, restore: bool)`.
- TS (`shared/schema.ts`): `SpendingPlan`, `InsertSpendingPlan`, `SpendingPlanSummary`, `SpendingPlanTotals`, `SpendingPlansOverview`, `SpendingPlanMonth`, `SpendingPlanCategory`, `SpendingPlanCell`, `SpendingPlanDetail`, `SpendingPlanAssignment`, `PlanCandidateFilters`, `PlanCandidatesResult` — money fields `string`, nullable where Rust is `Option`.
- `spendingPlansApi` in `tauri-api.ts`: `getIndex()`, `getOverview(year, reportCurrency)`, `getDetail(id)`, `getTransactions(id)`, `searchCandidates(filters)`, `create(data)`, `createMany(items)`, `update(id, data)`, `setClosed(id, closed)`, `remove(id)`, `setTransactionPlans(assignments, restore = false)`.

- [ ] **Step 1:** Write the command file, module, handler entries, `collect_types()` registrations.
- [ ] **Step 2:** `cd src-tauri && cargo test generate_bindings -- --ignored && cargo clippy -- -D warnings` → clean, `shared/generated-types.ts` contains every new type.
- [ ] **Step 3:** Mirror types into `shared/schema.ts`, add `spendingPlansApi`.
- [ ] **Step 4:** Add the ten `validation.spendingPlan*` keys (Task 2 list) to `common.json` in all ten locales (EN + CS written, others translated per glossary).
- [ ] **Step 5:** `npm run typecheck && npx vitest run src/i18n/ && cd src-tauri && cargo test validation_errors_carry_keys` → PASS.
- [ ] **Step 6:** Commit `feat(spending-plans): commands, wire types and API`.

### Task 5: Anonymizer coverage

**Files:**
- Modify: `src-tauri/src/bin/local_anonymize_profile.rs`

- [ ] **Step 1:** Extend its tests (follow how `budget_goals` / `cashflow_items` money and names are covered): `spending_plans.amount` is scaled, `name` replaced by a neutral label (`Plan 1`, `Plan 2`, … per row order), `note` set to NULL; add `("spending_plans", &["amount"], "currency")` to the money-column list.
- [ ] **Step 2:** `cd src-tauri && cargo test --bin local_anonymize_profile` → FAIL, implement, → PASS.
- [ ] **Step 3:** Commit `chore(anonymizer): scrub spending plans`.

### Task 6: Frontend pure helpers

**Files:**
- Create: `src/utils/spending-plans.ts`, `src/utils/spending-plans.test.ts`

**Interfaces:**
- Produces:
  - `export const NEAR_THRESHOLD = 0.9`
  - `export type PlanStatus = 'over' | 'near' | 'ok' | 'empty' | 'unknown'`
  - `planStatus(spent: number | null, planned: number, paymentCount: number): PlanStatus` (`paymentCount === 0` → `empty`; `spent === null` → `unknown`)
  - `sortPlansByRisk<T>(rows: T[], key: (row: T) => { status: PlanStatus; ratio: number; closed: boolean }): T[]` — order: active before closed; within: over, near, unknown, ok, empty; ties by ratio desc then name is the caller's (stable sort).
  - `planMonthAxis(year: number, monthsWithPayments: string[]): string[]` — `YYYY-01..YYYY-12` extended to earlier/later months present.
  - `matchesPlanSearch(name: string, query: string): boolean` — reuse `matchesSearch` from `src/utils/holdings-search.ts` if it folds diacritics, else fold with `normalize('NFD')`.

- [ ] **Step 1: Failing tests:** `planStatus(11200, 10000, 9) === 'over'`; `(9000, 10000, 3) === 'near'`; `(8999, 10000, 3) === 'ok'`; `(10000, 10000, 1) === 'near'`; `(0, 8000, 0) === 'empty'`; `(null, 5000, 2) === 'unknown'`; risk order of a fixture with all statuses and one closed over plan last; `planMonthAxis(2026, ['2027-02'])` ends with `'2027-02'` and has 14 entries; `planMonthAxis(2026, [])` has 12; `matchesPlanSearch('Dovolená Jeseníky', 'dovo') === true`, `('Dovolená Jeseníky', 'DOVOLENA') === true`, `('Zahrada', 'dovo') === false`.
- [ ] **Step 2:** `npx vitest run src/utils/spending-plans.test.ts` → FAIL; implement; → PASS.
- [ ] **Step 3:** Commit `feat(spending-plans): status, risk order and month axis helpers`.

### Task 7: Shared icon set and icon/color picker

**Files:**
- Modify: `src/components/common/category-icons-map.ts` (8 new icons), `src/components/common/CategoryIcon.tsx` (`CategoryIconChip` gains `shape?: 'round' | 'square'` and `size?: 'sm' | 'md' | 'lg'`)
- Create: `src/components/common/IconColorPicker.tsx` (named export `IconColorPicker({ icon, color, onIconChange, onColorChange, previewLabel })`: icon radiogroup from `ICON_NAMES`, the 14 `COLOR_PRESETS`, custom color input, preview chip)
- Modify: `src/components/categories/CategoryFormDialog.tsx` (use `IconColorPicker`; move `COLOR_PRESETS` / `DEFAULT_COLOR` into the picker module and export them)

- [ ] **Step 1:** Implement; categories dialog renders identically except for the eight new icons.
- [ ] **Step 2:** `npm run lint && npm run typecheck` → clean. Commit `feat(categories): shared icon and color picker with eight new icons`.

### Task 8: Overview page, plan form, take-over dialog, hooks, navigation, i18n namespace

**Files:**
- Create: `src/hooks/use-spending-plans.ts`, `src/hooks/use-spending-plan-mutations.ts`, `src/pages/SpendingPlans.tsx`, `src/components/spending-plans/{PlanChip.tsx,PlanFormDialog.tsx,TakeOverDialog.tsx,PlansTable.tsx,PlansStats.tsx}`, `src/i18n/locales/<10 langs>/spending_plans.json`
- Modify: `src/App.tsx`, `src/components/common/nav-config.ts`, `src/i18n/index.ts`, `src/i18n/locales/*/common.json` (`nav.spendingPlans`), `src/lib/price-refresh.ts` (add `'spending-plans'`, `'spending-plan'` to `PRICE_DEPENDENT_KEYS`), `docs/standards/i18n-glossary.md` (spending plan term)

**Interfaces:**
- Consumes: Task 4 `spendingPlansApi` and types; Task 6 helpers; Task 7 `CategoryIconChip shape="square"`, `IconColorPicker`.
- Produces: `useSpendingPlanIndex()` (key `["spending-plans", "index", fxRevision]`), `useSpendingPlansOverview(year)`, `useSpendingPlanDetail(id)`, `useSpendingPlanTransactions(id)`, `usePlanCandidates(filters, enabled)`; `useSpendingPlanMutations()` → `{ createPlan, createPlans, updatePlan, setClosed, deletePlan, setTransactionPlans }` (keys and invalidation per spec §9; `setTransactionPlans` takes `{ assignments, restore? }`); `PlanChip({ plan, size })`; `PlanFormDialog({ open, onOpenChange, plan?, defaultYear, onCreated? })`; `TakeOverDialog({ open, onOpenChange, fromYear, toYear })`.
- Copy: everything from the prototype's overview, form and take-over dialog (spec §9, §10). Namespace `spending_plans`; plurals per `docs/standards/i18n.md`.

- [ ] **Step 1:** Hooks + nav + routes (`/spending-plans`, `/spending-plans/:id` → placeholder detail until Task 9) + namespace registration.
- [ ] **Step 2:** Overview page per spec §9 "Overview page" (stats, search, segment, risk-ordered table, row menu, footer, empty states, year stepper bounds); `fxQuality` missing marker next to totals and Utraceno when not complete.
- [ ] **Step 3:** `PlanFormDialog` (react-hook-form + zod, `validation.*` keys, defaults spec §9) and `TakeOverDialog` (spec §9 + C6: existing names disabled).
- [ ] **Step 4:** `npm run lint && npm run typecheck && npm run format:check && npm test` → PASS (locale parity included).
- [ ] **Step 5:** Commit `feat(spending-plans): overview page, plan form and take-over`.

### Task 9: Detail page

**Files:**
- Create: `src/pages/SpendingPlanDetail.tsx`, `src/components/spending-plans/{PlanSpendChart.tsx,PlanCategoriesCard.tsx,PlanPaymentsCard.tsx,AddPaymentsDialog.tsx,PlanPicker.tsx}`
- Modify: `src/components/stocks-analysis/FilterBadge.tsx` → move to `src/components/common/FilterBadge.tsx` with props `{ labels: string[]; onClear?: () => void; title?: string }` (stocks analysis call sites map their tags to labels); `src/App.tsx` (real detail route)

**Interfaces:**
- Consumes: Task 8 hooks/mutations/`PlanChip`/`PlanFormDialog`; Task 6 `planMonthAxis`, `planStatus`.
- Produces: `PlanPicker({ anchor/trigger, transactionIds, currentPlanIds, excludePlanId?, onAssigned })` — popover per spec §9 "Plan picker" (used again in Task 10), including "Nový výdajový plán…" via `PlanFormDialog` `onCreated`; an `useAssignWithUndo()` helper (in `use-spending-plan-mutations.ts`) that calls `setTransactionPlans`, then toasts with "Vrátit zpět" sending the previous ids with `restore: true`.
- Chart: follow `src/components/charts/` conventions (`MoonyBarChart`, `ChartAxis`, `ChartTip`, tokens `s3`, `chart-line`, `chart-cost`); bars per month from `detail.months` on the `planMonthAxis` axis, cumulative line to today's month, dashed plan line, shaded future months, value labels, month click toggles the filter; with a category filter the bars are the `detail.cells` of that category; the Kategorie card with a month filter sums the `cells` of that month (never convert on the client).
- Kategorie/Platby/filters/add-payments dialog: spec §9 "Detail page" and "Přidat platby do plánu" dialog; payment rows reuse the ledger's counterparty cell (extract a small `LedgerCounterpartyCell` from `BankAccountDetail.tsx` if it is inline) so the look matches (O15).

- [ ] **Step 1:** Move/generalize `FilterBadge`; `npm run typecheck` clean; stocks analysis unchanged visually.
- [ ] **Step 2:** Page head, stats, closed alert, delete confirm, edit/close actions.
- [ ] **Step 3:** Chart + Kategorie + Platby with both filters and badges; bulk Přesunout/Odebrat via `PlanPicker` / `useAssignWithUndo`.
- [ ] **Step 4:** `AddPaymentsDialog` on `usePlanCandidates` (search debounced 200 ms, account select from `useBankAccounts`, period select, only-unassigned switch, row toggle, footer counts incl. "se přesune z jiného plánu").
- [ ] **Step 5:** Gates (`npm run lint && npm run typecheck && npm run format:check && npm test`) → PASS. Commit `feat(spending-plans): plan detail with spend chart, categories, payments and add dialog`.

### Task 10: Account ledger: plan column, selection, bulk bar

**Files:**
- Modify: `src/pages/BankAccountDetail.tsx` (and extract `src/components/bank-accounts/LedgerBulkBar.tsx`, `src/components/bank-accounts/LedgerPlanCell.tsx`)

**Interfaces:**
- Consumes: Task 9 `PlanPicker`, `useAssignWithUndo`, `PlanChip`; `tx.spendingPlanId`; Task 8 `useSpendingPlanIndex()` for chip names, closed state and the picker's "zbývá X z Y".

- [ ] **Step 1:** Checkbox column (header selects current page) + "Výdajový plán" column (chip / hover "Přidat do plánu"; closed plan chip with tooltip "Uzavřený plán", spec C5). Detail column truncates first.
- [ ] **Step 2:** Bulk bar (spec §9 "Account ledger"); selection resets on page, period, sort and search change.
- [ ] **Step 3:** Gates → PASS. Commit `feat(bank-accounts): tag payments with spending plans from the ledger`.

### Task 11: Docs, full verification, app walk-through

**Files:**
- Modify: `docs/architecture/overview.md` (domain map row), `README.md` (feature list line), `docs/design-system/README.md` (page archetype row for the plans overview/detail, plan chip, ledger selection)

- [ ] **Step 1:** Docs.
- [ ] **Step 2:** Full gates: `npm run lint && npm run typecheck && npm run format:check && npm test` and `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; `cargo test generate_bindings -- --ignored && git diff --exit-code shared/generated-types.ts`.
- [ ] **Step 3:** Run the app on a free port with the demo profile (memory: worktree demo runs on 1421/1422, verify the page greets Alex before typing `demo1234`; never type into `moony-dev`). Walk spec §11 in light and dark and in English; screenshot overview, detail, ledger.
- [ ] **Step 4:** Commit `docs(spending-plans): architecture, design system and README`.
