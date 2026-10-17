# Mobile Finance integration (Phase 2a into feat/mobile) Implementation Plan

Date 2026-10-17. Spec: `docs/specs/2026-10-16-mobile-app-design.md` (§8.4 Finance; §4 principle 7
and D6 hide amounts). Parent plans: `docs/plans/2026-10-16-mobile-app.md` (Phase 1, feat/mobile)
and `docs/plans/2026-10-16-mobile-app-finance.md` (Phase 2a, arrives with the merge in Task 1).
Hand-over note of Phase 2a: `/Users/filipkral/Library/Application Support/Claude/side-session-notes/local_5f063dc4-d8a4-4b43-816b-1067b3357f71/mobile-finance-2a.md`.

## Goal

Branch `claude/mobile-finance-integration-8da3a6` (cut from `feat/mobile` at 2d2c8d7b) carries the
Phase 2a Finance branch `claude/mobile-finance-2e89a2` (7 commits over 150765c2) merged with
`--no-ff`, with every money string on the Finance screens masked by the Přehled eye (hide amounts),
two cheap follow-ups from the hand-over note, green gates, simulator evidence and a clean final
review. The parent session merges this branch into `feat/mobile`; nobody pushes.

## Global Constraints

- Worktree `/Users/filipkral/Programování/FinanceApp/Moony-tauri/.claude/worktrees/mobile-finance-integration-8da3a6`,
  branch `claude/mobile-finance-integration-8da3a6`. `git branch --show-current` before every commit.
  Never push; never touch `main`, `feat/mobile`, `claude/mobile-finance-2e89a2` or other worktrees.
  Stage explicit paths only (`git add <path>`; never `git add -A` or `git add .`); never `git stash`.
- Shell: one plain command per call. The sandbox refuses `&&` / `;` chains, `git -C`, `HOME=`
  prefixes and ad-hoc scripts. Run each command on its own.
- No Rust, no app build: never run `cargo …`, `npm run tauri dev`, `npm run tauri ios dev` (disk is
  tight; every change here is frontend-only). If a backend command turns out to be missing, stop
  and report BLOCKED.
- Gate before every commit, each on its own call: `npm run lint`, `npm run typecheck`, `npm test`,
  `npm run format:check` (run `npm run format` first when it fails, then stage the formatted files).
  `src/i18n/locales.test.ts` (ten-locale parity) is part of `npm test`.
- `AGENTS.md` ten rules; `invoke()` only in `src/lib/tauri-api.ts`; user-facing strings only in the
  `mobile` namespace, every key in all ten locales (`en cs de es it nb pl sv ja zh`), plural forms per
  `docs/standards/i18n.md` (en/de/nb/sv `_one _other`; es/it `_one _many _other`; cs `_one _few _other`;
  pl `_one _few _many _other`; ja/zh `_other`).
- Design contract `docs/design-system/README.md` §13 and `docs/design-system/mobile.css`: tokens only
  (no raw hex), `text-m-*` type roles, touch targets ≥ 44 pt, EntityRow 60 pt two-line rule, chevron
  only on a row without a trailing value.
