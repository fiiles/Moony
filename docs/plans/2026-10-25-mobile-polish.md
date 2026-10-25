# Mobile polish — consolidation after Phases 4b/4c/6a and Cashflow 3b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify the pieces that Phases 4b, 4c and 6a built in parallel (one delete-and-leave hook, one revaluation sheet, one date chip, phone type roles for the shared desktop pieces), localise the iOS permission texts, close two hygiene items, and finish Cashflow 3b (the Finance category picker and transaction sheet in the drilldown, `Segment`, `ConfirmSheet`, Export).

**Architecture:** Every change reuses an existing phone building block instead of adding a second one: `useDeleteAndLeave` (4b), `RevalueSheet` (4b), `DateChip` (4b), `Segment` / `ConfirmSheet` / `CategoryPickerSheet` / `TransactionSheet` (Phase 2), `useExportTextFile` + the Rust text-export allow-list (Phase 5 / 4c). Shared desktop components change only additively (an optional prop); phone wrappers carry the phone sizes. Rust changes are one bounded reader and one allow-list entry, each with tests.

**Tech Stack:** React 19 + TypeScript, wouter, TanStack Query, Tailwind on the shared tokens, react-i18next (10 locales), Vitest; Rust (Tauri 2, rusqlite); xcodegen for the iOS project.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` (binding; §7 export, §8.3 agenda, §8.7 Cashflow, §8.8 details, §10 components) with the design rules of `docs/design-system/README.md` §13 ("A new phone screen must"). Hand-over sources: `docs/plans/2026-10-17-mobile-phase-3-cashflow.md` "As built / rulings" (R7, R11, R19 and the follow-ups), `docs/plans/2026-10-24-mobile-phase-4b-details.md` and `docs/plans/2026-10-24-mobile-phase-4c-classes.md` (follow-ups).

## Global Constraints

Process (parent task, 2026-10-25):

- Worktree `.claude/worktrees/epic-poincare-d69bfb`, branch `claude/mobile-polish-d69bfb`, cut from `feat/mobile` bc6d9d4a. `git branch --show-current` before every commit. Never push; never touch `main`, `feat/mobile` or another worktree; the parent session merges this branch.
- One plain shell command per call (the sandbox refuses `&&` / `;` chains except one `cd src-tauri && cargo …`, `git -C`, scripts); stage explicit paths only; never `git stash`; never commit `package-lock.json` churn from `npm install`.
- Desktop rendering and behaviour byte-identical: `src/components/**`, `src/hooks/**`, `src/utils/**`, `shared/**` change only additively (a new optional prop, a comment) and only where a task names the file; the desktop call path must provably run the same code and emit the same markup.
- `invoke()` only in `src/lib/tauri-api.ts`. Rust per `docs/standards/rust-backend.md` (thin commands, `AppError`, no `.unwrap()` outside tests), service tests on an in-memory `Connection` or pure functions; no blanket `cargo update`; `git diff --stat src-tauri/Cargo.lock` stays empty.
- Every user-facing string in all ten locales (en, cs, de, es, it, nb, pl, sv, ja, zh) in the same commit, phone strings in the `mobile` namespace; desktop strings are reused, not copied. Plural forms per `docs/standards/i18n.md` §2. A key that a task makes unused is removed from all ten files in the same commit. `src/i18n/locales.test.ts` must pass.
- Design rules, README §13 "A new phone screen must": tokens only (`src/mobile/no-colour-literals.test.ts`), `useChartTokens()` for chart colours, `text-m-*` roles (`src/mobile/type-roles.test.ts`), `text-m-field` on fields, 44 pt targets, `src/index.test.ts`. A changed component's README §13 row (and its `mobile.css` block / `mobile.html` frame where it has one) is updated in the same commit.
- Hide amounts: every amount through `<Money>` / `useMoneyText()` / `maskMoney`; counts, percents and dates stay. `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only `src/mobile/hooks/use-money-text.ts`.
- Mutations follow AGENTS.md rule 7 through the existing hooks (`refreshMoneyFlows`, the class mutation hooks).
- Gate per task before committing: `npm run format`, then `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`; a task that touches Rust also `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test`, `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`. Conventional Commit, trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>` (Claude Sonnet 5.5 / Claude Opus 5.5 / Claude Haiku 5.5).
- Subagents: implementers `sonnet` (translations `haiku`), task reviews `sonnet`, Task 6 review `opus` (a bound on untrusted input), final whole-branch review `opus`; always an explicit `model`, never Fable. Ledger `.superpowers/sdd/2026-10-25-mobile-polish/progress.md`.
- Simulator (controller only, never an implementer): iPhone 17, UDID `9B31123B-089C-451D-858B-587ACE9BEB2D`, Vite on port 1440, addressed by UDID, never `booted`; demo profile Alex Novák (`demo1234`). Screenshots `docs/screenshots/mobile/polish-NN-*.png`.

From the spec and the phone contract (Phase 1 plan "Global constraints"):

- Both platforms, one visual language, no platform chrome; reference width 390 pt, every layout checked at 360 and 430 (§5).
- Android back closes the newest sheet (`useSheetHistory`, stacked), never leaves the app from a sheet; a detail whose record is deleted leaves with no ghost of it in the history.
- Money, dates and numbers only through `useFormat()` / the phone money hooks; no 1:1 conversion, a missing rate shown as "—" (ADR 0012).
- Privacy: no new network request, no telemetry; an export goes only where the person picks in the Files picker.

## Review Focus

Inputs the spec implies and no task's feature tests exercise directly; each line's check is pinned to the owning task.

1. **A slow delete while the record drops out of its list** (a position's list refetches before the mutation's snapshot finishes; a double tap on the confirm button): the screen leaves exactly once, back lands on the class list, the screen never shows "not found" or the skeleton meanwhile, and a restored deep link is replaced by the list. Task 1: `leaveDeleted` tests stay green; the controller deletes a position, a coin and an other asset on the device and walks back.
2. **The agenda chip in the long languages at the largest text size** (de, nb, sv, pl, it at `--m-type-scale: 1.3`, 360 pt): nothing is painted outside the chip, the distance in the sub-line is never the part cut off, a met watch target still reads "Dnes / cíl". Task 3: the controller checks Přehled in German and Norwegian at 1.3 (screenshot).
3. **A drilldown pick that takes other payments along, and its undo; a meaning change; a deletion** (CategoryPickerSheet with "Zařadit i další platby", the toast's Vrátit; TransactionSheet's Automaticky / Převod; Smazat): the other rows the list holds turn "přesunuto" and turn back on undo (the desktop's `applyBulkChanges` / `restoreBulkChanges`), a payment whose meaning leaves the list's flow is "přesunuto", a deleted payment leaves the list, and the report behind refreshes. Task 7a: `drilldownRows` tests for the flow rule; the controller checks one bulk pick and its undo on the device.
4. **An upload body that arrives as a JSON array** (the postMessage fallback on Android): an array longer than the limit is refused before a byte vector is built; an array with a bad element is still an internal error; the raw body has the same limit. Task 6: `upload_bytes_*` tests with a small limit.
5. **Export of a period that is still loading or empty, with amounts hidden** (Export tapped during a period step, an empty month, the eye closed): no file from the previous period's data, "Žádná data k exportu" for an empty period, the CSV carries real amounts (an export the person asks for is not masked — the desktop's export is not either). Task 7c: `cashflowCsv` tests for the empty report and the counted rows; the controller exports October and opens the file.

---

### Task 1: One delete-and-leave (Position, Crypto, OtherAsset)

**Files:**
- Modify: `src/mobile/pages/PositionDetail.tsx`, `src/mobile/pages/CryptoDetail.tsx`, `src/mobile/pages/OtherAssetDetail.tsx` (each owns `deleteOpen`, the `ConfirmSheet` and `useDeleteAndLeave`)
- Modify: `src/mobile/components/wealth/{PositionMenu,CryptoMenu,OtherAssetMenu}.tsx` (the confirm sheet and the delete mutation leave the menu; `onDeleted` becomes `onDelete`)
- Modify: `src/mobile/hooks/use-position.ts` (`remove` invalidates the domain keys with `refetchType: 'inactive'`, as `useDeleteCrypto` does), comments in `src/mobile/hooks/use-crypto.ts`, `src/mobile/hooks/use-other-asset.ts` that name `useLeaveDeleted`
- Delete: `src/mobile/hooks/use-leave-deleted.ts`
- Modify: `docs/design-system/README.md` §13 rows of the position, crypto and other-asset details (deleting is the page's, as the loan's)

**Interfaces:**
- Consumes: `useDeleteAndLeave({ home, pathOf, keysOf, remove, closeSheet }): { deleting: boolean; run: (id: string) => void }` (`src/mobile/hooks/use-delete-and-leave.ts`); `useRetained<T>(value: T | null): T | null`; `positionPath`, `cryptoPath`, `otherAssetPath`, `routes.stocks`, `routes.cryptoList`, `routes.otherAssetsList`.
- Produces: `PositionMenu({ holding, onDelete })`, `CryptoMenu({ holding, onDelete })`, `OtherAssetMenu({ asset, valuations, onDelete })` — `onDelete: () => void` asks the page to open its confirm sheet (the `LoanMenu` shape).

- [ ] **Step 1: Pages own the delete.** In each page: `const [deleteOpen, setDeleteOpen] = useState(false)`; `const exit = useDeleteAndLeave({ home, pathOf, keysOf, remove: () => remove.mutateAsync(), closeSheet: () => setDeleteOpen(false) })` with the keys the pages pass today (position `['investment', id]`, `['investment-transactions', id]`; coin `['crypto-transactions', id]`; asset `['other-asset-transactions', id]`, `['other-asset-valuations', id]`); the `ConfirmSheet` moves from the menu to the page unchanged (same title, description, label, `isPending={remove.isPending}`), `onConfirm={() => exit.run(id)}`. While `exit.deleting` the page keeps its last loaded screen: never the not-found line, never the skeleton, the `···` stays (`useRetained` on a memoised view whose identity changes only with its content — `LoanDetail`'s `LoanView` pattern; an object rebuilt every render loops `useRetained`).
- [ ] **Step 2: Menus lose the delete.** The danger item calls `onDelete`; the menus no longer import `ConfirmSheet`, `leaveAfterSheet` or the delete mutation. The position's `remove` keeps its toast and snapshot and invalidates `DELETE_KEYS` with `refetchType: 'inactive'` (the open screen reads the list it invalidates).
- [ ] **Step 3: Delete `use-leave-deleted.ts`.** `grep -rn "use-leave-deleted\|useLeaveDeleted" src docs/design-system` prints nothing.
- [ ] **Step 4: Gates and commit.** `npx vitest run src/mobile/hooks/use-delete-and-leave.test.ts` PASS, the full gate, commit `refactor(mobile): one delete-and-leave for positions, coins and other assets`.

### Task 2: One revaluation sheet (other asset uses `RevalueSheet`)

**Files:**
- Create: `src/mobile/lib/revalue.ts`, `src/mobile/lib/revalue.test.ts` (move `RevalueForm`, `RevalueErrors`, `revalueData` and their tests out of `property-detail.ts` / `property-detail.test.ts`; `todayIso` and `isoToUtcDay` stay there — `CostSheet` uses them)
- Modify: `src/mobile/sheets/RevalueSheet.tsx` (generic props), `src/mobile/pages/PropertyDetail.tsx`, `src/mobile/pages/OtherAssetDetail.tsx` (drop `RevalueDialog` and its memoised `revalueAsset`)
- Modify: `docs/design-system/README.md` §13 (Property detail row's sheet sentence; Other asset row: Přecenit opens the phone sheet)

**Interfaces:**
- Consumes: `ValuationKind` and `useValuationMutations(kind, assetId)` (`src/hooks/use-valuation-mutations.ts`), `priceDecimals` (`@shared/currencies`).
- Produces: `RevalueSheet({ open, onOpenChange, kind: ValuationKind, assetId: string, name: string, value: number, currency: string, unit?: string, quantity?: number })` — `value` is today's estimate in `currency` (per unit when `unit` is given), `quantity` the units held; `startRevalueForm(value: number, hidden: boolean, today: string): RevalueForm`; `revaluedTotal(valueText: string, quantity: number): number | null`.

- [ ] **Step 1: Failing tests** in `revalue.test.ts`: the moved `revalueData` cases unchanged; `startRevalueForm(5_400_000, false, '2026-10-25')` → `{ valueText: '5400000', dateIso: '2026-10-25', note: '' }`, with `hidden` → `valueText: ''`; `revaluedTotal('1 950,5', 2)` → `3901`, `''` / `'abc'` / `'-1'` → `null`.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/revalue.test.ts` — FAIL (module missing).
- [ ] **Step 3: Implement** `revalue.ts`; `property-detail.ts` no longer exports the moved names.
- [ ] **Step 4: Generic sheet.** Label `common:revalue.pricePerUnit` with `unit` when given, else `common:revalue.marketValue`; hint: with `quantity` and a valid typed amount `common:revalue.totalNote` (`total` = `revaluedTotal` in whole units, `previous` = `value` with `priceDecimals(value, currency)`), otherwise `common:revalue.previousNote` (`previous` = `value`, whole units for a property, `priceDecimals` for a per-unit price) — both through `useMoneyText`; title `common:revalue.title` with `name`; the primary button `common:revalue.submit` for every kind (ruling 8). Behaviour otherwise as today (empty field while amounts are hidden, today at most, a double Enter saves once, the mutation's toast on refusal). Property passes `kind="realEstate"`, `assetId`, `name`, `value = Number(marketPrice) || 0`, `currency = recordCurrency(marketPriceCurrency)`; the other asset passes `kind="otherAsset"`, `value = row.price`, `currency = recordCurrency(asset.marketPriceCurrency)`, `unit = otherAssets:table.unit`, `quantity = row.quantity`.
- [ ] **Step 5: Gates and commit** `refactor(mobile): one revaluation sheet for properties and other assets`.

### Task 3: Agenda row on `DateChip`

**Files:**
- Modify: `src/mobile/components/AgendaRow.tsx`
- Modify: `docs/design-system/README.md` §13 (Date chip row: the agenda row uses `DateChip`; Entity row / Row menu rows unchanged), `docs/design-system/mobile.html` (the agenda rows' chips show the weekday, the distance moves to the sub-line, `.m-row__s--split`)

**Interfaces:**
- Consumes: `DateChip({ day, caption, urgent?, label? })`, `DATE_CHIP_WIDTH` (`src/mobile/components/DateChip.tsx`); `relativeLabel`, `daysBetween`, `agendaDay`, `isUrgent` (`src/utils/milestones.ts`); the sub-line pattern of `UpcomingPremiums` (a truncating part, then `· distance` as `flex-none`).

- [ ] **Step 1: Implement.** The chip is `<DateChip day={day} urgent={urgent} caption={fmt.day(day, { weekday: 'short' })} />`, a met watch target `<DateChip day={day} urgent={urgent} label={t('card.today')} caption={t('card.target')} />`; `chipWidth={DATE_CHIP_WIDTH}`; the local `DATE_CHIP` constant and the inline chip go. The sub-line is `<span className="flex min-w-0">` with `subLine(m)` truncating, then `<span className="flex-none whitespace-pre"> · </span><span className="flex-none">{distance}</span>` (`milestones:relative.<key>` with its count); a watch target has no distance; an empty `subLine(m)` leaves the distance alone. Swipes, long press, menu and accessibility unchanged. No unit test: a component (testing policy); the check is Review Focus 2.
- [ ] **Step 2: Gates and commit** `fix(mobile): agenda rows use the date chip with the weekday, the distance in the sub-line`.

### Task 4: Shared desktop pieces scale with Dynamic Type on the phone

**Files:**
- Create: `src/mobile/components/Scaled.tsx` (phone wrappers)
- Modify (additive): `src/components/common/EmptyState.tsx` (`titleClassName?`, `descriptionClassName?`), `src/components/reports/ReportQualityNote.tsx` (`className?` on `ReportQualityNote` and on `ReportError`)
- Modify: every phone importer of the five pieces — `src/mobile/components/wealth/{ClassEmpty,PositionHeroCard,ClassHero}.tsx`, `src/mobile/components/{cashflow/CashflowBars,loan/LoanHeroCard,property/PropertyHeroCard}.tsx`, `src/mobile/pages/{Stocks,Cashflow,PropertyDetail,CryptoDetail,Finance,Crypto,Overview,OtherAssets,Wealth,Budgets,CashflowCategory,Bonds,PositionDetail,OtherAssetDetail,WatchDetail}.tsx`, `src/mobile/pages/finance/{AccountDetail,CategorizeQueue}.tsx`
- Modify: `src/mobile/type-roles.test.ts` (the import gate), `docs/design-system/README.md` §13 (a row "Scaled desktop pieces")

**Interfaces:**
- Produces (`src/mobile/components/Scaled.tsx`): `ScaledEmptyState(props: EmptyStateProps)` (title `text-m-section`, description `text-m-sub`), `ScaledChartLegend(props)` (`text-m-note`; swatches and the `font-600` stay), `ScaledConversionNote(props)` (`text-m-note font-600`), `ScaledReportNote(props)` (`text-m-note font-600`), `ScaledReportError(props)` (`text-m-sub`), and `export type { LegendItem }`. Each passes its classes through the component's own class props, merged after the caller's `className` so a caller's spacing stays. At the default text size each renders at the size and weight it has today (10/600, 12, 16/620).

- [ ] **Step 1: Failing gate** in `type-roles.test.ts`: a new `describe` that lists every non-test file under `src/mobile/` importing from `@/components/common/EmptyState`, `@/components/charts/ChartLegend` or `@/components/reports/ReportQualityNote` and expects only `components/Scaled.tsx` (message: "import the phone wrapper from @/mobile/components/Scaled"). Plus one unit case on the scanner with a two-line import (`} from '@/components/charts/ChartLegend';`).
- [ ] **Step 2: Run** `npx vitest run src/mobile/type-roles.test.ts` — FAIL (the current importers are listed).
- [ ] **Step 3: Implement** the additive props (desktop strings unchanged when the prop is absent: `cn(<the existing string>, titleClassName)`), `Scaled.tsx`, and switch every phone importer. Run the gate: PASS.
- [ ] **Step 4: Desktop proof (in the review, not committed).** Copy the three base files (`git show feat/mobile:<path>`) next to the new ones under a temporary name, render each desktop call shape with `react-dom/server` `renderToStaticMarkup` from both (EmptyState with and without `bare`, with an action, with a `className`; ReportQualityNote with and without `planning`; ReportError; ConversionQualityNote and ChartLegend are untouched files), assert the markup is equal, delete the temporary files; paste the result into the report.
- [ ] **Step 5: Gates and commit** `fix(mobile): empty states, legends and quality notes follow Dynamic Type on the phone`.

### Task 5: Localised permission texts (`InfoPlist.strings`)

**Files:**
- Create: `src-tauri/gen/apple/Localizations/{en,cs,de,es,it,nb,pl,sv,ja,zh-Hans}.lproj/InfoPlist.strings`
- Modify: `src-tauri/gen/apple/project.yml` (target `moony_iOS` sources: `- path: Localizations` with a comment), regenerated `src-tauri/gen/apple/moony.xcodeproj/project.pbxproj` (`xcodegen generate` run from `src-tauri/gen/apple`, the generator the Tauri CLI used for this project; nothing else in it may change)
- Create: `src/mobile/permission-strings.test.ts`
- Modify: `docs/architecture/overview.md` (the "Identifier and project files" bullet names the localised strings)

**Interfaces:**
- Produces: each file is UTF-8 with exactly two entries, `"NSFaceIDUsageDescription" = "…";` and `"NSCameraUsageDescription" = "…";`, each preceded by a `/* … */` comment. English is `Info.ios.plist`'s text verbatim; Czech: Face ID "Moony odemyká vaše data pomocí Face ID.", camera "Moony fotí dokumenty a vaši nemovitost; fotky se ukládají zašifrované v tomto telefonu."; the other eight match their `mobile.json`'s form of address (de Sie, es tú…), "Face ID" untranslated. `zh-Hans` is the app's `zh` (Simplified).

- [ ] **Step 1: Failing test** `permission-strings.test.ts` (fs through a variable specifier, the `src/index.test.ts` pattern): for each of the ten folders the file exists, parses into exactly the two keys with non-empty values, contains no unescaped `"` inside a value; the `en` values equal the two `<string>`s of `src-tauri/Info.ios.plist`; `project.yml` lists `path: Localizations` under `moony_iOS`; the pbxproj names every one of the ten regions in `knownRegions`.
- [ ] **Step 2: Run** `npx vitest run src/mobile/permission-strings.test.ts` — FAIL.
- [ ] **Step 3: Implement** the files, the `project.yml` entry and the regenerated pbxproj (`git diff --stat src-tauri/gen/apple` shows only `project.yml`, `project.pbxproj` and the new files). PASS.
- [ ] **Step 4: Gates and commit** `feat(mobile): Face ID and camera permission texts in the ten app languages`. (The controller checks the built bundle: every `<lang>.lproj/InfoPlist.strings` inside `Moony.app`.)

