# Mobile Settings: Kategorie, Kategorizace, Co sledovat — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three new pushed pages in the phone's Nastavení. They let a phone-only user review and undo everything the phone learns about categorization, manage categories, and change the tracked areas.

**Architecture:** Frontend only, inside `src/mobile`. Every backend call already exists in `src/lib/tauri-api.ts` (`categoriesApi`, `categorizationApi`, `authApi.updateProfile`). Pure row/label logic goes into small `src/mobile/lib/*.ts` modules with Vitest tests. Pages reuse the phone components (`SectionPage`, `SwitchRow`, `EntityRows`/`EntityRow` with swipe, `RowMenu`, `ConfirmSheet`, `Sheet`, `SearchField`). The add and edit forms reuse the desktop `CategoryFormDialog`, which renders as a phone sheet.

**Tech Stack:** React 19, TypeScript, TanStack Query, react-i18next, Vitest, Tailwind tokens of the mobile layer.

**Spec:** `docs/specs/2026-11-06-mobile-categorization-settings-design.md` (approved 2026-11-06). Read it in full before any task.

## Global Constraints

- **Repo rules.** Follow `AGENTS.md`. In particular:
  - never call `invoke()` outside `src/lib/tauri-api.ts`;
  - every user-facing string goes in all ten locales (`en cs de es it nb pl sv ja zh`), or `src/i18n/locales.test.ts` fails;
  - check `docs/DEPRECATED.md` before copying a pattern.
- **Scope.**
  - No Rust, schema or migration changes.
  - The desktop's `src/components/settings/settings-sections.ts` stays untouched.
- **Reuse desktop strings for the same concept.** Use them verbatim: `settings:categoryManagement.*`, `settings:rulePacks.*`, `categorization:*` (custom rules, `learnedTab.*`, `scopedLearning`, `legacyLearning`), the `DashboardPreferencesCard` key for excluding personal real estate, and the first-run area labels (`mobile:firstRun.*`).
- **New phone-only strings** go under `mobile:settings.tracking.*`, `mobile:settings.categories.*` and `mobile:settings.categorization.*`, plus row titles and summaries in `mobile:settings.rows.*`.
- **Mobile layer rules.**
  - No colour literals (`src/mobile/no-colour-literals.test.ts`).
  - Type roles only (`type-roles.test.ts`).
  - Icon-only buttons need `aria-label`.
- **Interaction.**
  - Every destructive action goes through `ConfirmSheet`.
  - Rows with actions offer swipe left and long press (`RowMenu`). Tap opens the same menu or the edit form, as the spec says per page.
- **Hit counts** use `HIT_DAYS` from `src/utils/rule-hits.ts` (imported, never copied).
- **Commits.**
  - Conventional Commits, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Commit only in the worktree `/Users/filipkral/Programování/FinanceApp/Moony-tauri-mobile-categorization` on branch `feat/mobile-categorization-settings`. Check `git branch --show-current` before every commit.

## Review Focus

1. **A profile whose `menuPreferences` is `null` or missing** (old profiles, desktop-created): Co sledovat shows every switch on, and Finance shows loans and insurance. Test: `areasFromPreferences(null)` returns all true (Task 1).
2. **Deleting a category that payments, rules or budgets use:** the confirm stays disabled until a "Přesunout do" target is picked, and the target list never contains the deleted category. Test: `reassignTargets` / `canConfirmDelete` (Task 2).
3. **System categories and system custom rules:** never deletable, and a system rule's switch is disabled. Tests: `categoryRowActions` (Task 2), `customRuleActions` (Task 3).
4. **A learned payee with no payee text** (IBAN-only), or whose `bankAccountId` points at a deleted account: the row still has a title and a scope line, never `undefined` or a crash. Tests in Task 3.
5. **Two quick pack toggles:** a toggle computes the next enabled set from the server state and switches are disabled while a mutation is pending (desktop behaviour), so the second tap cannot drop the first. Test: `nextEnabledPacks` (Task 3).

---

### Task 1: Co sledovat, the shared area mapping, Finance honouring loans and insurance, and the new routes

