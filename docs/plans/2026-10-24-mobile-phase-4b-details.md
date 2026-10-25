# Mobile Phase 4b — loan, policy and property details Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the phone's loan, policy and property placeholders with the three details of spec §8.8 (Úvěr, Pojistka, Nemovitost), add the insurance list behaviours §8.4/§8.8 describe that the Finance tab lacks, and build the documents section with camera capture as a shared phone component.

**Architecture:** Every screen sits on the desktop's existing queries, keys, mutations and pure maths (`useLoans`, `loanTrajectory`, `extraPaymentWhatIf`, the insurance and real-estate hooks, the encrypted-attachment commands); the phone adds pure view-model functions in `src/mobile/lib/` (tested), components in `src/mobile/components/{loan,policy,property,documents}/`, sheets in `src/mobile/sheets/`, and three pages. Long edit forms stay the desktop dialogs as in Phases 2a/4a; the what-if, the revaluation, the confirmations and the documents are phone sheets. No Rust unless a task below says a phone need is missing.

**Tech Stack:** React 19 + TypeScript, wouter, TanStack Query, Recharts (through `ScrubChart`), Tailwind on the shared tokens, react-i18next (10 locales), Vitest.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` (§8.4, §8.8, §10; binding) with the prototype `docs/specs/2026-10-16-mobile-app-prototype.html` (frame 13 Úvěr + note 18, frame 14 Pojistka + note 19, frame 10 Majetek). Where the prototype and the spec differ the spec wins (R7): Úvěr's fourth stat is **Konec fixace** and Příští splátka sits in the hero note; Pojistka's premiums chart is the **last** section, after Poznámky.

## Global Constraints

Process (from the parent task, 2026-10-24):

- Worktree `.claude/worktrees/mobile-details-8cbcbf`, branch `claude/mobile-details-8cbcbf`, cut from `feat/mobile` d0128630. `git branch --show-current` before every commit. Never push; never touch `main`, `feat/mobile` or another worktree; the parent session merges this branch.
- One plain shell command per call (the sandbox refuses `&&` / `;` chains except one `cd src-tauri && cargo …`, `git -C`, `HOME=` prefixes, scripts); stage explicit paths only; never `git stash`; never commit `package-lock.json` churn from `npm install`.
- Reuse only, never edit: `src/components/**` (desktop), `src/hooks/**`, `src/utils/**`, `src/lib/money-flow-refresh.ts`, `shared/**`. Shared phone files are touched minimally and additively: `src/mobile/shell/routes.ts` (nothing to add: the three detail routes exist), `src/mobile/shell/MobileShell.tsx` (three route swaps), the ten `src/i18n/locales/*/mobile.json` (new blocks only: `loan`, `policy`, `property`, `documents`, and keys under `finance.insurance…`), `docs/design-system/README.md` §13 (new rows), `docs/design-system/mobile.css` / `mobile.html` (new blocks / frames only). Sibling branches edit the same files (crypto/bonds/other assets/watchlist; dark mode + Dynamic Type).
- Additive changes to Phase 1 components are allowed where a task names them (`Hero`, `ScrubChart`); every existing caller keeps compiling and looking the same.
- Rust only where a phone need is genuinely missing, then `docs/standards/rust-backend.md`, service tests, `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test`, `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`; no blanket `cargo update`.
- `invoke()` only in `src/lib/tauri-api.ts`.
- Every user-facing string in all ten locales (en, cs, de, es, it, nb, pl, sv, ja, zh) in the same commit, phone strings in the `mobile` namespace; desktop strings are reused, not copied (`loans:*`, `insurance:*`, `realEstate:*`, `common:*`). Plural forms per `docs/standards/i18n.md` §2 (en/de/nb/sv `_one _other`; es/it `_one _many _other`; cs `_one _few _other`; pl `_one _few _many _other`; ja/zh `_other`). `src/i18n/locales.test.ts` must pass.
- Gate per task before committing: `npm run format`, then `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Conventional Commit, trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>` (Claude Sonnet 5.5 / Claude Opus 5.5 / Claude Haiku 5.5).
- Subagents: implementers `sonnet`, translations `haiku`, task reviews `sonnet`, review of the documents/attachments task (bytes, file input, viewer) `opus`, final whole-branch review `opus`; always an explicit `model`, never Fable. Ledger `.superpowers/sdd/2026-10-24-mobile-phase-4b-details/progress.md`.
- Simulator (controller only, never an implementer): iPhone 17, UDID `9B31123B-089C-451D-858B-587ACE9BEB2D`, Vite on port 1440, addressed by UDID, never `booted`. Demo profile (Alex Novák, `demo1234`) copied from `…/worktrees/feat+mobile/demo-data/mobile-demo-profile`.

From the spec and the phone contract:

- **Hide amounts** (D6, README §13): every amount on the three details, their sheets and the insurance list goes through `useMoneyText()` / `<Money>` / `maskMoney` / the masking of `ScrubChart`; counts, percents, dates and the policy number stay. Acceptance per task: `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only `src/mobile/hooks/use-money-text.ts`.
- **A hand-entered balance holds exactly until changed** (owner rule): the loan's balance, chart and stats come from the ledger walk (`row.trajectory`, `loanBalanceAt`), which stops amortizing after a `balance_check`; the phone never estimates over it and says so where the desktop does (`useLoanValuationNote`).
- Money is TEXT on the wire; every amount is shown in its own currency (the loan's, the policy's, the property's); totals in the main currency only through the desktop hooks' conversions, a missing rate shown as "—", never 1:1 (ADR 0012).
- Phone contract: `Screen` with the back button named after the parent tab; `Sheet` + `useSheetHistory` (stacked; Android back closes the newest); `ConfirmSheet` for every destructive action; `EntityRow` rules (chevron only without a trailing value; whole row tappable; long press = row menu via `RowMenu`); 44 pt targets; `text-m-*` roles; Lucide 24/1.75 in bars, 18 in menus; **tokens only** (no literal colour anywhere — the dark-mode sibling switches the phone to dark); a new component gets its `mobile.css` block, a `mobile.html` demo (no horizontal overflow at 360) and a README §13 row in the same commit.
- Mutations go through the desktop mutation hooks, which already invalidate the domain keys + `["portfolio-metrics"]` + `["cashflow-report"]` and call `recordSnapshot()` + invalidate `["portfolio-history"]` (AGENTS.md rule 7); a phone write that has no desktop hook follows `use-loan-mutations.ts`'s `useLoanRefresh` shape.
- Deleting the record a detail shows leaves like `PositionDetail` (`leaveAfterSheet` → `useBack`, a 250 ms guard that replaces a restored deep link with the tab root, then `removeQueries` of the record's own keys): no ghost of it in the history.
- Privacy (§4, §9): no new network request. Photos and documents go to the backend as raw bytes (the desktop's path); no plaintext copy is written by the phone code.

## Review Focus

Inputs the spec implies and no task's feature tests exercise directly; each line's test is pinned to the owning task.

1. **A loan with a hand-entered balance** (a `balance_check` newer than the last payment): hero, chart (flat after the check), stats and the what-if all start from that balance and nothing amortizes over it until a later event. Task 1: `loanDetailStats` and `loanWindow` tests with a manual-ledger fixture (balance unchanged across three due days after the check).
2. **A loan without a payment or whose payment is below the interest** (`row.mode === 'static'`, `belowInterest`): the inline alert shows, Mimořádná splátka is disabled with the reason, the what-if never divides by zero and the schedule list says it is empty instead of rendering nothing. Task 1: `loanAlerts` test for `noPayment`, `paymentBelowInterest`, `balanceAtEndDate`, `scheduleTruncated` and the fixation-ending alert; `extraPaymentText` test for a `null` what-if.
3. **A policy in a foreign currency or without a frequency / next payment / end date** (one-off premium, ended policy): the key-value card leaves the row out (never "undefined", never 1:1 into the main currency) and the "Nejbližší platby" list skips it. Task 2: `policyFacts` test; Task 3: `upcomingPremiums` test (ended, one-off, foreign currency, a payment exactly today).
4. **A document or photo the viewer cannot show** (HEIC from the photo library, a 30 MB PDF, a file with no extension, a picker cancelled): the phone says so plainly (the desktop's validation key), uploads nothing, and the section stays usable; a cancelled capture uploads nothing. Task 3: `pickedFileProblem` / `documentNameFrom` tests (heic/heif refused, pdf/jpg/png/webp/docx accepted, no extension, upper-case extension, empty file, over 100 MB); Task 2: `byte_document_rejects_empty_bytes` and `document_len_limit`.
5. **Hide amounts on across all three details and their sheets** (hero, scrub ticks, stats, schedule, events, what-if result, limits, premiums chart, yearly balance, costs, valuation history): every amount masked, counts/percents/dates/policy number kept. Tasks 1, 2, 4: the reviewer runs the `fmt.money(` grep and rejects any unmasked amount; the controller takes one screenshot per detail with the eye on.

---

## Task 1: Úvěr detail (`/finance/loans/:id`, frame 13)

**Files:**
- Create: `src/mobile/lib/loan-detail.ts`, `src/mobile/lib/loan-detail.test.ts`
- Modify: `src/mobile/lib/scrub.ts`, `src/mobile/lib/scrub.test.ts` (add `splitProjected`)
- Modify (additive): `src/mobile/components/ScrubChart.tsx` (`projectedFrom`), `src/mobile/components/Hero.tsx` (period type parameter + `periodLabel`, chart without a segment, `projectedFrom` pass-through)
- Create: `src/mobile/components/DateChip.tsx` (the 54 × 42 date chip of `AgendaRow`, as its own component; `AgendaRow` is left as it is)
- Create: `src/mobile/components/loan/{LoanHeroCard,LoanAlerts,LoanStats,LoanSchedule,LoanEvents,LoanMenu}.tsx`
- Create: `src/mobile/sheets/ExtraPaymentSheet.tsx`
- Create: `src/mobile/pages/finance/LoanDetail.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (route `routes.loanDetail` → `LoanDetail`), `src/mobile/pages/finance/FinancePlaceholder.tsx` (drop `LoanPlaceholder`)
- Modify: ten `src/i18n/locales/*/mobile.json` (block `loan`), `docs/design-system/{README.md,mobile.css,mobile.html}`

**Interfaces:**
- Consumes (desktop, unchanged): `useLoans()` → `{ rows: LoanRow[], today, isLoading }` (`src/hooks/use-loans.ts`; `LoanRow` in `src/utils/loans.ts`: `loan, ledger, plan, trajectory, mode, belowInterest, currency, outstanding, principal, payment, repaidShare, matured, payoffDay, fixationEnd`); `useLoanSchedule(id)` → `LoanSchedule` (`warnings`, `paymentsRemaining`); `useLoanEvents(id)` → `LoanEvent[]`; `useLoanMutations()` → `{ updateMutation, deleteMutation }`; `useLoanEventMutations(id)` → `{ remove }`; `useLoanText()` → `{ monthYear, months, between }`; `useLoanValuationNote(loan)` → `{ text, mode, warn }`; `extraPaymentWhatIf`, `suggestedExtraPayment`, `whatIfStep`, `clipSeries`, `stepValueAt` (`src/utils/loan-trajectory.ts`); `dueDay` (`shared/calculations/loan-amortization.ts`); `annuityPayment`; dialogs `LoanFormDialog` (`{ open, onOpenChange, onSubmit, loan, isLoading }`) and `LoanEventDialog` (`{ open, onOpenChange, loan, ledger, plan, today, kind, initial? }`).
- Produces (later tasks rely on): `DateChip({ day: number; caption: string; urgent?: boolean; label?: string })`; `Hero` accepts `periodLabel?: (p: P) => string` for a non-`ChartPeriod` segment, renders the chart whenever `points` is given (the segment only when `period`, `periods` with ≥ 2 entries and `onPeriodChange` are given), and passes `projectedFrom?: number` to `ScrubChart`.

- [ ] **Step 1: Write the failing tests** in `src/mobile/lib/loan-detail.test.ts` and `src/mobile/lib/scrub.test.ts`. Fixtures are built with `loanRow(loan, today, convert)` from wire `Loan` objects (as `src/utils/loans.test.ts` does): an automatic loan (3 100 000, 1,9 %, 30 years, started 15. 5. 2019), the same with a `balance_check` of 2 400 000 on 3. 3. 2026, one with `monthlyPayment` "0", one whose payment is below the interest, one with `interestRateValidityDate` 10 days after `today`.
  - `splitProjected(points, projectedFrom)`: without `projectedFrom` every point has `actual === value` and no `projected`; with it, points at or before carry `actual`, at or after carry `projected`, and a point is inserted at `projectedFrom` (step value of the last point before it) when none sits there, so both lines meet; `projectedFrom` outside the range adds nothing.
  - `loanWindow(trajectory, today, 'past' | '5' | 'all')`: `'past'` ends at `today`; `'5'` starts at `max(start, dueDay(today, -12))` and ends at `dueDay(today, 60)`; `'all'` ends at `max(payoffDay ?? last, today + 86 400)` — the desktop's windows (`LoanDetail.tsx` 289–305); the manual fixture's points are flat from 3. 3. 2026 to `today`.
  - `loanChartEvents(row, events)`: one `drawdown` (`type 'buy'`, at `startDate`), one per `extra_payment` (`'income'`), one per `rate_change` and `balance_check` (`'mark'`), plus `fixationEnd`, `half`, `payoff` marks when the row has them (`'mark'`) — ids stable (`drawdown`, the event id, `fixation`, `half`, `payoff`).
  - `loanDetailStats(row, events, schedule)` returns `{ balance, interestPaid, principalRepaid, repaidShare, extraPaid, remaining, payoffDay, endBalance, fixationEnd, next }` with the desktop's rules (`LoanDetail.tsx` 400–415): `interestPaid = trajectory.interestPaid`, `principalRepaid = max(0, principal − balance)`, `remaining = schedule.paymentsRemaining ?? (endBalance !== null ? upcoming.length : null)`, `next = trajectory.upcoming[0] ?? null`; the manual fixture's `balance` is exactly 2 400 000 minus the payments due after 3. 3. 2026 per the walk (assert against `loanBalanceAt(ledger, today).balance`).
  - `loanAlerts(row, schedule, today)` returns the ordered list of `{ kind: 'noPayment' | 'paymentBelowInterest' | 'balanceAtEndDate' | 'scheduleTruncated' | 'fixationSoon' | 'fixationPassed'; … }`: the schedule's warning codes (and `noPayment` also for `row.mode === 'static'`, as `LoanWarnings` does), `fixationSoon` with `days` when `interestRateValidityDate` is 0–30 days ahead, `fixationPassed` when it is up to 60 days past and no `rate_change` is dated on or after it.
  - `scheduleYears(rows)` groups the upcoming rows per UTC calendar year with `{ year, count, payments, interest, principal, balanceEnd }` (the desktop table's grouping, `LoanScheduleTable.tsx` 55–77).
  - `extraPaymentText(whatIf)` returns `null` for `null`, `{ kind: 'term', months: monthsBefore − monthsAfter, interestSaved }` or `{ kind: 'payment', drop: plan payment − whatIf.payment, interestSaved }`, `{ kind: 'noEffect' }` when nothing changes.

- [ ] **Step 2: Run them** — `npx vitest run src/mobile/lib/loan-detail.test.ts src/mobile/lib/scrub.test.ts` — expected FAIL (functions not defined).

- [ ] **Step 3: Implement** `splitProjected` in `src/mobile/lib/scrub.ts` and `loanWindow`, `loanChartEvents`, `loanDetailStats`, `loanAlerts`, `scheduleYears`, `extraPaymentText` in `src/mobile/lib/loan-detail.ts` (types `LoanHorizon = 'past' | '5' | 'all'`, `LoanChartEvent extends ChartEvent { kind: 'drawdown' | 'extraPayment' | 'rateChange' | 'balanceCheck' | 'fixationEnd' | 'half' | 'payoff'; event?: LoanEvent }` — titles are rendered by the page with `loans:detail.chart.events.*`). Run the tests: PASS.

- [ ] **Step 4: Chart and hero (additive).** `ScrubChart` takes `projectedFrom?: number`: the data go through `splitProjected`; area and solid line draw `actual`, a dashed line (`chart-line` token, 1.5 px, dash 4 4, no area) draws `projected`, both with the same `step` type; the ring sits on the last actual point; the value domain and the scrub still use `value` (scrubbing the plan shows the planned balance). `Hero` becomes `Hero<E, P extends string = ChartPeriod>` with `periodLabel?: (p: P) => string` (default `common:periodsLong.<p>`), draws the chart whenever `points` is given, draws the segment only with the period trio and ≥ 2 periods, and passes `projectedFrom`. Every existing `Hero` / `ScrubChart` caller compiles unchanged.

- [ ] **Step 5: The screen.** `LoanDetail` (`useRoute(routes.loanDetail)`, back `useBack(routes.finance)` labelled `tabs.finance`):
  - Title = loan name; first body line (caption, centred, as `PositionDetail`) "CZK · 3,1 % · fixace do 20. 10. 2026" (currency · rate · `loans:detail.withFixation`-style fixation when `fixationEnd`).
  - `LoanHeroCard`: label `loans:detail.hero.outstanding`, value = `stats.balance` (masked), **note** = "Příští splátka 12 340 Kč · 20. 10." (`mobile:loan.nextPayment`) or the desktop's `loans:detail.hero.paidOff` / `noPayment`; segment Dosud · 5 let · Do splacení (`loans:detail.horizon.*`, default `'5'`); chart = `loanWindow` points, `step`, `zeroBaseline`, `projectedFrom = today`, marks = `loanChartEvents` with callouts from `loans:detail.chart.events.*`; legend under the chart (Skutečnost line, Plán splácení dash, mimořádná splátka ○, `loans:detail.chart.legend.*`) on the Majetek `.m-legend` pattern.
  - `LoanAlerts` under the hero: one inline alert per `loanAlerts` entry with the desktop's `loans:detail.warnings.*` text; the fixation alerts carry the action "Zapsat změnu sazby" (opens the rate-change dialog); when `useLoanValuationNote(loan).mode === 'manual'` one muted alert with its text ("zůstatek zadaný ručně … drží").
  - `LoanStats`: two `StatRow`s of two — Zaplacené úroky (note "od 5/2019"), Splaceno z jistiny (note "21 % · mimořádně 50 000 Kč"), Zbývá splátek (count; note "do března 2050" / `remainingToEnd` / `paidOff`), Konec fixace (month-year; note `loans:detail.stats.fixationNote` or "bez fixace").
  - Primary full-width button **Mimořádná splátka** (`loans:actions.extraPayment`), disabled with the reason as its hint when `row.mode === 'static'` or `row.outstanding <= 0`.
  - `LoanSchedule`: section head "Splátky" (`loans:detail.schedule.title`); `Segment` Příštích 12 · Po letech (`loans:detail.schedule.modes.next12|years`); rows are `EntityRow`s without a chip: date / "úrok X · jistina Y" / payment / "zbývá X"; years: year / "N splátek · úrok X" / paid in the year / "zbývá X"; the next-12 list folds after 4 behind `MoreRow` "Zobrazit všech 12"; empty → `loans:detail.schedule.empty`.
  - `LoanEvents`: section head `loans:detail.events.title` with the count; rows newest first with a `DateChip` (day + year), title/sub from `loans:detail.events.*`, the drawdown last; long press → `RowMenu` Smazat (not on the drawdown) → `ConfirmSheet` with `loans:detail.events.confirmDelete.*` (`descriptionManual` for a balance check) → `useLoanEventMutations(id).remove`.
  - `LoanMenu` (`···` trailing): Upravit → `LoanFormDialog` + `updateMutation`; Zadat zůstatek k datu / Zapsat změnu sazby → `LoanEventDialog` (`balance_check` / `rate_change`); Smazat → `ConfirmSheet` (`loans:confirmDelete.*`) → `deleteMutation` → leave as `PositionDetail` does, removing `['loan-schedule', id]` and `['loan-events', id]`.
  - Loading → skeleton (hero, 2 × 2 stats, rows); unknown id → `loans:detail.notFound`.

- [ ] **Step 6: `ExtraPaymentSheet`** (`Sheet`, half height): `AmountField` (unit = loan currency, initial `suggestedExtraPayment(balance)`), `Segment` Zkrátit dobu · Snížit splátku (`loans:detail.whatIf.modes.*`), the live result from `extraPaymentWhatIf(row.plan, balance, today, amount, mode)` worded with `loans:detail.whatIf.termResult` / `paymentResult` / `noEffect` / `noPayment` and its note, all amounts masked; footer Zrušit + **Zapsat jako provedenou** (`loans:detail.whatIf.record`), which closes the sheet and then opens `LoanEventDialog` kind `extra_payment` with `initial = { amount, mode }` — the desktop's record path (date and note chosen there).

- [ ] **Step 7: Strings, design system, gates, commit.** `mobile.loan.*` in all ten locales (only what the desktop namespaces lack: the hero note, the caption line, "bez fixace", the manual-balance alert wording if the desktop's does not fit, menu aria label); README §13 rows for Loan detail, Date chip, Extra-payment sheet and the `Hero`/`ScrubChart` additions; `mobile.css` blocks `.m-datechip`, `.m-alert` (if missing), `.m-chart__projected`; a `loan-frame` in `mobile.html` (no overflow at 360). Run the gates and the `fmt.money(` grep; commit `feat(mobile): loan detail with the plan line, the extra-payment sheet, the schedule and events`.

---

## Task 2: Documents from bytes — two commands, wrappers, camera permission (Rust) — reviewer `opus`

A photo of a contract taken in the phone exists only as bytes in the webview: the webview has no file-system access (`fs:deny-default`), and writing a plaintext temp file is what the plaintext-leaks fix removed. Today only photos accept bytes (`add_photo_to_batch`); documents take a path. This task adds the byte twin of each document command; nothing else changes and the desktop keeps its path commands.

**Files:**
- Modify: `src-tauri/src/services/attachments.rs` (byte variant of `store_document` + two public functions + tests)
- Modify: `src-tauri/src/commands/insurance.rs`, `src-tauri/src/commands/real_estate.rs` (one command each), `src-tauri/src/lib.rs` (register; and `build.rs` if it lists app commands)
- Modify: `src/lib/tauri-api.ts` (`insuranceApi.addDocumentBytes`, `realEstateApi.addDocumentBytes`)
- Modify: `src-tauri/Info.ios.plist` (`NSCameraUsageDescription`: "Moony photographs documents and your property; the photos are stored encrypted on this phone."), and the generated `src-tauri/gen/apple/**/Info.plist` the same way if that file is tracked (Phase 2b regenerated it)

**Interfaces:**
- Produces: `#[tauri::command] add_insurance_document_bytes(db, request: tauri::ipc::Request) -> Result<InsuranceDocument>` and `add_real_estate_document_bytes(db, request) -> Result<RealEstateDocument>`. The IPC body is the file's bytes (raw, with `photo_bytes`'s JSON-array fallback); headers: `owner-id` (policy / property id), `file-name` (`encodeURIComponent` of the picked file's name, used only for the MIME sniff and the extension; reduced to its base name), `document` (`encodeURIComponent(JSON.stringify(InsertInsuranceDocument | InsertRealEstateDocument))` — the existing types, validated with their existing `validate()`).
- Produces (TS): `insuranceApi.addDocumentBytes(insuranceId: string, bytes: Uint8Array, fileName: string, data: { name: string; description?: string; fileType?: string }): Promise<InsuranceDocument>` and `realEstateApi.addDocumentBytes(realEstateId, bytes, fileName, data): Promise<RealEstateDocument>`, built like `addPhotoToBatch` (`tauriInvoke(cmd, bytes, { headers })`). No new wire type.

- [ ] **Step 1: Failing service tests** in `services/attachments.rs` (in-memory schema with the two document tables, their owner tables and the blob tables the existing tests use): `byte_document_is_stored_with_sniffed_mime` (a `%PDF-` head → `application/pdf`, extension `pdf`, `file_size` = byte length, blob readable back equal), `byte_document_name_with_path_is_reduced_to_base_name` (`../../x/smlouva.pdf` → original name `smlouva.pdf`), `byte_document_rejects_empty_bytes` (`validation.fileRequired`), `document_len_limit` (a pure `check_document_len(len: u64) -> Result<()>`: `MAX_DOCUMENT_BYTES` ok, `+1` → `validation.attachmentTooLarge`), `byte_document_for_unknown_owner_is_not_found` (no row, no blob left behind), and the real-estate twin of the first test.
- [ ] **Step 2: Run** `cd src-tauri && cargo test attachments` — FAIL (functions missing).
- [ ] **Step 3: Implement** `pub fn add_insurance_document_bytes(conn, insurance_id, file_name, bytes: &[u8], doc: &NewDocument, now) -> Result<InsuranceDocument>` and `add_real_estate_document_bytes(…) -> Result<RealEstateDocument>` over a shared `store_document_bytes` (one transaction, `attachment_store::put_bytes`, `sniff_mime(Some(name), head)`), the owner checked first; the commands decode the headers with `urlencoding::decode` (already a dependency), deserialize, `validate()`, then call the service — thin, `AppError` only, no `.unwrap()`. Run the tests: PASS.
- [ ] **Step 4: Wrappers and plist.** Add the two wrappers; add the camera usage description.
- [ ] **Step 5: Gates** — `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test`, `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. `git diff --stat src-tauri/Cargo.lock` is empty. Commit `feat(attachments): add documents from bytes for the phone camera`.

---

## Task 3: Shared phone documents — section, capture, add sheet, viewer — reviewer `opus`

**Files:**
- Create: `src/mobile/lib/documents.ts`, `src/mobile/lib/documents.test.ts`
- Create: `src/mobile/components/documents/{DocumentsSection,CaptureButtons,DocumentRow}.tsx`
- Create: `src/mobile/sheets/{AddDocumentSheet,DocumentViewerSheet}.tsx`
- Modify: ten `mobile.json` (block `documents`), `docs/design-system/{README.md,mobile.css,mobile.html}`

**Interfaces:**
- Consumes: Task 2's `addDocumentBytes` wrappers; `insuranceApi.getDocuments` / `deleteDocument`, `realEstateApi.getDocuments` / `deleteDocument` with the desktop keys `['insurance-documents', id]` / `['real-estate-documents', id]`; `attachmentUrl(kind, id)` (`src/utils/attachment-url.ts`), `viewerKind(mime)` (`src/utils/attachment-viewer.ts`), the desktop `PdfPages` (`{ url, onReady, onError }`, lazy) and `extensionLabel` if exported; `translateApiError`.
- Produces: `DocumentsSection({ kind: 'insurance-document' | 'real-estate-document'; ownerId: string; fileTypes: readonly string[]; typeLabel: (fileType: string) => string })` — the whole "Dokumenty" section (head with "N · šifrované", rows, Vyfotit / Vybrat soubor, the sheets); `CaptureButtons({ cameraLabel; pickLabel; pickAccept?: string; multiple?: boolean; disabled?: boolean; onFiles: (files: File[], source: 'camera' | 'picker') => void })` — two secondary buttons over two hidden inputs, the camera one exactly `<input type="file" accept="image/*" capture="environment">`; both inputs are reset after every change so the same file can be chosen twice. The gallery (Task 7) reuses `CaptureButtons`.

- [ ] **Step 1: Failing tests** in `documents.test.ts`: `pickedFileProblem({ name, type, size })` → `null` for pdf / jpeg / png / webp / a `.docx`; `'heic'` for `image/heic`, `image/heif` or a `.HEIC` name with an empty type; `'empty'` for size 0; `'tooLarge'` above 100 MB (104 857 600); `documentNameFrom(fileName: string, source: 'camera' | 'picker', photoName: string): string` → the base name without its extension for a picked file (`Smlouva 2024.pdf` → `Smlouva 2024`, `smlouva` → `smlouva`), `photoName` (the caller formats "Foto 24. 10. 2026" with `documents.photoName`) for a camera capture and for a picked name that is empty without its extension (`.pdf`), trimmed to 200 characters; `documentSub(doc, typeLabel, fmt)` parts: type label · size (`1,2 MB`, `830 kB`, none when `fileSize` is null) · upload day.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/documents.test.ts` — FAIL.
- [ ] **Step 3: Implement** `documents.ts` (`MAX_DOCUMENT_BYTES = 100 * 1024 * 1024`, HEIC by type `/^image\/hei[cf]$/` or name `/\.hei[cf]$/i`, as `src/utils/photo-files.ts`). PASS.
- [ ] **Step 4: Components and sheets.**
  - `DocumentRow`: `EntityRow` with a square chip (Lucide `FileText`, `Image` for `viewerKind === 'image'`), the name, `documentSub`, a chevron (no trailing value); tap → viewer; long press → `RowMenu` Smazat → `ConfirmSheet` (`insurance:documents.deleteConfirm.*` / `realEstate:documents.deleteConfirm.*` by kind) → delete + invalidate the list key + toast.
  - `AddDocumentSheet` (half sheet): opens with the chosen `File` and its source; Název (prefilled with `documentNameFrom`), Typ as `PickRow`s of `fileTypes`, footer Zrušit + Uložit; Uložit reads `file.arrayBuffer()` once, calls `addDocumentBytes`, invalidates the list key, toasts `…:documents.added` and closes; while pending the button says `insurance:documents.uploading`; an error stays in the sheet as the translated message. A file with a `pickedFileProblem` never opens the sheet: a toast says why (`realEstate:gallery.errors.heicUnsupported`, `common:validation.fileRequired`, `common:validation.attachmentTooLarge`).
  - `DocumentViewerSheet` (full sheet, title = the name): `image` → `<img>` on `attachmentUrl(kind, id)` (contained, pinch is the system's); `pdf` → `PdfPages` (lazy, a skeleton until `onReady`, `common:attachments.viewer.failed` on `onError`); `external` → one sentence (`mobile:documents.openOnComputer`). Footer: Zavřít only — no "open elsewhere" or "save copy" on the phone (`open_with_system` is desktop-only).
  - `DocumentsSection` composes them: `SectionHead` "Dokumenty" (`insurance:detail.documents`) with the note "2 · šifrované" (`mobile:documents.count`), rows newest first, an empty line `insurance:documents.noDocuments`, `CaptureButtons` with `mobile:documents.takePhoto` / `mobile:documents.chooseFile`.
- [ ] **Step 5: Strings, design system, gates, commit.** `mobile.documents.*` in ten locales (takePhoto "Vyfotit", chooseFile "Vybrat soubor", fromPhotos "Vybrat z fotek", photoName "Foto {{date}}", count "{{count}} · šifrované" with plurals, openOnComputer, saving); README §13 rows Documents section / Capture buttons / Document viewer; `mobile.css` `.m-docrow`, `.m-capture`; a `documents-frame` in `mobile.html`. Gates + grep; commit `feat(mobile): shared documents section with camera capture and the encrypted viewer`.

---

## Task 4: Pojistka detail (`/finance/policies/:id`, frame 14)

**Files:**
- Create: `src/mobile/lib/policy-detail.ts`, `src/mobile/lib/policy-detail.test.ts`
- Create: `src/mobile/components/policy/{PolicyFacts,PolicyLimits,PolicyPremiums,PolicyMenu}.tsx`
- Create: `src/mobile/pages/finance/PolicyDetail.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (route `routes.policyDetail` → `PolicyDetail`); delete `src/mobile/pages/finance/FinancePlaceholder.tsx` (both of its screens are gone after Tasks 1 and 4)
- Modify: ten `mobile.json` (block `policy`), `docs/design-system/{README.md,mobile.css,mobile.html}`

**Interfaces:**
- Consumes: `useInsurance()` → `{ policies, rows: InsuranceRow[], today }` (`src/hooks/use-insurance.ts`; `InsuranceRow`: `frequency, ended, premiumNative, yearlyNative, coverage, coverageKnown, nextPayment, anniversary, endDay, limitCount`); `useInsuranceMutations()` → `{ setStatus, remove }`; `nextAnniversary`, `limitsByValue`, `paidSeries` (`src/utils/insurance.ts`); `insuranceApi.getRealEstate(id)` under `['insurance-real-estate', id]`; `useAuth().user?.insuranceNoticeDays`; dialogs `InsuranceFormDialog` (`{ policy, open, onOpenChange }`, `key={policy.updatedAt}`) and `EndPolicyDialog` (`{ open, onOpenChange, policy, today, isPending, onConfirm(endDay) }`); Task 1's `Hero`/`ScrubChart` `projectedFrom`; Task 3's `DocumentsSection`.

- [ ] **Step 1: Failing tests** in `policy-detail.test.ts`: `noticeDeadline(policy, row, noticeDays, today)` → `nextAnniversary − days × 86 400` when that day is today or later; `null` for one-time, ended, `noticeDays` null or 0, or a deadline already past; `policyFacts(policy, row, property, notice, today)` → the ordered keys `number, provider, type, premium, nextPayment, term, notice, property`, each left out when unknown (no number, one-time premium shown as the one-time amount without frequency, no next payment for an ended policy, `term` = "od 9. 1. 2024 · neurčitá" or "9. 1. 2024 – 31. 12. 2027" plus the anniversary when open-ended, no `property` without a link); `premiumPoints(policy, today)` → `paidSeries` past + future as one series ending 12 payments ahead, `paidToDay` and `paymentsMade` passed through, an empty series for a policy that has not started.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** PASS.
- [ ] **Step 4: The screen** (`useBack(routes.finance)`): title = policy name; caption "Kooperativa · Majetkové · aktivní" (`insurance:types.*`, `insurance:detail.statusActive|statusEnded`); nav trailing **Upravit** (`NavTextButton`, opens `InsuranceFormDialog`) and `···` (`PolicyMenu`: Ukončit → `EndPolicyDialog` → `setStatus` inactive with the end day / Obnovit → `setStatus` active; Smazat → `ConfirmSheet` `insurance:confirmDelete.*` → `remove` → leave as `PositionDetail`, removing `['insurance-documents', id]` and `['insurance-real-estate', id]`).
  - `PolicyFacts`: one key-value card of `KeyValueRow`s in `policyFacts` order — Číslo smlouvy with a 44 pt copy button (`navigator.clipboard.writeText`, toast `mobile:policy.copied`, a failure toasts `common:status.error`); Pojistné "2 150 Kč · čtvrtletně" (masked amount); Příští platba "4. 11. 2026 · za 19 dní"; Konec smlouvy / výročí; Výpověď nejpozději (when `noticeDeadline`); Pojištěná nemovitost with a chevron → `propertyPath(id)`.
  - `PolicyLimits`: `SectionHead` "Limity krytí" (`insurance:detail.limits.title`) with the note "celkem 6,65 mil. Kč" from `row.coverage` (main currency; left out unless `coverageKnown`); rows title / amount in the limit's own currency, by value (`limitsByValue`); empty → `insurance:detail.limits.empty`.
  - `DocumentsSection kind="insurance-document"` with `fileTypes` contract, certificate, claim, other (`insurance:documents.types.*`).
  - Poznámky: a card with the notes as plain text (`whitespace-pre-wrap`), or `insurance:detail.notes.empty`; edited through Upravit.
  - `PolicyPremiums` last: section head "Zaplaceno na pojistném" with the note "od začátku"; a card on the `TrendCard` pattern without a segment — label "Celkem · 11 plateb", value `paidToDay` in the premium currency (masked), `ScrubChart` 120 pt, `step`, `projectedFrom = today`; nothing when the series has fewer than two points.
  - Loading skeleton; unknown id → `insurance:detail.notFound`.
- [ ] **Step 5: Strings, design system, gates, commit.** `mobile.policy.*` (copied, copy aria label, premium "{{amount}} · {{frequency}}", term lines, notice row label if the desktop lacks one, payments count label); README §13 row Policy detail (+ copy button); `policy-frame` in `mobile.html`. Commit `feat(mobile): policy detail facts first with limits, documents and the premiums chart`.

---

## Task 5: Insurance on the Finance tab — "Nejbližší platby" and the type filter

What exists (do not rebuild): the Pojištění section with rows sorted by the next payment, the yearly total in the head and the remembered "Zobrazit ukončené" toggle in `···`. Missing per §8.4/§8.8: the dated list "Nejbližší platby" and the type filter in `···`.

**Files:**
- Create: `src/mobile/lib/upcoming-premiums.ts`, `src/mobile/lib/upcoming-premiums.test.ts`
- Create: `src/mobile/components/finance/UpcomingPremiums.tsx`
- Modify: `src/mobile/pages/Finance.tsx` (the `···` radio group, the filtered rows, the new section after Pojištění)
- Modify: ten `mobile.json` (keys under `finance.insurance`), `docs/design-system/README.md` (row)

**Interfaces:**
- Consumes: `paymentDays(policy, from, to)`, `isEnded` (`src/utils/insurance.ts`), `InsuranceRow`; Task 1's `DateChip`; `MoreRow`; `useRemembered`.
- Produces: `upcomingPremiums(rows: readonly InsuranceRow[], today: number, days = 90): UpcomingPremium[]` with `{ policyId: string; name: string; provider: string; day: number; amount: number; currency: string }`, and `policyTypes(rows): string[]`.

- [ ] **Step 1: Failing tests**: payments from `today` (a payment due today is included) to `today + 90 days`, sorted by day then name; ended policies and one-time policies left out; each payment in the policy's own currency (`premiumNative`), never converted; a monthly policy yields three entries in 90 days; `policyTypes` returns the distinct types present in the order of `insurance:types` (life, health, property, vehicle, travel, liability, accident, other).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** PASS.
- [ ] **Step 4: Finance.** `···` gets a radio group "Typ pojištění" (`insurance:table.allTypes` + `insurance:types.*` of the present types; only when two or more types exist), remembered as `useRemembered('finance.policyType', 'all')`; the filter applies to the policy rows and the new list, not to the head's yearly total. `UpcomingPremiums` after the policy rows: `SectionHead` "Nejbližší platby" (`mobile:finance.insurance.upcoming`) with the note "90 dní"; rows with a `DateChip` (day + `relative`), the policy name, the insurer, the amount (masked); five shown, then `MoreRow` "Zobrazit všech N"; a tap opens the policy; hidden when empty or when every policy is ended.
- [ ] **Step 5: Strings, README row, gates, commit** `feat(mobile): nearest premiums and the policy type filter on Finance`.

---

## Task 6: Nemovitost detail — hero, stats, Přecenit, Finance, notes, menu (`/wealth/real-estate/:id`)

**Files:**
- Create: `src/mobile/lib/property-detail.ts`, `src/mobile/lib/property-detail.test.ts`
- Create: `src/mobile/hooks/use-property.ts`, `src/mobile/hooks/use-property-mutations.ts`
- Create: `src/mobile/components/property/{PropertyHeroCard,PropertyStats,PropertyFinance,YearlyBalanceCard,PropertyMenu}.tsx`
- Create: `src/mobile/sheets/{RevalueSheet,LinkSheet}.tsx`
- Create: `src/mobile/pages/PropertyDetail.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (route `routes.propertyDetail` → `PropertyDetail`; `WealthPlaceholder.tsx` stays untouched — its `PropertyPlaceholder` export becomes unused, the sibling branch rewrites that file)
- Modify: ten `mobile.json` (block `property`), `docs/design-system/{README.md,mobile.css,mobile.html}`

**Interfaces:**
- Consumes: `realEstateApi.get/getValuations/getLoans/getInsurances/delete/linkLoan/unlinkLoan/linkInsurance/unlinkInsurance`, `loansApi.getSchedule`, `loansApi.getAvailable`, `insuranceApi.getAvailable` with the desktop keys (`['real-estate', id]`, `['real-estate-valuations', id]`, `['real-estate-loans', id]`, `['real-estate-insurances', id]`, `['loan-schedule', loanId]`, `['available-loans']`, `['available-insurances']`); `propertyRow`, `yearlyBalance` (`src/utils/real-estate.ts`), `valuationTrace`, `convertValuations` (`src/utils/valuation-trace.ts`), `useMainConvert`, `useDatedMainConvert` (`src/hooks/use-fx.ts`), `useValuationMutations('realEstate', id)` (`src/hooks/use-valuation-mutations.ts`); dialogs `AddRealEstateModal` (`{ realEstate, open, onOpenChange }`) and `ValuationHistoryDialog` (`{ open, onOpenChange, kind: 'realEstate', assetId, valuations }`); Task 1's `Hero` chart without a segment; `ConfirmSheet`, `AmountField`, `PickRow`.
- Produces: `useProperty(id)` → `{ property, row: PropertyRow | null, balance: YearlyBalance | null, valuations, loans, policies, trace: { points, events }, missing: string[], isLoading, notFound }`; `usePropertyMutations(id)` → `{ remove, link, unlink }` (Task 7 adds the cost and photo mutations to the same hook).

- [ ] **Step 1: Failing tests** in `property-detail.test.ts`: `propertyTrace(valuations, purchase, now, nowValue, convertAt)` → the desktop's `valuationTrace` points plus events — the purchase as `'buy'` (only when the date, price and a rate are known and it precedes the first estimate), every valuation as `'mark'` with the first flagged, one point per day (newest wins); `propertyStatItems(row, balance)` → Hodnota (value + note "▲ 18 % vs. nákup" from the appreciation), Kapitál / LTV (equity + "LTV 45 %"; "bez úvěru" without loans; `null` value when a rate is missing — shown as "—"), Hrubý výnos (percent + the yearly rent note, or `realEstate:detail.stats.noRentNote`), Čistý cashflow (`balance.cashflow` per year with its tone; `null` → "—"); `linkCandidates(available, linked)` drops the already linked ones.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** The numbers are exactly the desktop detail's (`RealEstateDetail.tsx` 266–300 and the Roční bilance card), in the same currency. PASS.
- [ ] **Step 4: The screen** (`useBack(routes.wealth)`, labelled `tabs.wealth`): title = name; caption "Adresa · Investiční · pronajato" (`realEstate:types.*`, `realEstate:detail.rented|notRented`); `ConversionQualityNote` when `missing` is not empty.
  - `PropertyHeroCard`: label `realEstate:detail.heroValue`, the value, the change line vs. the purchase (`realEstate:detail.vsPurchasePct`), the trace chart without a segment (linear, marks with callouts: purchase "Koupeno za …", revaluation "Přeceněno na …" from `realEstate:detail.chart.*`), legend.
  - `PropertyStats`: two `StatRow`s of two (`propertyStatItems`).
  - Primary full-width **Přecenit** (`realEstate:detail.revalue`) → `RevalueSheet`: `AmountField` (unit = market currency, prefilled), Datum (date input, today, not in the future → `common:validation.valuationDateInFuture`), Poznámka (optional); primary "Přecenit" → `useValuationMutations('realEstate', id).add` with `{ assetId, value, currency, valuedAt: UTC day, note }`; closes on success.
  - `PropertyFinance`: `SectionHead` "Finance" (`realEstate:detail.tabs.financials`); linked loans as rows (name, "splátka X / měs.", outstanding) → `loanPath`, linked policies as rows (name, insurer, premium per period) → `policyPath`; long press → `RowMenu` Odpojit (`realEstate:detail.unlinkItem`) → unlink; two text actions "Připojit úvěr" / "Připojit pojistku" opening `LinkSheet` (title/description `realEstate:linkDialog.*`, `PickRow` per **unlinked** candidate, empty `realEstate:linkDialog.noLoans|noInsurances`; a pick links, closes, toasts `realEstate:toast.loanLinked|insuranceLinked`). Then `YearlyBalanceCard`: key-value card of `realEstate:detail.balance.*` rows (rent, recurring, insurance, interest, net income, principal, cashflow), amounts masked; `realEstate:detail.balance.empty` without data.
  - Poznámky card (read-only, edited through Upravit), `realEstate:detail.noNotes` when empty.
  - `PropertyMenu` (`···`): Upravit → `AddRealEstateModal`; Historie přecenění → `ValuationHistoryDialog` with the valuations; Smazat → `ConfirmSheet` (`realEstate:confirmDelete.*`) → `remove` (invalidates `['real-estate']`, `['real-estate', id]`, the valuation keys, `['portfolio-metrics']`, `['cashflow-report']`, then `recordSnapshot()` + `['portfolio-history']`) → leave as `PositionDetail` does, removing the property's own keys.
  - Loading skeleton; unknown id → `mobile:property.notFound`.
- [ ] **Step 5: Strings, design system, gates, commit** `feat(mobile): property detail with the valuation trace, revaluation, finance links and yearly balance`.

---

## Task 7: Nemovitost — Náklady, Galerie, Dokumenty

**Files:**
- Modify: `src/mobile/lib/property-detail.ts` (+ tests), `src/mobile/hooks/use-property-mutations.ts`, `src/mobile/pages/PropertyDetail.tsx`
- Create: `src/mobile/components/property/{PropertyCosts,PropertyGallery}.tsx`, `src/mobile/sheets/{CostSheet,PhotoViewerSheet}.tsx`
- Modify: ten `mobile.json`, `docs/design-system/{README.md,mobile.css,mobile.html}`

**Interfaces:**
- Consumes: `realEstateApi.getCosts/createCost/updateCost/deleteCost`, `realEstateApi.update` with `realEstateUpdatePayload(property, overrides)` (`src/utils/real-estate-payload.ts`) for recurring costs, `useRealEstateMutations().addRecurringCost`, `realEstateApi.getPhotoBatches/createPhotoBatch/addPhotoToBatch/deletePhoto/deletePhotoBatch` (keys `['real-estate-costs', id]`, `['real-estate-photo-batches', id]`), `splitPhotoFiles`, `PHOTO_ACCEPT` (`src/utils/photo-files.ts`), `attachmentUrl('photo-thumb' | 'photo', id)`, `yearlyFactor`; Task 3's `CaptureButtons` and `DocumentsSection`.

- [ ] **Step 1: Failing tests**: `todayBatch(batches, today)` → the batch whose `photoDate` is today's UTC day, else `null`; `costRows(property, costs, fmt)` → recurring rows (name, frequency, amount, per-year amount via `yearlyFactor`) and one-time rows newest first; `withoutRecurring(costs, index)` removes exactly one entry; `costInput(form, currency)` → `InsertRealEstateOneTimeCost` with `date` as a UTC-day number (never a `Date`) and the amount as a string.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** PASS.
- [ ] **Step 4: Sections** (after Finance, before Poznámky):
  - `PropertyCosts`: `SectionHead` "Náklady" (`realEstate:detail.tabs.costs`) with the note "ročně X" (`balance.recurring`, masked); two groups captioned `realEstate:detail.recurringCosts` / `oneTimeCosts`; rows name / frequency or date / amount; long press → `RowMenu`: one-time Upravit / Smazat, recurring Smazat; empty lines `realEstate:detail.noRecurringCosts|noOneTimeCosts`; a text action "Přidat náklad" → `CostSheet`: `Segment` Opakovaný · Jednorázový, Název, `AmountField` in the property's market currency, Frekvence segment měsíčně · čtvrtletně · ročně (recurring) or Datum + Popis (one-time); the same sheet edits a one-time cost. Every cost write invalidates `['real-estate-costs', id]`, `['real-estate']`, `['real-estate', id]`, `['portfolio-metrics']`, `['cashflow-report']`, then `recordSnapshot()` + `['portfolio-history']` (rule 7; the desktop's cost dialogs do less).
  - `PropertyGallery`: `SectionHead` "Galerie" (`realEstate:gallery.title`) with the photo count; a three-column grid of thumbnails (newest batch first; square, `object-cover`, radius per the contract); `CaptureButtons` labelled Vyfotit / Vybrat z fotek (`pickAccept = PHOTO_ACCEPT`, `multiple`); the files pass `splitPhotoFiles` (HEIC and unsupported → the desktop's `realEstate:gallery.errors.*` toasts), go into today's batch (created when missing, `photoDate` = today's UTC day) one by one through `addPhotoToBatch(batchId, new Uint8Array(await file.arrayBuffer()))`, with "Nahrávám 2 / 5" on the buttons; a tap on a thumbnail → `PhotoViewerSheet` (the full photo, its batch day and description, Smazat fotku → `ConfirmSheet` → `deletePhoto`, and `deletePhotoBatch` when it was the batch's last photo); empty → `realEstate:gallery.noPhotos`.
  - `DocumentsSection kind="real-estate-document"` with `fileTypes` deed, contract, appraisal, other (`realEstate:documents.types.*`).
- [ ] **Step 5: Strings, design system, gates, commit** `feat(mobile): property costs, gallery with the camera and documents`.

---

## Task 8: Translation pass — implementer `haiku`, reviewer `sonnet`

- [ ] Read every key this branch added to `src/i18n/locales/{en,cs}/mobile.json` (`git diff feat/mobile -- src/i18n/locales`) and the same keys in de, es, it, nb, pl, sv, ja, zh: replace English left in a foreign file, fix plural sets per `docs/standards/i18n.md` §2, keep `{{placeholders}}` identical, match the desktop's wording for the same idea (`loans`, `insurance`, `realEstate` namespaces of the same language). `npm test -- src/i18n` and the four gates; commit `chore(i18n): phase 4b phone strings in all ten languages`.

---

## Task 9: Simulator verification, screenshots, hand-over (controller)

- [ ] Demo profile into the iPhone 17 container, `tauri ios dev` on 1440; per detail: top, bottom, every sheet, the `···` menu, hide amounts on; Vybrat soubor / Vybrat z fotek end to end (a PDF and a JPEG pushed with `xcrun simctl addmedia` / Files); the camera input's attributes checked in code (the simulator has no camera feed). Screenshots `docs/screenshots/mobile/details-NN-*.png` via `xcrun simctl io 9B31123B-089C-451D-858B-587ACE9BEB2D screenshot`. Then the final whole-branch review (`opus`) against `feat/mobile`, one fix round, a re-review; this plan's "As built / rulings"; the Czech hand-over `.superpowers/sdd/2026-10-24-mobile-phase-4b-details/handover.md`.

---

## As built / rulings (2026-10-24)

Rulings taken while planning (each with what it costs if wrong):

1. **Spec over prototype** (R7): Úvěr's stats are Zaplacené úroky, Splaceno z jistiny, Zbývá splátek, Konec fixace, with Příští splátka in the hero note; the policy's next payment is a row of the key-value card (no separate payment card above it) and the premiums chart is the last section. Costs one card if the owner prefers frame 14's payment card.
2. **Loan hero segment Dosud · 5 let · Do splacení** (the desktop's horizons, default 5 let) — the spec names no segment, the prototype and the desktop have it. Costs one segment.
3. **`Hero` and `ScrubChart` grow additively**: a period type parameter with `periodLabel`, a chart without a segment (the property trace), `projectedFrom` (the plan line and the premiums ahead). Every existing caller is unchanged.
4. **Long forms stay the desktop dialogs**: Upravit (loan, policy, property), Zadat zůstatek k datu, Zapsat změnu sazby, Zapsat jako provedenou (the what-if sheet closes and opens the desktop extra-payment dialog prefilled, so the date and the note are chosen there), Ukončit pojistku, Historie přecenění. Phone sheets: the what-if, Přecenit, Připojit, the costs, the documents, every confirmation. Costs a sheet per dialog later.
5. **Fixation alerts** (prototype note 18): "Fixace končí za N dní" up to 30 days ahead, "Fixace skončila" up to 60 days after with no rate change since, both with "Zapsat změnu sazby". Costs one alert kind if unwanted.
6. **Documents travel as bytes** (Task 2): the camera gives bytes, the webview has no file system and a temp file would be a plaintext copy. The phone's "Vybrat soubor" uses the same byte path (`<input type="file">`) rather than the dialog plugin's path, so one upload path serves both. Costs two small commands.
7. **HEIC is refused on the phone** (owner ruling e32c9799); any other file up to 100 MB is stored; a type the phone cannot show (Word, …) opens to one sentence "otevřete v Moony na počítači" — no "open elsewhere" or "save copy" on the phone (`open_with_system` is desktop-only). Costs a share-sheet export later.
8. **Notes are read-only on the details** and edited through Upravit (both desktop forms carry them). Costs an inline editor later.
9. **The notice deadline is computed on the phone** (`nextAnniversary − insuranceNoticeDays`), the same inputs as the milestone. Costs nothing.
10. **"Nejbližší platby" covers 90 days**, five rows then "Zobrazit všech N"; the type filter filters rows and the list, never the head's yearly total.
11. **Připojit offers only unlinked loans and policies** (`get_available_*`): linking one that belongs to another property would silently move it. Costs a "move" option.
12. **Costs on the phone are in the property's market currency**; a cost in another currency is added on the desktop. Phone cost writes follow rule 7 in full.
13. **Photos of one day go into that day's batch**; deleting a batch's last photo deletes the batch.
14. **`WealthPlaceholder.tsx` is not edited** (the sibling branch rewrites it); its `PropertyPlaceholder` export is left unused for the parent's merge.
15. **The property detail shows the desktop detail's numbers in the desktop's currency** (`propertyRow`, `yearlyBalance`, `valuationTrace`).

Rulings taken while executing (each with what it costs if wrong; the ledger has the full trail):

16. **`documentNameFrom(fileName, source, photoName)`** takes the formatted photo name from the caller (a pure function cannot call `t()`). Costs nothing.
17. **A hand-entered loan balance holds exactly** until it is changed (owner rule): the manual-balance case reads exactly the entered amount, with the note that it holds. Costs nothing.
18. **The desktop dialogs the phone opens show their own previews unmasked** with the eye on (reuse-only, as in earlier phases). Costs a masked phone sheet per dialog if the eye should cover dialogs.
19. **Delete leaves from the page, not from the menu** (`use-delete-and-leave`): the delete is awaited on the screen, `history.back()` only while the screen is still the record's, a restored deep link is replaced by the tab root, a second tap does nothing. The loan, the policy and the property delete this way. Costs nothing; Phase 4c's `use-leave-deleted` has the shape that stranded the loan screen (follow-up).
20. **The owner-exists check runs on the new byte path only**; the desktop path command is unchanged (a desktop behaviour change is the owner's call). Costs one check later.
21. **Document zoom is a double-tap / button toggle 1× ↔ 2×** for images and PDF pages, the box scrolling both ways; the box has `touch-action: pan-x pan-y`, so a pinch over the document no longer zooms the page and the sheet (6a re-enabled page pinch). Costs pinch inside the viewer.
22. **"Vybrat soubor" filters to the desktop's document types** (pdf, doc, docx, jpg, jpeg, png, gif), so WebKit hands Photo Library HEIC originals over as JPEG instead of a refused HEIC. Costs other types (xlsx, txt…), which the desktop dialog does not offer either.
23. **The viewer of a document fills the sheet's remaining height** (one scroller at the largest text); the sheet footers use the shared `FOOTER_*` classes. A document the phone cannot show opens a normal-height sheet.
24. **The policy's Upravit is a full-width secondary button under the facts card**, not a nav button: the nav keeps back, title and `···` like the loan, so the policy name is readable at 402 pt. Costs one button row.
25. **The premiums series never adds a one-time amount in another currency 1:1**; a recurring policy's one-time amount is its own facts row (desktop `contract.oneTime`). The caption puts the status first, so truncation never hides "ukončeno".
26. **The upcoming-payments chip caption is the short weekday**; the distance ("za 73 dní") is in the row's sub-line. The shared relative labels overflow the 54 pt chip in most languages (measured); `DateChip` truncates as a last guard. Costs the prototype's relative caption in the chip.
27. **A stale remembered policy type falls back to "all"** (`effectiveType`).
28. **The purchase ▲ in the property hero follows the desktop rule** (purchase date, price and rate known), not "precedes the first estimate". Costs nothing.
29. **A valuation trace spanning fewer than two distinct days shows no chart**, only the one-estimate note. Costs nothing.
30. **`feat/mobile` was merged twice before Task 7** (8a109181 as Task M, d9b5c629 as Task M2), so the costs and the gallery were built under the dark-mode and Dynamic Type rules. Both sides kept in every conflict.
31. **Photo deletion resolves against the live batch** at confirm (a stale "last photo" flag never deletes photos the person did not pick); editing a cost while amounts are hidden keeps the stored amount unless the field is touched; a future-dated one-time cost keeps its own day as the field's maximum; Return in the cost name moves to the amount.
32. **sv keeps "fotoomgången"** (the sv desktop uses it for the same batch). Costs changing both together if ever.
33. **WebKit's plaintext upload copies are swept on the phone** (`services/upload_copies.rs`, final review): the `tmp/WKFileUploadPanel-*` folders WKWebView leaves after every pick or photo go at startup and at lock (`logout`), never after an upload (a second picked file may still wait in an open sheet). Verified on the simulator: 3 leftovers removed at start, 1 at lock. Costs a copy living until the next lock or launch.
34. **An ended fixation reads the desktop's three-way wording** ("fixace skončila …") in the loan caption and the Finance row, like the stat. Costs nothing.
35. **The contract number wraps at its spaces** (`overflow-wrap: break-word`, not `break-normal`): a long number without spaces may still break inside, rather than run under the copy button. Costs nothing.
36. **The policy's linked property is read fresh on every opening**; extra-payment and add-document sheets got their small races closed (hidden prefill empty, a late save no longer closes a newer sheet, Smazat fotku disabled while an upload runs, IME Return ignored).