### Task 6: Bounded upload bodies; no dead placeholders — reviewer `opus`

**Files:**
- Modify: `src-tauri/src/services/real_estate_photos.rs` (`photo_bytes` → `upload_bytes` with a limit; tests)
- Modify: `src-tauri/src/commands/insurance.rs`, `src-tauri/src/commands/real_estate.rs` (the three call sites)
- Modify: `src/mobile/pages/WealthPlaceholder.tsx` (only `RealEstatePlaceholder` stays; `Kind` and the property query go)

**Interfaces:**
- Produces: `pub fn upload_bytes(body: &InvokeBody, max_len: u64) -> Result<Cow<'_, [u8]>>` — a raw body longer than `max_len`, or a JSON array with more than `max_len` items, is `AppError::Validation("validation.attachmentTooLarge")` before anything is copied or converted; any other JSON, or an item that is not a byte, stays `AppError::Internal` (the old message). Every call site passes `attachment_store::MAX_DOCUMENT_BYTES` (ruling 11).

- [ ] **Step 1: Failing tests** (the three existing `photo_bytes_*` tests renamed to `upload_bytes_*` with a limit of 16), plus `upload_bytes_refuses_a_json_array_longer_than_the_limit` (`json!([1, 2, 3, 4, 5])` with limit 4 → `Validation("validation.attachmentTooLarge")`; `json!([1, 2, 3, 4, "x"])` with limit 4 → `Validation` too, which shows the length is checked before any item), `upload_bytes_takes_an_array_at_the_limit` (4 items, limit 4 → those bytes), `upload_bytes_refuses_a_raw_body_over_the_limit` (`Raw(vec![0; 5])`, limit 4 → `Validation`).
- [ ] **Step 2: Run** `cd src-tauri && cargo test upload_bytes` — FAIL.
- [ ] **Step 3: Implement** and switch the three commands. PASS.
- [ ] **Step 4: Placeholder.** `WealthPlaceholder.tsx` keeps one default export for the Nemovitosti list route (`RealEstatePlaceholder`, still mounted in `MobileShell.tsx`); the four unused exports, the `property` branch and its query go.
- [ ] **Step 5: Gates (Rust and TS) and commit** `fix(attachments): refuse an oversized upload array before reading it; drop dead phone placeholders`.