**Files:**
- Create: `src/mobile/lib/tracked-areas.ts`, `src/mobile/lib/tracked-areas.test.ts`
- Create: `src/mobile/pages/settings/TrackingSection.tsx`
- Modify:
  - `src/mobile/shell/routes.ts` (`SETTINGS_SECTIONS`) and `src/mobile/shell/routes.test.ts`;
  - `src/mobile/pages/settings/SettingsSection.tsx`;
  - `src/mobile/pages/Settings.tsx` (new rows);
  - `src/mobile/hooks/use-first-run.ts` (`saveAreas` uses the helper);
  - `src/mobile/pages/Finance.tsx` and `src/mobile/hooks/use-finance.ts` (hide loans and insurance);
  - `src/mobile/components/finance/UpcomingPremiums.tsx` if it lists premiums independently;
  - locales `src/i18n/locales/*/mobile.json`.
- Commit also: `docs/audits/2026-11-06-mobile-ux-audit.md`, the spec and this plan (already in the worktree, untracked). Commit them in this task's first commit as `docs:`.

**Interfaces:**
- Produces:
  - `export interface TrackedAreas { investments: boolean; realEstate: boolean; liabilities: boolean }`
  - `export function areasFromPreferences(prefs: MenuPreferences | null | undefined): TrackedAreas`. A group is on when any of its flags is on. `null` or `undefined` gives all true.
  - `export function preferencesWithAreas(current: MenuPreferences | null | undefined, areas: TrackedAreas): MenuPreferences`. Investments sets `investments`, `bonds` and `crypto`; realEstate sets `realEstate`; liabilities sets `loans` and `insurance`. `otherAssets` is `current?.otherAssets ?? true`.
  - `SETTINGS_SECTIONS` becomes `['profile', 'tracking', 'security', 'reminders', 'notifications', 'backup', 'categories', 'categorization', 'import']`. Tasks 2 and 3 add their pages to the `SettingsSection` switch; this task adds placeholder-free cases only for `tracking` and lets `categories` and `categorization` redirect to Nastavení until their tasks land. Keep the `switch` exhaustive with a `default` redirect.

- [ ] **Step 1: Write the failing tests** in `tracked-areas.test.ts`:
  - `areasFromPreferences(null)` → `{ investments: true, realEstate: true, liabilities: true }`;
  - prefs with only `crypto: true` among investment flags → `investments: true`;
  - all three investment flags false → `investments: false`;
  - `loans: false, insurance: true` → `liabilities: true`;
  - `preferencesWithAreas({ ...allTrue, otherAssets: false }, { investments: false, realEstate: true, liabilities: false })` → `investments`, `bonds`, `crypto`, `loans` and `insurance` false, `realEstate` true, `otherAssets` false;
  - `preferencesWithAreas(null, allOn)` → `otherAssets: true`.
- [ ] **Step 2:** Run `npx vitest run src/mobile/lib/tracked-areas.test.ts`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement both helpers.** In the same task, switch `use-first-run.ts` `saveAreas` to `preferencesWithAreas(null, areas)`. The result must stay identical to today's literal; check against the current code.
- [ ] **Step 4:** Run the test. Expected: PASS.
- [ ] **Step 5: Build `TrackingSection`.**
  - Layout: a `SectionPage` with three `SwitchRow`s, using the first-run area labels and hints. Find the keys in `pages/first-run/AreasStep.tsx`.
  - Below them, a one-line hint (`mobile:settings.tracking.hideOnly`, cs "Skryté oblasti se jen nezobrazují, data zůstávají.").
  - Then a separate `SwitchRow` for `excludePersonalRealEstate`, using the desktop `DashboardPreferencesCard` label and hint keys.
  - Saving: through `authApi.updateProfile` the way `use-first-run.ts` does it (`setQueryData(['user-profile'], profile)`). Prefer an existing profile mutation hook if `src/hooks` has one (the desktop `MenuVisibilityCard` / `DashboardPreferencesCard` use it).
  - On error: show the toast those cards use. Switches are disabled while the mutation is pending.