- Money only through `useFormat().money` — and on the phone only through the four hide-amounts
  routes documented in `src/mobile/components/Money.tsx`: `<Money>` (element), `useMoneyText()`
  (string, same signature as `fmt.money`, handed to shared hooks as their formatter),
  `ConvertedMoney` with its `format` prop, `maskMoney` / `MASK` for text not built by `fmt.money`.
  Inputs are never masked; counts, percents and dates stay visible. Acceptance for the pass:
  `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only
  `src/mobile/hooks/use-money-text.ts`.
- Commit: Conventional Commit (`feat(mobile): …`, `fix(mobile): …`, `refactor(mobile): …`), body in
  English, trailer `Co-Authored-By: <the model that wrote the code, as it names itself> <noreply@anthropic.com>`.
- Keep `AccountDetail.tsx` (~790 lines) and `Finance.tsx` (~606 lines) as they are: no big splits in
  this plan unless a review demands one.

## Review focus

1. A string key that both sides added to `mobile.json` with different values: the merge must keep
   one value per key, the same key set in all ten locales, no duplicate keys (Task 1; parity test).
2. The Finance hero, stat values and the trend callout size themselves to the shown text: with
   amounts hidden the text is the mask in full, never compact ("•••• Kč", not "•••• mil. Kč")
   (Task 2; `useMoneyText` already does this — the call sites must use it).
3. Signed amounts keep their sign and lose their digits: "+ •••• Kč", "− •••• Kč" (Task 2; the
   ledger row and the transaction sheet).
4. Accessible names that carry amounts (the flows strip's month buttons) mask too (Task 2).
5. Leaving Finance for an account detail and coming back keeps the sort, the paid-off/ended toggles
   and the trend period; relaunching the app resets them (Task 3).

## Task 1: Merge the Finance branch and resolve the conflicts

**Model:** standard (sonnet). **Files:** the merge touches ~90 files; hand-resolved ones are the ten
`src/i18n/locales/*/mobile.json`, `docs/design-system/README.md`, and the auto-merged shared files
listed below.

Steps:

1. `git branch --show-current` must print `claude/mobile-finance-integration-8da3a6`; `git status`
   must be clean; `git rev-parse HEAD` must be `2d2c8d7b…`.
2. `git merge --no-ff claude/mobile-finance-2e89a2`. It stops on conflicts. Expected textual
   conflicts: the ten `src/i18n/locales/<lng>/mobile.json` (both sides add keys — keep both sides'
   keys, valid JSON, no duplicate keys; a key added by both sides with different values: keep the
   feat/mobile value and report it) and `docs/design-system/README.md` §13 (keep both sides' rows and
   paragraphs; the Finance side adds component rows and the "settings-row exception" for the chevron,
   the feat/mobile side adds the hide-amounts / background-lock / biometrics material). Possibly also
   `src/mobile/components/Hero.tsx`, `StatRow.tsx`, `ScrubChart.tsx`, `RecordBalancesSheet.tsx`,
   `MobileShell.tsx` where both sides changed the same lines.
3. Resolve each conflict so both sides' behaviour survives, then `git add <path>` each resolved file
   (one path per call).
4. Semantic reconciliation of the shared files, whether git merged them by itself or not — read each
   after the merge:
   - `src/mobile/components/Hero.tsx`: feat/mobile changed `formatValue` to `(value: number) => string`
     (the shown text; the fit follows it; masked while hidden). The Finance side changed Hero too
     (its own additions stay). The string type wins; Finance call sites (`AccountDetail.tsx`) must
     pass string formatters (they do: `money0`).
   - `src/mobile/components/StatRow.tsx`: `StatItem.value: string` (feat/mobile) wins over
     `ReactNode` (Finance); the Finance side's other changes stay.
   - `src/mobile/components/ScrubChart.tsx`: feat/mobile added `useHideAmounts` and the `MASK` on
     the value-axis ticks; the Finance side added its own props. Both stay.
   - `src/mobile/sheets/RecordBalancesSheet.tsx`: feat/mobile masks the hint with `maskMoney`; the
     Finance side moved it to the stack `useSheetHistory` and changed its lead. Both stay.
   - `src/mobile/shell/MobileShell.tsx`: feat/mobile added `useBackgroundLock` with its cover and
     `clearEnrolmentAsked` on `needs_setup`; the Finance side added routes (account detail). Both stay.
   - `src/mobile/hooks/use-sheet-history.ts` + `src/mobile/lib/sheet-history.ts`: the Finance
     side's stack version is the only change since the base; it stays as is.
   - `src/mobile/pages/Overview.tsx`, `Lock.tsx`, `MobileApp.tsx`: feat/mobile only; unchanged by
     the merge.
5. Gates (Global Constraints). `npm run typecheck` is the semantic-conflict detector: fix every
   error it reports in the merged code (a Finance call site passing a `ReactNode` where a string is
   now required, a removed/renamed import, …). Fix nothing else.
6. Commit the merge with a message like
   `Merge branch 'claude/mobile-finance-2e89a2' into claude/mobile-finance-integration-8da3a6: Phase 2a Finance tab, account detail, transaction sheet and picker`
   and a body listing the conflicts and how each was resolved, plus the trailer. Put the semantic
   fixes from step 5 into the merge commit itself (one green commit); if they are more than a
   handful of lines, a second commit `fix(merge): …` right after is acceptable — say which in the
   report.
7. Report: every conflict and its resolution; every semantic fix with file:line; the four gate
   results (test count); `git log --oneline -3`.

Acceptance: `git log --merges -1` shows the merge; `git diff --stat claude/mobile-finance-2e89a2 HEAD -- src/mobile docs/design-system src/i18n/locales` shows only feat/mobile's material and the semantic fixes; all four gates green; `npm test` includes `src/i18n/locales.test.ts` passing.

## Task 2: Hide-amounts pass over the Finance screens

**Model:** standard (sonnet). **Files:** `src/mobile/pages/Finance.tsx`,
`src/mobile/pages/finance/AccountDetail.tsx`, `src/mobile/components/LedgerRow.tsx`,
`src/mobile/components/FlowStrip.tsx` (via its `formatValue` prop), `src/mobile/components/TrendCard.tsx`
(via its `formatValue` prop), `src/mobile/sheets/TransactionSheet.tsx`,
`src/mobile/sheets/BalanceHistorySheet.tsx`, `src/mobile/hooks/use-delete-description.ts`; read
`src/mobile/pages/Overview.tsx` first as the reference and `src/mobile/components/Money.tsx` for the
four routes. `CategoryPickerSheet.tsx` and `ImportHistorySheet.tsx` show counts and dates only —
verify, leave them.

The Finance branch was built before hide amounts existed, so its money strings come straight from
`useFormat().money` and ignore the eye. Route every one of them through the mask exactly as Přehled
does:

1. **Strings that size themselves or feed a prop typed `string`** (Hero `formatValue`, `StatItem.value`,
   `EntityRow` `value` / `sub` amounts, `SectionHead` numbers, FlowStrip's and TrendCard's
   `formatValue`, accessible names): `const moneyText = useMoneyText();` and call it with the same
   `(amount, currency, options)` the `fmt.money` call had. Keep the `{ decimals: 0 }` /
   `{ signed: true, decimals: 0 }` options as they are (`useMoneyText` drops `compact` while hidden by
   itself).
2. **Interpolated i18n amounts** (`t('…', { amount: fmt.money(…) })`): the same `moneyText`.
3. **Signed row amounts** built as `${credit ? '+ ' : '− '}${fmt.money(Math.abs(…), currency)}`
   (LedgerRow.tsx and TransactionSheet.tsx): keep the sign outside, mask the number —
   `` `${sign}${moneyText(Math.abs(…), currency)}` `` → "+ •••• Kč". The two sites compose the
   same text; you may share one small pure helper for the composition (e.g. the sign prefix from
   the credit flag) in `src/mobile/lib/` with a Vitest test — optional, only if it stays tiny.
4. **The delete confirmation** (`use-delete-description.ts`): its amount masks too (the eye covers
   every amount in the app; the date and counterparty still identify the row).
5. **Where an element will do** (a change line rendered as JSX, a stat note): `<Money value currency options sign>`.
6. **Charts**: `ScrubChart` already masks its value axis; TrendCard's scrub callout and the
   detail's balance chart get their masked text through the `formatValue` props you now pass.
7. **Never masked**: inputs (none on these screens today), counts (`fmt.number`, "3 platby"),
   percents (`fmt.percent` interest rates), dates, the uncategorized count.
8. Remove each `useFormat()` that no longer formats anything after the pass; keep it where it still
   formats days, months, numbers or percents.

Tests: `src/mobile/lib/hide-amounts.test.ts` already covers `maskMoney`; add a test only for a pure
helper you add. No component tests (policy).

Gates, then commit `feat(mobile): mask every amount on the Finance screens while amounts are hidden`.

Acceptance: `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only
`src/mobile/hooks/use-money-text.ts`; `grep -rn "useFormat" src/mobile/pages/Finance.tsx src/mobile/pages/finance src/mobile/sheets src/mobile/components src/mobile/hooks` shows only files that still format non-money values; typecheck, lint, test, format green.

## Task 3: Two cheap follow-ups from the Phase 2a hand-over note

**Model:** standard (sonnet). **Files:** `src/mobile/hooks/use-upkeep.ts`,
`src/mobile/hooks/use-finance.ts`, `src/mobile/pages/Finance.tsx`, plus one new shared hook file and
one new lib file with its test.

**3a. One source for the upkeep queries.** `useFinance` (Finance branch) duplicates two queries of
`useUpkeep` (Phase 1) verbatim: the per-account `['import-batches', account.id]` queries (with
`staleTime: 0`) reduced to `lastImportByAccount` (newest `importedAt` per account or `null`), and
the uncategorized query `['bank-transactions', 'uncategorized-all', currencyCode, uncategorizedEnd]`
(`cashflowApi.getTransactions(0, end, { categoryId: 'uncategorized', reportCurrency })` with
`uncategorizedEnd = utcDayFloor(Date.now() / 1000) + 86400 - 1`). Extract them into one shared hook
module (e.g. `src/mobile/hooks/use-account-upkeep.ts` exporting `useLastImports(accounts)` →
`{ lastImportByAccount, isPending }` and `useUncategorizedAll()` → the query result) with the
identical query keys and options (the desktop shares these cache entries), and make both
`useUpkeep` and `useFinance` consume it. No behaviour change: `isLoading` / `ready` semantics of
both hooks stay as they are. The reduction to `lastImportByAccount` is a pure function — put it in
`src/mobile/lib/` with a Vitest test (accounts without batches → `null`; the newest `importedAt`
wins).

**3b. Finance remembers its view state between visits.** `Finance.tsx` keeps `sort`,
`showMatured`, `showEnded` and the trend `period` in `useState`, so leaving for an account detail
and coming back (the route remounts) resets them. Ruling: remember them for the app session, in
memory, not in storage — a relaunch starts fresh. Add a tiny module-level store
`src/mobile/lib/view-memory.ts` (`createViewMemory()` → `{ get(key, initial), set(key, value) }`
or equivalent, pure, Vitest-tested: returns the initial value until set, then the set value; keys
are independent) and a hook `src/mobile/hooks/use-remembered.ts` — `useRemembered<T>(key: string, initial: T): [T, (next: T) => void]`
with the shape of `useState`, reading the store for its initial value and writing it on every set.
Replace the four `useState` calls in `Finance.tsx` with `useRemembered('finance.sort', 'balance')`
etc. (`addKind` stays `useState`: a dialog must not reopen on return). Overview's hero period is
out of scope.

Gates, then one commit per part, or one commit for both:
`refactor(mobile): share the upkeep queries between Údržba and Finance` and
`feat(mobile): Finance remembers its sort, toggles and trend period for the session`.

Acceptance: `grep -n "import-batches\|uncategorized-all" src/mobile/hooks/*.ts` shows each key in
exactly one hook file; new lib tests pass; typecheck, lint, test, format green.

## Task 4: Gate and simulator verification (run by the controller)

No code. The controller runs the four gates once more at the branch head, then verifies on the
simulator through the dev browser bridge:

1. Demo profile: terminate the app (`xcrun simctl terminate E3EF021E-F419-4E45-96E4-E305A5B1B326 com.moonyfinance.mobile`),
   find the container (`xcrun simctl get_app_container E3EF021E-F419-4E45-96E4-E305A5B1B326 com.moonyfinance.mobile data`),
   replace `<container>/Library/Application Support/com.moonyfinance.mobile` with a copy of
   `/private/tmp/claude-501/-Users-filipkral-Programov-n--FinanceApp-Moony-tauri/9ca3991e-1cce-4f7a-ac67-202f50de16cc/scratchpad/demo-profile`
   (Alex Novák, password `demo1234`; biometrics off → decline the enrolment sheet with "Teď ne").
2. `npm run dev -- --port 1430 --strictPort` in the background from this worktree; launch the app
   (`xcrun simctl launch E3EF021E-F419-4E45-96E4-E305A5B1B326 com.moonyfinance.mobile`); open
   `http://localhost:1430` in the browser pane (`preview_start` with `url:`, never `name:`),
   `resize_window` 390×844. Never `xcrun simctl … booted` (another session drives the iPhone 17).
3. Verify: Finance tab, account detail (hero, flows strip, ledger, balance history), transaction
   sheet, category picker render; toggle the eye on Přehled and confirm every amount on the Finance
   screens is masked ("•••• Kč", "+ •••• Kč"), counts and percents stay; leave Finance to a detail
   and back → sort/toggles/period kept.
4. Evidence: `xcrun simctl io E3EF021E-F419-4E45-96E4-E305A5B1B326 screenshot <path>` and pane
   screenshots into `.superpowers/sdd/2026-10-17-mobile-finance-integration/shots/`; two or three
   masked device screenshots committed as `docs/screenshots/mobile/finance-21-hidden-*.png`
   (`docs(mobile): hide-amounts verification screenshots`).
5. Stop Vite afterwards (`pkill -f "vite --port 1430"`).