### Task 7a: Cashflow drilldown on the Finance picker and transaction sheet

**Files:**
- Modify: `src/mobile/pages/CashflowCategory.tsx`
- Delete: `src/mobile/sheets/CashflowCategoryPickerSheet.tsx`
- Modify: `src/mobile/lib/cashflow-drilldown.ts`, `src/mobile/lib/cashflow-drilldown.test.ts`
- Modify (additive): `src/mobile/sheets/CategoryPickerSheet.tsx` (`onHistoryApplied?`, `onHistoryRestored?`), `src/mobile/hooks/use-categorize.ts` (`PickOptions.onRestored?`)
- Modify: ten `src/i18n/locales/*/mobile.json` (remove the `cashflow.picker.*` keys nothing uses any more), `docs/design-system/README.md` §13 (Cashflow drilldown sentences), `docs/plans/2026-10-17-mobile-phase-3-cashflow.md` is not edited (a frozen plan)

**Interfaces:**
- Consumes: `CategoryPickerSheet({ tx, open, onOpenChange, categories, recentIds, onChange?, onApplied? })`; `TransactionSheet({ tx, open, onOpenChange, category, flow, onPickCategory, onChange?, onDeleted? })`; `useRecentCategoryIds(accountId)` (`src/mobile/hooks/use-categorize-queue.ts`) and `getCompleteBankHistory(accountId)` under its key `['bank-transactions', accountId, 'history']`; the effective flow query `['effective-transaction-flows', [id]]` → `cashflowApi.getEffectiveFlows([id])` (as `CategorizeQueue`); `applyBulkChanges`, `restoreBulkChanges` (`src/utils/report-drilldown.ts`).
- Produces: `drilldownRows(rows, changes, targetKey, dayLabel, flows?: { flowKind: FlowKind; current: ReadonlyMap<string, FlowKind> })` — a row is also `moved` when `current` holds a flow for it other than `flowKind`; `CategoryPickerSheet` props `onHistoryApplied?: (changed: ChangedRow[], categoryId: string) => void` (called once the learning took other payments along; the sheet keeps its toast and undo — unlike `onApplied`, it does not make the sheet quiet) and `onHistoryRestored?: (changed: ChangedRow[]) => void` (called after the toast's undo restored them); `PickOptions.onRestored?: (changed: ChangedRow[]) => void`.

- [ ] **Step 1: Failing tests** in `cashflow-drilldown.test.ts`: in an `expense` list a row with `current.get(id) === 'transfer'` is `moved` with no category change; with `'expense'` it is not; a row in neither map is not; the category rule is unchanged when `flows` is absent.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/cashflow-drilldown.test.ts` — FAIL.
- [ ] **Step 3: Implement** the flow rule. PASS.
- [ ] **Step 4: Page.** A row's **tap** opens the `TransactionSheet`; **swipe left** and the menu's "Změnit kategorii" open the `CategoryPickerSheet` directly; the transaction sheet's category row opens the picker over it (stacked sheets). Both need the full `BankTransaction`: the page `fetchQuery`s the account's complete history (the key `useRecentCategoryIds` reads, so the picker's recents share it) and finds the row by id, with `categoryId` set to the change made here (`withCategory`, as `CategorizeQueue`); a row that is no longer there is treated as deleted. State: `changes` (category picks: the picker's `onChange` patch; bulk picks via `onHistoryApplied` → `applyBulkChanges`, undo via `onHistoryRestored` → `restoreBulkChanges`), `flows` (the effective flow read by the open transaction sheet's flow query, recorded per row whenever it resolves), `removed` (TransactionSheet's `onDeleted`: the row leaves the snapshot). The summary's moved count counts `moved` over all rows (`drilldownRows` over the whole snapshot), the list shows the first `limit`. The page's own `changeCategory` and `CashflowCategoryPickerSheet` go: the picker saves through `useCategorize` (which refreshes the money flows).
- [ ] **Step 5: Strings, README, gates, commit** `feat(mobile): cashflow drilldown opens the transaction sheet and the Finance category picker`.

### Task 7b: Period segment, delete confirmation, the accounts sheet's Hotovo

**Files:**
- Modify: `src/mobile/components/cashflow/PeriodBar.tsx` (the `Segment`), `src/mobile/sheets/BudgetDeleteSheet.tsx` (`ConfirmSheet` as its body)
- Modify: `docs/design-system/README.md` §13 (the period bar and the budget delete rows name `Segment` / `ConfirmSheet`)

**Interfaces:**
- Consumes: `Segment<T>({ value, options, onChange, label, className? })`; `ConfirmSheet({ open, onOpenChange, title, description, confirmLabel, onConfirm, isPending?, confirmDisabled? })`.
- Produces: unchanged `PeriodBar` and `BudgetDeleteSheet` props.

- [ ] **Step 1: Implement.** `PeriodBar` renders `<Segment value={timeframe} options={…periods.timeframe.*} onChange={onTimeframeChange} label={t('periods.label')} className="w-full" />` — the same markup classes, so nothing moves; the stepper stays. `BudgetDeleteSheet` keeps `PendingDelete`, its retained `shown`, the two mutations and its strings and renders one `ConfirmSheet` (`open={pending !== null}`, `onOpenChange={(open) => !open && onClose()}`, `isPending`, `onConfirm={confirm}`; `description` falls back to `''` while nothing is shown).
- [ ] **Step 2: Hotovo.** No code unless the device check fails: the controller opens `···` › Účty, unticks an account, Hotovo, and checks that the report rescopes, back (Android-style `history.back()`) returns to Přehled without passing through the sheet, and the `?acc=` in the URL survives a push to the drilldown and back (Phase 3 R19 under the stacked `useSheetHistory`). A failure is fixed here with `afterHistorySettles`'s contract and a test in `history-settle.test.ts`.
- [ ] **Step 3: Gates and commit** `refactor(mobile): cashflow period segment and budget delete on the shared components`.

### Task 7c: Export in Cashflow `···`

**Files:**
- Create: `src/mobile/hooks/use-csv-export.ts` (the run, toasts and pending state of `useClassExport`, generalised), `src/mobile/lib/cashflow-export.ts`, `src/mobile/lib/cashflow-export.test.ts`, `src/mobile/components/cashflow/CashflowMenu.tsx` (the `···` out of `Cashflow.tsx`)
- Modify: `src/mobile/hooks/use-class-export.ts` (a thin wrapper over `useCsvExport`), `src/mobile/pages/Cashflow.tsx`
- Modify: `src-tauri/src/services/backup.rs` (`TEXT_EXPORT_FILE_NAMES` gains `"moony-cashflow.csv"`; tests)
- Modify: `docs/design-system/README.md` §13 (Cashflow row: Export in `···`), `docs/architecture/overview.md` if it lists the staged export names

**Interfaces:**
- Consumes: `useExportTextFile().exportText(fileName, contents): Promise<ExportOutcome>`; `buildCashflowCsv(report, monthLabel, groupLabel)` (`src/utils/report-currencies.ts`); `categoryTitle(...)` (`src/mobile/lib/cashflow-rows.ts`); `tc('buttons.export')`, `common:export.*`.
- Produces: `useCsvExport(): { run: (fileName: string, load: () => Promise<{ csv: string; count: number }>) => Promise<void>; isPending: boolean }`; `CASHFLOW_EXPORT_FILE = 'moony-cashflow.csv'`; `cashflowCsv(actuals: CashflowActuals, monthLabel: (month: number) => string, groupLabel: (group: CashflowGroup) => string): { csv: string; count: number }` (count = months + expense + income + transfer groups, the desktop's; `csv` is `buildCashflowCsv`'s output; a report with no transaction gives count 0); `CashflowMenu({ accountsLabel, onAccounts, budgetsHref, exportDisabled, onExport })`.

- [ ] **Step 1: Failing tests.** `cashflow-export.test.ts`: a fixture `CashflowActuals` with two months and one group per section → count 5 and the desktop's header lines (`month;income_czk;expenses_czk`, `expense_category;amount_czk;transactions`); `transactionCount === 0` → count 0. Rust in `backup.rs`: `text_export_file_name_is_on_the_allow_list_only` lists `moony-cashflow.csv` among the accepted names, `cashflow.csv` and `moony-cashflow.csv.txt` among the refused; `a_text_export_is_written_to_the_documents_folder_and_overwritten` works with it.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/cashflow-export.test.ts` and `cd src-tauri && cargo test text_export` — FAIL.
- [ ] **Step 3: Implement.** `cashflow-export.ts` and the allow-list entry (`[&str; 6]`). PASS.
- [ ] **Step 4: Menu and hook.** `CashflowMenu` holds Účty (N), Spravovat rozpočty, a separator and Export (`Download`, `tc('buttons.export')`), as `RowMenuItem`s with 18 pt icons like the class pages' `···`. Export runs `useCsvExport().run(CASHFLOW_EXPORT_FILE, …)` with the shown report (`actuals`, the phone's `categoryTitle` labels, months as `fmt.day(month, { year: 'numeric', month: '2-digit' })`); disabled while the report is a placeholder of the previous period, has no data yet, or an export runs. `useClassExport(kind)` keeps its signature over `useCsvExport`.
- [ ] **Step 5: Gates (Rust and TS) and commit** `feat(mobile): export the cashflow report as CSV from the Cashflow menu`.