- [ ] **Step 6: Finance honours `loans` and `insurance`.**
  - Read `user.menuPreferences` the way `use-wealth.ts` reads it.
  - When `loans === false`, hide the Úvěry section and the "úvěr" item of the `+` menu. When `insurance === false`, hide the Pojištění section, its `+` item and the premium rows of "Nejbližší platby".
  - The subtitle counts only the shown sections.
  - Detail routes stay reachable.
- [ ] **Step 7: Routes and rows.**
  - Update `routes.test.ts` for the new `SETTINGS_SECTIONS` and add `tracking` to `SettingsSection.tsx`.
  - In `Settings.tsx`, add the "Co sledovat" row after Vzhled, with the summary "Vše" or the shown groups joined by ", ".
  - Add the Kategorie and Kategorizace rows before Import, without a summary yet; Tasks 2 and 3 add theirs.
- [ ] **Step 8: Locales.** Add the new keys to all ten `mobile.json`. Czech is the reference; translate the others sensibly. Run `npx vitest run src/i18n/locales.test.ts src/mobile`. Expected: PASS.
- [ ] **Step 9: Gates.** Run `npm run typecheck && npm run lint && npm run format:check`. Expected: no errors.
- [ ] **Step 10: Commit.**
  - First: `docs: mobile UX audit, spec and plan for categorization settings`.
  - Then: `feat(mobile): Co sledovat settings page; Finance hides loans and insurance when untracked`.

### Task 2: Kategorie

**Files:**
- Create: `src/mobile/lib/category-rows.ts`, `src/mobile/lib/category-rows.test.ts`
- Create: `src/mobile/pages/settings/CategoriesSection.tsx`, and `src/mobile/sheets/CategoryDeleteSheet.tsx` if `ConfirmSheet` cannot hold the pick row as children
- Modify: `src/mobile/pages/settings/SettingsSection.tsx`, `src/mobile/pages/Settings.tsx` (row summary), locales

**Interfaces:**
- Consumes: `categoriesApi.getCategories()`, `categoriesApi.getCategoryUsage()` (`CategoryUsage { categoryId, transactions, rules, learnedPayees, budgetGoals }`), `useCategoryMutations()` from `src/hooks/use-category-mutations.ts`, and the desktop `CategoryFormDialog`. Read `src/components/categories/CategoryManager.tsx` for the desktop query keys, grouping and delete flow, and use the same query keys so caches are shared.
- Produces:
  - `export interface CategoryRow { category: TransactionCategory; usage: CategoryUsage | null; inUse: boolean }`
  - `export function groupCategories(categories: TransactionCategory[], usage: CategoryUsage[]): { kind: string; rows: CategoryRow[] }[]`. Kind order and in-group order are copied from the desktop `CategoryManager`.
  - `export function reassignTargets(categories: TransactionCategory[], deleting: TransactionCategory): TransactionCategory[]`. Same kind first, then the others; the deleted one is excluded.
  - `export function canConfirmDelete(row: CategoryRow, reassignTo: string | null): boolean`
  - `export function categoryRowActions(row: CategoryRow): { edit: true; delete: boolean }`. `delete` is false for system categories.

- [ ] **Step 1: Write the failing tests:**
  - `groupCategories`: the desktop kind order; rows without usage get `usage: null, inUse: false`; a category with only `budgetGoals: 1` is `inUse: true` (match the desktop's in-use rule; read it).
  - `reassignTargets` never contains `deleting.id` and lists same-kind categories first.
  - `canConfirmDelete({ inUse: true }, null)` → false; `canConfirmDelete({ inUse: true }, 'x')` → true; `canConfirmDelete({ inUse: false }, null)` → true.
  - `categoryRowActions` for a system category → `delete: false`.
- [ ] **Step 2:** Run `npx vitest run src/mobile/lib/category-rows.test.ts`. Expected: FAIL.
- [ ] **Step 3:** Implement the helpers.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5: Build `CategoriesSection`.**
  - Layout: `SectionPage` with one group per kind (section head = the desktop kind label). Each row shows the category icon chip as the phone picker shows it, the name, the usage line (`settings:categoryManagement.transactionsCount` and friends; nothing when unused) and the `categoryManagement.system` badge.
  - `+` in the nav bar opens `CategoryFormDialog` for create (find how other phone pages put a `+` in the nav bar, e.g. `Budgets.tsx`). Tap on a row opens it for edit.
  - Swipe left "Smazat" and long press open a `RowMenu` with Upravit / Smazat. For a system category, Smazat is a disabled item showing `categoryManagement.systemCannotDelete`.
  - The delete confirm uses the desktop texts `deleteTitle`, `deleteInUseDescription` / `deleteUnusedDescription`, `moveTo`, `moveToPlaceholder` and `deleteConfirm`.
  - The "Přesunout do" pick row opens a list sheet of `reassignTargets`.
  - The danger button follows `canConfirmDelete`; call `deleteCategory.mutateAsync({ id, reassignTo })` exactly as the desktop does.
  - States: loading as skeleton rows; a failed query shows an error with "Zkusit znovu" (reuse the phone's existing load-failed component, e.g. `components/wealth/LoadFailed.tsx`, or its generic sibling).
- [ ] **Step 6:** Add the Nastavení row summary "N kategorií" (plural forms in every locale) and the `categories` case in `SettingsSection.tsx`.
- [ ] **Step 7:** Locales in all ten files, then run `npx vitest run src/i18n/locales.test.ts src/mobile`, `npm run typecheck`, `npm run lint` and `npm run format:check`. Expected: all pass.
- [ ] **Step 8: Commit** `feat(mobile): Kategorie settings page with reassign-on-delete`.

### Task 3: Kategorizace

**Files:**
- Create: `src/mobile/lib/categorization-rows.ts`, `src/mobile/lib/categorization-rows.test.ts`
- Create: `src/mobile/pages/settings/CategorizationSection.tsx`, `src/mobile/sheets/PackRulesSheet.tsx`
- Optional split: `src/mobile/components/categorization/{PackRow,CustomRuleRow,LearnedPayeeRow}.tsx` if the page passes ~300 lines
- Modify: `SettingsSection.tsx`, `Settings.tsx` (row summary), locales

**Interfaces:**
- Consumes:
  - `categorizationApi`: `getRulePacks`, `setRulePacksEnabled`, `getPackRules(packId)`, `setPackRuleDisabled(ruleId, disabled)`, `getCustomRules`, `updateCustomRule(id, CustomRuleInput)`, `deleteCustomRule(id)`, `getLearnedPayees`, `deleteLearnedPayee(id)`, `getRuleHitCounts(HIT_DAYS)`;
  - the bank accounts read that `use-finance.ts` uses, for account names;
  - the desktop utils in `src/utils/learned-rules.ts` and `src/utils/rule-hits.ts` (`hitMap`, `formatIbanDisplay`, `filterLearnedRules`, …), reused where they fit. Read `src/pages/CategorizationRules.tsx`, especially `LearnedRow`, the custom-rules tab and its mutations, and `src/components/settings/RulePacksCard.tsx`.
- Produces:
  - `export function sortPacks(packs: RulePackInfo[], label: (p: RulePackInfo) => string, locale: string): RulePackInfo[]`. Enabled first, then `global`, then by label (the `RulePacksCard` order).
  - `export function nextEnabledPacks(packs: RulePackInfo[], packId: string, enabled: boolean): string[]`
  - `export function customRuleActions(rule: CustomRule): { toggle: boolean; delete: boolean }`. Both false for `isSystem`.
  - `export function customRuleInput(rule: CustomRule, isActive: boolean): CustomRuleInput`. All fields are copied as the desktop `toggleCustomRule` does.
  - `export function learnedPayeeTitle(entry: LearnedPayeeEntry): string`. `originalPayee`, else `normalizedPayee`, else the formatted IBAN, else `'—'`.
  - `export function learnedPayeeScope(entry: LearnedPayeeEntry, accountName: (id: string) => string | null): { kind: 'scoped'; account: string | null; direction: 'incoming' | 'outgoing' | null } | { kind: 'legacy' }`. A deleted account gives `account: null`. The component then renders the desktop `scopedLearning` / `legacyLearning` text; a null account uses a generic "účet" wording (new mobile key if the desktop has none).

- [ ] **Step 1: Write the failing tests:**
  - `sortPacks`: an enabled pack before a disabled one; `global` first among the enabled.
  - `nextEnabledPacks`: enabling adds the id; disabling removes it; other enabled packs are kept.
  - `customRuleActions({ isSystem: true })` → `{ toggle: false, delete: false }`.
  - `customRuleInput` copies `ibanPattern` and `stopProcessing` and sets `isActive`.
  - `learnedPayeeTitle` for an IBAN-only entry returns the formatted IBAN; with nothing at all it returns `'—'`.
  - `learnedPayeeScope` for an unknown `bankAccountId` → `{ kind: 'scoped', account: null, … }`; with `bankAccountId: null` and `isCredit: null` → the legacy kind. Read the desktop `LearnedRow` for the exact rule.
- [ ] **Step 2:** Run `npx vitest run src/mobile/lib/categorization-rows.test.ts`. Expected: FAIL.
- [ ] **Step 3:** Implement the helpers.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5: Build the page.** It is a `SectionPage` with three sections, each with a one-line empty state (new mobile keys) and an error-with-retry state.
  1. **Balíčky pravidel.** One `SwitchRow` per pack, labelled through `Intl.DisplayNames` as the desktop does, with the hint `rulePacks.rulesCount`. All switches are disabled while the mutation is pending. Tapping the row text opens `PackRulesSheet`:
     - a `Sheet` with `fill` and `full`;
     - a `SearchField` filtering by name or pattern;
     - one row per rule showing "name · pattern → category" and a switch (`setPackRuleDisabled`).
  2. **Vlastní pravidla.** Each row shows the name (or pattern), the line "→ category · N× za 90 dní" (hits via `hitMap`) and an `isActive` switch built from `customRuleInput`. System rules get the badge, a disabled switch and no delete. Swipe left, long press and tap all open the menu (Vypnout/Zapnout, Smazat). Smazat goes through a `ConfirmSheet` with the desktop's custom-rule delete copy, then `deleteCustomRule`.
  3. **Naučení příjemci.** Each row shows `learnedPayeeTitle`, the category name and the scope line. "Zapomenout" by swipe left or long press opens a `ConfirmSheet` with `learnedTab.confirmForget.*`, then `deleteLearnedPayee`. A `SearchField` appears above the list when there are more than 15 entries.

  After every mutation, invalidate `rulePacks`, `packRules`, `customRules`, `learnedPayees`, `ruleHits` and `categorizationOverview`, plus the phone's categorize-queue and picker keys. Find them in `hooks/use-categorize-queue.ts` and `hooks/use-categorize.ts`, and use the same query keys as the desktop so caches are shared.
- [ ] **Step 6:** Add the Nastavení row summary "N balíčky · N vlastních pravidel · N příjemců" (plurals; omit zero parts; "Nic naučeného" when all are zero) and the `categorization` case in `SettingsSection.tsx`.
- [ ] **Step 7:** Locales in all ten files, then run `npx vitest run src/i18n/locales.test.ts src/mobile`, `npm run typecheck`, `npm run lint` and `npm run format:check`. Expected: all pass.
- [ ] **Step 8: Commit** `feat(mobile): Kategorizace settings page (packs, own rules, learned payees)`.

### Task 4: Branch gates and handoff

- [ ] **Step 1:** Run `npm test` and `npm run typecheck && npm run lint && npm run format:check`. Expected: all green. No Rust changed, so cargo gates are not needed; confirm with `git diff --stat main -- src-tauri` that the output is empty.
- [ ] **Step 2:** Whole-branch review (opus) against the spec and the Review Focus. Fix the findings and commit them as `fix(mobile): …`.
- [ ] **Step 3:** The controller verifies live in the phone shell on the demo profile, per the spec's verification line. This is not the implementer's job.