---

## As built / rulings

Rulings taken while writing this plan (spec and README §13 are the authority; each line says what was decided and what it costs if wrong). The list grows during execution.

1. **Branch from `feat/mobile` bc6d9d4a** (Phases 1–6a, R25): every piece to unify is there. Cost: none.
2. **Deleting a position, a coin or an other asset is the page's** (`useDeleteAndLeave`, the loan's shape): the menu unmounts when the record drops out of its list and takes a per-call `onSuccess` with it, which is what left the loan hanging. Cost: three pages each hold one more sheet.
3. **The position's delete stops refetching its open list** (`refetchType: 'inactive'`, as the coin's and the asset's already do): the screen keeps its data until it has left; the list that mounts next refetches. Cost: none seen; the class list is stale for at most the 250 ms guard.
4. **`revalueData` moves to `src/mobile/lib/revalue.ts`** with the sheet's other pure rules; `todayIso` / `isoToUtcDay` stay in `property-detail.ts` (`CostSheet` uses them). Cost: one more import path.
5. **The other asset's hint shows the position value only once a valid amount is typed**; before that the current per-unit price ("Oproti dosavadnímu odhadu …"). Cost: none.
6. **The agenda chip shows the weekday, the distance moves to the sub-line** (4b measured "in 2 Monaten" at 56 px against a 54 pt chip); a met watch target keeps "Dnes / cíl" in the chip. Cost: the sub-line's own text ends with an ellipsis one word earlier.
7. **Phone wrappers, not a phone mode in the desktop components** (`Scaled.tsx`, guarded by a gate in `type-roles.test.ts`): the desktop files gain only optional class props, so the desktop call path is untouched. `ReportQualityNote` and `ReportError` join the three named pieces (Cashflow uses them; Phase 3 deferred "ReportError v desktopové velikosti"). Cost: a sixth file name to learn.
8. **One button label for every revaluation: `common:revalue.submit`** (the desktop dialog's "Uložit odhad"), ("Save estimate"; cs "Uložit přecenění"), instead of borrowing "Přecenit" from a class namespace. Cost: the property sheet's button changes from "Přecenit" to "Uložit přecenění" in cs.
9. **The permission texts live in `src-tauri/gen/apple/Localizations/`**, listed in `project.yml`, the project regenerated with `xcodegen generate`. Side effect: the app now declares ten localizations, so system UI inside it (the date and month wheels, the text-selection menu, the photo picker) follows the device language instead of English, and iOS Settings › Moony offers a per-app language. WKWebView's `navigator.language` may now follow the device language too, which `src/mobile/hooks/use-first-run.ts` uses for the first-run default region, currency and rule pack (check on a non-English device before the next release). Cost: if the owner prefers English system UI, `CFBundleAllowMixedLocalizations` would be the next question.
10. **`zh-Hans.lproj` for the app's `zh`** (the UI is Simplified Chinese, PingFang SC). Cost: a Traditional-Chinese device gets English permission texts.
11. **Photos get the document limit too** (`MAX_DOCUMENT_BYTES`, 100 MB): a photo upload had no bound, and one reader serves both. Cost: none (a camera photo is a few MB).
12. **Only `RealEstatePlaceholder` stays**: its route is still mounted for a deep link to `/wealth/real-estate`, which nothing links to. Cost: none.
13. **The drilldown's tap opens the transaction sheet; swipe left and the row menu keep the one-step category change** (the spec's swipe action is the category). Cost: one more tap for a category change from a tap.
14. **The full transaction comes from the account's complete history** (the query the picker's recents already read), not from a new backend command; a report row lacks the import, status and suggestion fields the sheet shows. Cost: the first tap on an account's row waits for that account's history, and every pick reloads the account's complete history in the background (the picker's recents observe it).
15. **A meaning change and a deletion count like a category change in the drilldown** ("přesunuto" for a payment that left the list's flow, a deleted one leaves the list): the snapshot never reshuffles otherwise. Cost: none.
16. **Bulk picks and their undo follow the desktop drilldown** (`applyBulkChanges` / `restoreBulkChanges`), through two additive picker callbacks that keep the picker's own toast and undo. Cost: two optional props on a shared phone sheet.
17. **Cashflow Export writes `moony-cashflow.csv`** (a fixed name on the Rust allow-list, like the class exports), not the desktop's dated `cashflow-2026-10.csv`. Cost: two exports of different periods overwrite each other in the same folder unless the person renames one in the picker.
18. **The exported CSV is not masked** when amounts are hidden: the eye hides the screen, an export is a file the person asked for (the desktop's export does the same). Cost: none.

Rulings taken during execution (2026-10-25), each with what it costs if wrong; the ledger (`.superpowers/sdd/2026-10-25-mobile-polish/progress.md`) has the full trail:

19. **`feat/mobile` is merged into the branch twice**: @3fe2ff7f after Task 1 and @ab92825b after Task 7a, each while the tree was clean, so the later tasks and the final review build on it. Conflicts: the `AgendaRow` doc, the `AccountDetail` imports, the README rows; `upload_bytes` keeps the parent's Internal message. Cost: Task 1's review diff predates the first merge; none otherwise.
20. **No per-class README rows for the position, coin and other-asset details**: the plan named rows that do not exist, so the shared delete is named in the Loan detail row. Cost: one docs edit.
21. **`font-semibold` instead of the plan's `font-600`** in the `Scaled.tsx` wrappers: this config compiles no CSS for `font-600`, and the plan's intent (keep 10 / 600) wins over its class name. Cost: none (the desktop `ChartLegend`'s own `font-600` is an older dead class, out of scope).
22. **The pbxproj is a graft of xcodegen's Localizations entries**: a plain run churns unrelated parts of it on this branch, and the graft holds exactly what xcodegen adds for `Localizations/`. Cost: the next real xcodegen run rewrites the same entries (no functional difference).
23. **Task 7b ships no Hotovo code**: the device check passed (the sheet's history entry, back, forward and `?acc=` all behave), and the brief allows code only on a failure. Cost: none.
24. **Task 7a is reviewed on opus**, not the plan's sonnet: its async and stacked-sheet state is the riskiest UI logic of the branch. Cost: a pricier review seat.
25. **Device evidence comes from the browser pane over the dev bridge**, plus simulator screenshots where the app could be driven: simulator keyboard injection was unreliable, and its hardware keyboard maps to the Czech layout (type `demo!@#$` for `demo1234`). Cost: native-only behaviour is checked only where the simulator could be unlocked.
26. **The demo profile is re-copied from `feat+mobile/demo-data`**: the merged build refused the old copy as pre-release. Cost: the container's earlier demo copy is overwritten.

Follow-ups, one line each:

- Drilldown: prefetch the account's complete history, or show a pending state while it loads on the first tap.
- Move `upload_bytes` to `services::attachments`.
- Strengthen `src/mobile/permission-strings.test.ts`: the build file inside the Resources phase, the ten variant children, the entry under `sources:`.
- Have the eight non-en/cs permission texts reviewed natively.
- `src/mobile/lib/watch-detail.ts:35` is the documented exception to the `fmt.money(` grep (a watched ticker's public price).
- Lock page: the revealed password input should not autocapitalise or autocorrect.
- Check on a real device whether rapid taps on the period ‹ zoom the page (pinch zoom is on since 6a).
- Demo profile: the two properties have no purchase date and no valuations, so a history recalculation leaves real estate out of past net worth.
- Deferred minors, Task 1: the `notFound` fields of `usePosition`, `useCryptoPosition` and `useOtherAsset` are unread; the three pages are not keyed by id; the pages create unused rename and follow observers through `usePositionMutations`, and `PositionMenu` an unused remove observer.
- Deferred minors, Task 2: `startRevalueForm` writes `String(value)`, which gives `1e-7` for a tiny per-unit price (`parseAmountInput` rejects it); the hint choice is untested inside the component (a pure `revalueHint` would pin it); the README Button pair row says "the property's revaluation sheet" (say "the shared revaluation sheet"); one `RevalueSheet` prop JSDoc line is over 100 columns.
- Deferred minors, Task 3: the `DateChip` caption and label prop docs still name a distance caption and only a met target; `AgendaRow` computes the relative label and distance for the two "today" kinds and discards them; the README Date chip row overgeneralises (loan events show the year, no distance); README line ~452 and `mobile.html:125` still use "noch 3 Tage" as the agenda note specimen.
- Deferred minors, Task 4: the `type-roles.test.ts` gate misses relative specifiers, a `.tsx` suffix and a backtick `import()` (none today); the `Scaled` import sits in the `@/components` block in about eleven files; `ScaledConversionNote` repeats the desktop's `mb-5` default.
- Deferred minors, Task 6: no raw-body test exactly at the limit; the desktop photo batch silently skips a photo over 100 MB among valid ones (older behaviour, newly reachable).
- Deferred minors, Task 7a: a slow failed read toasts after the person left (the opening ref outlives the page); a slow save's `onClose` can close another row's newly opened picker; bulk-moved and restored rows keep their old recorded flow (only the picked row is re-read); the effective-flow key and `queryFn` are written twice (one `effectiveFlowQuery(id)` helper); `if (!pickFor) return` could key on the retained `pickerRow`; README §13 does not list `mobile/components/cashflow/DrilldownRow.tsx`.
- Deferred minors, Task 7b: the README Segment row's file column does not list `cashflow/PeriodBar.tsx`; `PeriodBar` builds its options array on every render.
- Deferred minors, Task 7c: the category label building is duplicated between `exportReport` and `ReportBody` (a shared `useCategoryTitle` would pin "labels equal rows").
