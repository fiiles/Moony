# Mobile Phase 6a — Dark mode, Dynamic Type and Vzhled on the phone Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The phone app renders light or dark from the shared Vzhled preference (Systém / Světlý /
Tmavý, the desktop's `UserProfile.theme`), switches at once from Nastavení → Vzhled, and scales its
text with the iOS text size (Dynamic Type) — without changing the desktop's behaviour.

**Architecture:** The desktop `ThemeProvider` is mounted unchanged in the phone shell. Its native
calls (`setTheme`, `setBackgroundColor`) are guarded and do nothing on iOS (tao: "iOS / Android:
Unsupported"); the iOS 26 status bar follows the luminance of the page under it (spike, ruling 1),
so the web theme alone keeps it legible. `theme-boot.js` already stamps `data-theme` before the first paint on both shells; the phone stops
overwriting it. Charts re-read tokens through `useChartTokens()`. Dynamic Type: a probe element with
`font: -apple-system-body` gives the iOS body size; its ratio to 17 px is published as
`--m-type-scale` on `<html>`, and the phone `--m-fs-*` tokens multiply it, clamped in CSS (text
roles 100–130 %, display and stat values 100–110 %).

**Tech Stack:** React 19 + TypeScript, TanStack Query, Tailwind on the design tokens, Recharts,
Vitest; iOS simulator (iPhone Air, UDID `FDC6CEAE-8A19-44F7-A5F4-00A93AF6F9A9`).

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §10.2 (type scale with Dynamic Type), §10.6
(dark mode), §8.9 (Nastavení → Vzhled); `docs/specs/2026-10-16-dark-mode-design.md` (D2–D3 material
rules, D7 category chips, D8–D11 preference and runtime, D13b switch thumb, §4.1 token table, §4.3
chart rules). Outline: `docs/plans/2026-10-16-mobile-app.md` "Phase 6". Design: `docs/design-system/README.md`
§2, §8, §13 and `docs/design-system/mobile.css`. The ten-locale translator pass of Phase 6 is **not**
part of this plan (it waits for the sibling branches).

## Global Constraints

- Both platforms, one visual language, no platform chrome; reference width 390 pt, layouts checked at 360 and 430 (spec §5, D1).
- Dark mode from the shared token block; every mobile component uses tokens only (§10.6, D9); a token never gets a platform-specific name (§10.1). No `.dark` classes: `data-theme` on `<html>` switches the block.
- Mobile type scale §10.2 (display 36/580, large title 30/600, title 20/650, section 16/620, stat 17/600, row 15/600 + sub 12/500, body 15/450, caption 11, eyebrow 10/700 uppercase); "the text roles scale with the system setting up to 130 %; display and stat values up to 110 %"; touch targets ≥ 44 pt; side gutter 16 pt; entity row 60 pt (52 single-line) — rows may grow taller with the text, never wider.
- **Desktop behaviour byte-identical.** Shared theme files (`src/theme/ThemeProvider.tsx`, `src/theme/window-theme.ts`, `public/theme-boot.js`, `src/utils/theme.ts`, `src/lib/tokens.ts`, `src/components/ui/*`) change only additively (a new optional prop, a comment); the desktop path must provably run the same code. `index.css` / `tailwind.config.ts` / `src/lib/utils.ts` gain phone-only names, never change a desktop one.
- Shared phone files touched minimally (siblings add screens in parallel): `src/mobile/shell/routes.ts` (not at all), `MobileShell.tsx` (not at all), `MobileApp.tsx`, `src/i18n/locales/*/mobile.json` (one key), `docs/design-system/README.md` §13, `docs/design-system/mobile.css`.
- Every user-facing string lands in all ten `src/i18n/locales/*/mobile.json` in the same commit; desktop wording is reused from `settings:appearance.*`.
- `invoke()` only in `src/lib/tauri-api.ts` (rule 1). Capability changes in `src-tauri/capabilities/mobile.json` only; the keychain plugin's `default.toml` is not widened (Task 1b's command is granted explicitly).
- `Cargo.lock` unchanged (`libc 0.2.191` as on `feat/mobile`); never a blanket `cargo update`.
- One Conventional Commit per task ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; stage explicit paths only; never `git stash`; never push; never touch `main`, `feat/mobile` or another worktree.
- Gates per task: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`; when Rust, a plist or a capability changed also `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`.

## Review Focus

Inputs the spec implies and no task's unit tests fully exercise; each line's check is pinned to the
owning task.

1. **An explicit choice against the OS appearance, and a live switch** (Tmavý on a light iPhone, Světlý on a dark one, an OS flip under Systém): the status bar text stays legible right after the switch, the page never follows the OS under an explicit choice, and Systém follows it again. Check: `applyStatusBarStyle` test (Task 1b); simulator 16-flip check and both cross cases, keyboard and the app-switcher blur noted (Tasks 1b and 5).
2. **The OS flips while the app is open under Systém** (Control Center, sunset schedule): page, sheets, toasts and both Recharts charts repaint without a reload. Check: `useChartTokens()` in `ScrubChart` / `CashflowBars` (Task 2); simulator `simctl ui … appearance dark` with Přehled and Cashflow open (Task 5).
3. **Cold start with a stored dark preference, and a restored desktop backup whose profile says `dark` on a phone set to `light`**: the lock screen is dark from the first frame (boot script), and after unlock the profile wins and the device mirror follows (existing `ThemeProvider` effect). Check: `theme-boot.test.ts` stays green, simulator relaunch with Tmavý (Task 5).
4. **A field focused after `maximum-scale=1` is gone, and a pinch zoom**: no zoom-in on focus (every phone field computes ≥ 16 px), and a zoomed visual viewport never shrinks the app box (`visibleHeightFor` ignores scale > 1). Test: `visibleHeightFor` (Task 4); dev-bridge scan of field font sizes and a simulator focus check (Tasks 4, 5).
5. **The largest accessibility text size at 360 pt**: no horizontal overflow on Přehled, Finance, a sheet and the tab bar; sheet footer buttons and the button pair grow taller instead of clipping. Check: `mobile.html` overflow check with the text-size switch on "Největší" at 360 (Task 4); simulator at `accessibility-extra-extra-extra-large` (Task 5).

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `src/theme/ThemeProvider.tsx` | comment of `useResolvedTheme` only | 1 |
| `src/mobile/MobileApp.tsx` | mounts `ThemeProvider` (inside `AuthProvider`, around `I18nProvider`), drops its own `showWindow` | 1 |
| `src/main.tsx` | drops the light pin | 1 |
| `src-tauri/Info.ios.plist`, `src-tauri/gen/apple/moony_iOS/Info.plist` | `UIUserInterfaceStyle` removed | 1 |
| `src-tauri/plugins/moony-keychain/{build.rs,ios/Sources/MoonyKeychainPlugin.swift,android/…/KeychainPlugin.kt,guest-js/index.ts,permissions/autogenerated/*}`, `src-tauri/capabilities/mobile.json` | `set_status_bar_style` command | 1b |
| `src/lib/tauri-api.ts`, `src/mobile/lib/status-bar.ts` (+ test), `src/mobile/MobileApp.tsx` | `keychainApi.setStatusBarStyle`, `applyStatusBarStyle`, the `StatusBarStyle` component | 1b |
| `src/mobile/components/ScrubChart.tsx`, `cashflow/CashflowBars.tsx` | `useChartTokens()` | 2 |
| `src/mobile/no-colour-literals.test.ts` | grep gate: no colour literal under `src/mobile/` without a justifying comment | 2 |
| `docs/design-system/mobile.css`, `mobile.html` | dark rules (switch thumb), theme switch on the demo page | 2 |
| `src/mobile/sheets/AppearanceSheet.tsx`, `src/mobile/pages/Settings.tsx`, `locales/*/mobile.json` | Vzhled | 3 |
| `src/mobile/lib/type-scale.ts` (+ test), `src/mobile/lib/use-type-scale.ts` | Dynamic Type probe and factor | 4 |
| `src/mobile/lib/use-visible-height.ts`, `src/mobile/lib/visible-height.ts` (+ test) | ignore a zoomed viewport | 4 |
| `src/index.css`, `docs/design-system/mobile.css`, `tailwind.config.ts`, `src/lib/utils.ts` | scaled `--m-fs-*`, roles `m-lead`, `m-note`, `m-field` | 4 |
| `index.html`, `src/mobile/pages/Lock.tsx` | viewport without `maximum-scale`, comment | 4 |
| `docs/design-system/README.md` §13 | dark + type rules, "a new phone screen must…" | 2, 4 |

---

### Task 1: Theme plumbing on the phone

**Files:** `src/main.tsx`, `src/mobile/MobileApp.tsx`, `src/theme/ThemeProvider.tsx` (comment only), `src-tauri/Info.ios.plist`, `src-tauri/gen/apple/moony_iOS/Info.plist`. No new unit test: this is wiring; the logic it relies on is covered by `src/utils/theme.test.ts` and `src/utils/theme-boot.test.ts`, and the check is the simulator (step 4).

**Interfaces:**
- Produces: `useTheme()`, `useResolvedTheme()` (now the real theme) and `useChartTokens()` usable anywhere in the phone shell; `data-theme` on `<html>` from the boot script before the first paint, then from the provider.
- Consumes: `ThemeProvider`, `useTheme`, `window-theme.ts`, `theme-boot.js` as they are on `feat/mobile` — unchanged.

- [ ] **Step 1: Mount the provider.** `MobileApp` wraps `I18nProvider` in `<ThemeProvider>` inside `AuthProvider` (the desktop order in `App.tsx`) and drops its own `showWindow` effect and comment (the provider shows the window once, after the first stamp). Update the component's doc comment.
- [ ] **Step 2: Drop the pins.** `src/main.tsx` loses the `if (mobile) document.documentElement.dataset.theme = 'light'` line and its comment. `src-tauri/Info.ios.plist` and `src-tauri/gen/apple/moony_iOS/Info.plist` lose `UIUserInterfaceStyle` and its comment (check how the generated file is produced: if `tauri ios dev` merges `Info.ios.plist` into it, edit both by hand anyway so the committed generated file matches).
- [ ] **Step 3: Comments.** `useResolvedTheme()`'s doc comment no longer says the phone is light only (the fallback stays for a tree without a provider); any other "light only until Phase 6" comment under `src/` (grep) is corrected. README §13 is Task 2's.
- [ ] **Step 4: Gates** (`npm run lint`, `typecheck`, `test`, `format:check`; the plist changed: `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`). Desktop proof for the review: `git diff d0128630 -- src/theme src/App.tsx src/components public` shows comment lines only.
- [ ] **Step 5: Commit** `feat(mobile): dark mode plumbing — ThemeProvider in the phone shell, no light pin`. The controller then checks the simulator (Systém under OS light and dark, live OS flip).

### Task 2: Every phone screen on the dark tokens

**Interfaces:** Consumes Task 1 (`useChartTokens()` works in the shell). Produces the colour gate `src/mobile/no-colour-literals.test.ts` and the README §13 dark rules a new screen must follow.

- [ ] **Step 1: Failing gate test** `no-colour-literals.test.ts` (fs read like `src/index.test.ts`): every non-test `.ts`/`.tsx` under `src/mobile/` is free of `#[0-9a-f]{3,8}\b` in strings or classes, `rgba?(`, `hsla?(`, Tailwind palette colours (`(bg|text|border|fill|stroke|ring|shadow|from|via|to|divide|outline|decoration)-(white|black|slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)\b`), unless that line or the one above carries `colour-literal-ok:` with a reason; and no `useMemo(chartTokens` (charts read tokens through `useChartTokens()`). Run it: FAIL on the two `useMemo(chartTokens, [])`.
- [ ] **Step 2: Charts.** `ScrubChart.tsx` and `cashflow/CashflowBars.tsx`: `useMemo(chartTokens, [])` → `useChartTokens()` (from `@/theme/ThemeProvider`). Every other chart mark already uses `var(--…)` (`AllocationRing`, `AllocationBar`, `FlowStrip`, `TrendCard`). Gate test PASS.
- [ ] **Step 3: Contract.** `mobile.css`: the switch thumb follows D13b (dark off-state thumb `var(--ink-2)`, on-state `var(--ink-inverse)`, as `src/index.css` does for the shared `Switch` the phone renders); the thumb's `rgba(0,0,0,.2)` shadow gets a comment that it is the desktop `switch.tsx` value in both themes. Re-judge `--hero-orb`, `--sh-seg`, `--sh-destructive`, `--loss-ring` against the dark-mode spec (D2 moonlight orb lighter than paper, black-based shadows, loss ring on `loss`) — they are the shared tokens; record the verdict as a ruling, change nothing unless a value fails, and then stop and report (a change would move the desktop).
- [ ] **Step 4: Demo page.** `mobile.html`: a Světlý / Tmavý segment in the doc head (the styleguide's pattern: stamps `data-theme` on `<html>`, `?theme=dark` and the `D` key), every frame then renders dark; the overflow check re-runs after a switch. Open it in the browser pane at 360 / 390 / 430 in both themes: all checks green.
- [ ] **Step 5: README §13** — replace "the phone is light only until Phase 6" with the dark paragraph (theme from the profile through `ThemeProvider`, the native interface style, the boot script, charts through `useChartTokens()` or `var()`), and start the list **"A new phone screen must"**: tokens only (the gate test), contrast material via `dark` / `ink-inverse`, chart colours from `useChartTokens()` or `var(--…)`, category chips through `CategoryIconChip`, checked in `mobile.html` and the simulator in both themes.
- [ ] **Step 6: Gates; commit** `feat(mobile): phone screens on the dark tokens — chart tokens, colour gate, dark demo page`.

### Task 3: Nastavení → Vzhled

**Interfaces:** Consumes `useTheme()` (Task 1), `authApi.updateProfile`, `PickRow`, `Sheet`, `SettingsRow`. Produces `AppearanceSheet({ open, onOpenChange })`.

- [ ] **Step 1: Strings** `mobile.json` × 10: `settings.appearance.lead` (cs "Systém sleduje světlý nebo tmavý vzhled telefonu.", en "System follows the phone's light or dark appearance."); everything else from `settings:appearance.title|system|light|dark`. `npx vitest run src/i18n/locales.test.ts` PASS.
- [ ] **Step 2: `AppearanceSheet`** (half sheet, the `LanguagePickerSheet` pattern): title `settings:appearance.title`, the lead, a `radiogroup` of three `PickRow`s with the current preference checked, footer Zrušit. A tap on the current choice closes; another choice applies at once (`setPreference(next)`), saves `authApi.updateProfile({ theme: next })`, on success invalidates `['user-profile']` and closes (no toast); on error reverts (`setPreference(previous)`), shows `toast.error(settings:toast.updateFailed, { description: translateApiError(error, tc) })` and stays open. A second tap while saving is ignored.
- [ ] **Step 3: Settings root**: a `SettingsRow` (Lucide `SunMoon`) after Jazyk, title `settings:appearance.title`, sub = the current choice's label, opens the sheet (`SheetKind` gains `'appearance'`).
- [ ] **Step 4: Gates; commit** `feat(mobile): Nastavení → Vzhled — Systém, Světlý, Tmavý`.

### Task 1b: The status bar follows the theme (added after the Task 3 spike)

Why: the iOS 26 status bar samples the page under it, but after a live switch it keeps the style sampled before the repaint (white text over a light page in ~30 % of OS flips, until the next scroll), and iOS 15–25 do not sample at all. Spike (controller, ledger): an explicit `preferredStatusBarStyle` on tao's view controller was legible in 24 of 24 flips, also with a sheet's scrim over the light theme.

**Interfaces:**
- Produces: `keychainApi.setStatusBarStyle(theme: ResolvedTheme): Promise<void>` → `plugin:moony-keychain|set_status_bar_style` with `{ theme }` in `src/lib/tauri-api.ts`; `applyStatusBarStyle(theme: ResolvedTheme): Promise<void>` in `src/mobile/lib/status-bar.ts` (no-op outside Tauri — `'__TAURI_INTERNALS__' in window` — and a rejection is swallowed, like `window-theme.ts`'s `guarded`); a `StatusBarStyle` component in `MobileApp.tsx`, rendered inside `ThemeProvider`, that calls `applyStatusBarStyle(useResolvedTheme())` in an effect keyed on the resolved theme.
- Consumes: `useResolvedTheme()` (Task 1).

- [ ] **Step 1: Failing test** `src/mobile/lib/status-bar.test.ts` (Vitest runs in `node`: stub `window` with `vi.stubGlobal`, mock `@/lib/tauri-api`): with `__TAURI_INTERNALS__` present `applyStatusBarStyle('dark')` calls `keychainApi.setStatusBarStyle('dark')` (same for `'light'`); without it nothing is called; a rejecting call resolves to `undefined`. Run: FAIL (module missing).
- [ ] **Step 2: Implement** `applyStatusBarStyle` and the `keychainApi` wrapper (its doc comment names the command and that it exists on phones only). PASS.
- [ ] **Step 3: Native.** Swift `@objc func setStatusBarStyle(_ invoke: Invoke)` (args `{ theme: String }`): on the main queue store the theme, install once a replacement of the `preferredStatusBarStyle` getter on the class of `manager.viewController` — only when that class is a subclass, never `UIViewController` itself (it is tao's `TaoUIViewController`) — via `class_replaceMethod` with an `imp_implementationWithBlock` that returns `.darkContent` for `"light"`, `.lightContent` for `"dark"`, `.default` otherwise, then `setNeedsStatusBarAppearanceUpdate()`; resolve at once (as `setPrivacyScreen` does). Header comment of the file gains one line on it. Kotlin `setStatusBarStyle`: `WindowInsetsControllerCompat(window, window.decorView).isAppearanceLightStatusBars = theme == "light"` on the UI thread, marked "unverified on Android". `build.rs` COMMANDS gains `set_status_bar_style`; build once (`cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings` runs the plugin's build script) and commit the regenerated `permissions/autogenerated/commands/set_status_bar_style.toml`, `reference.md` and `schemas/schema.json`; `default.toml` unchanged; `src-tauri/capabilities/mobile.json` grants `moony-keychain:allow-set-status-bar-style` and its description says why. `guest-js/index.ts` documents the command.
- [ ] **Step 4: Wire** `StatusBarStyle` in `MobileApp.tsx` next to `BackgroundLock` (inside the providers). README §13's Dark bullet: the status bar follows the theme through this command (replace the sentence that says it follows the page's luminance alone).
- [ ] **Step 5: Gates** (all, incl. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the iOS clippy). Commit `feat(mobile): the status bar follows the theme — explicit style through the keychain plugin`. The controller rebuilds and repeats the 16-flip check.

### Task 4: Dynamic Type

**Interfaces:** Produces `typeScaleFrom(bodyPx: number): number` (= bodyPx ÷ 17, rounded to 3 decimals; `1` for a non-finite or non-positive input) in `src/mobile/lib/type-scale.ts`; `useTypeScale(): void` in `src/mobile/lib/use-type-scale.ts`; `visibleHeightFor(height: number, scale: number): number | null` (`null` when `scale > 1.01`, else `Math.round(height)`) in `src/mobile/lib/visible-height.ts`; CSS `--m-type-scale` (raw factor, default 1), `--m-text-k: clamp(1, var(--m-type-scale), 1.3)`, `--m-value-k: clamp(1, var(--m-type-scale), 1.1)`; roles `--m-fs-lead` 13 px (the sentence under a title or sheet title, text links), `--m-fs-note` 10 px (stat note, flow-strip and agenda sub-lines), `--m-fs-field: max(16px, var(--m-fs-body))` with Tailwind `text-m-lead`, `text-m-note`, `text-m-field` known to `cn()`.

- [ ] **Step 1: Failing tests** `type-scale.test.ts`: `typeScaleFrom(17)` → 1, `(23)` → 1.353, `(14)` → 0.824, `(53)` → 3.118, `(NaN)` / `(0)` → 1. `visible-height.test.ts`: `visibleHeightFor(778.4, 1)` → 778, `(500, 1.005)` → 500, `(300, 2)` → null.
- [ ] **Step 2: Run both** — FAIL; implement the two pure functions; PASS.
- [ ] **Step 3: Tokens.** `src/index.css` phone `:root` and `mobile.css` `:root` (identical): the three factors above; display and stat sizes `calc(<px> * var(--m-value-k))`, every other `--m-fs-*` and the two new roles `calc(<px> * var(--m-text-k))`; `--m-fs-field`. `tailwind.config.ts` `m-lead` (13, line 1.4, weight 450), `m-note` (10, line 1.3, weight 500), `m-field` (`var(--m-fs-field)`, line 1.3, weight `var(--m-fw-body)`); `src/lib/utils.ts` lists them. `mobile.css` gets `.m-t-lead`, `.m-t-note` and its fields use `--m-fs-field`. The tab bar label and chart axis ticks stay fixed (ruling 5).
- [ ] **Step 4: Probe.** `useTypeScale()` (called in `MobileApp` next to `useVisibleHeight()`): a layout effect that, when `CSS.supports('font', '-apple-system-body')`, measures a hidden fixed probe (`font: -apple-system-body`) and sets `--m-type-scale` on `<html>` to `typeScaleFrom(px)`; re-measures on `visibilitychange` (visible) and window `focus`; cleanup removes the probe and the property. Elsewhere (Android, a browser) nothing is set and the factor stays 1.
- [ ] **Step 5: Literal sizes.** Under `src/mobile/`: `text-[13px]` → `text-m-lead` (keep a different explicit weight), `text-micro` → `text-m-note` except `TabBar.tsx`, `text-[14px]` (Údržba title) and `text-[22px]` (lock greeting) → `text-[length:calc(14px*var(--m-text-k))]` / `…22px…`. Every field (`input`, `textarea`, `select`, `Input`, the lock and first-run fields, `AmountField`, `MonthField`, `SearchField`) uses `text-m-field`, except a field drawn in a larger role already (≥ 16 px). Fixed heights around text become minimums (`h-12` → `min-h-12` with vertical padding on buttons that hold a label: `ButtonPair`, sheet footers, the full-width buttons, segments `h-[30px]` → `min-h-[30px]`), rows keep their 60 / 52 pt minimum.
- [ ] **Step 6: Viewport.** `index.html`: `width=device-width, initial-scale=1.0, viewport-fit=cover` (desktop webviews ignore the meta). `index.css`: `html[data-shell='mobile'] { touch-action: manipulation; }` (no double-tap zoom; pinch stays). `use-visible-height.ts` uses `visibleHeightFor` and skips both the update and the pin-to-top while zoomed. `Lock.tsx`'s field comment says fields are ≥ 16 px (`text-m-field`), so iOS does not zoom on focus, and pinch zoom is allowed.
- [ ] **Step 7: Check at 360.** `mobile.html` gets a "Velikost písma: 100 % · 130 % · Největší" switch (sets `--m-type-scale` 1 / 1.3 / 3.1 on `<html>`, re-runs the check). In the browser pane at 360 in both themes with "Největší": every frame's check green (rows may be taller; a row-height failure only for rows that grew is expected — the check's row rule becomes "at least 60 / 52"). In the dev bridge (`http://localhost:1460`, 360 × 780) with `--m-type-scale: 3.1` set by hand on `<html>`: Přehled, Finance, the transaction sheet and the tab bar have no element past the right edge (`document.querySelectorAll('*')` bounding-box scan) and every focused-able field computes ≥ 16 px.
- [ ] **Step 8: README §13** type table gains a "scales to" column (130 % / 110 % / fixed), the two new roles and `m-field`, a Dynamic Type paragraph (probe, factor, caps, floor, no event → re-measure on return), and the type half of **"A new phone screen must"**: `text-m-*` roles only (no px sizes, `text-micro` only in the tab bar), fields `text-m-field`, minimum heights instead of fixed ones around text, checked with the text-size switch at 360.
- [ ] **Step 9: Gates; commit** `feat(mobile): Dynamic Type — text roles to 130 %, values to 110 %, no viewport lock`.

### Task 5: Simulator verification, screenshots, final review, hand-over

Run by the controller.

- [ ] **Step 1: Gates at the head** (all, incl. Rust and the iOS clippy); `git diff d0128630 -- src-tauri/Cargo.lock` is empty.
- [ ] **Step 2: Simulator** (iPhone Air, port 1460, demo profile Alex Novák / `demo1234`). Dark walk under OS dark + Systém: lock, first run (one step), Přehled, Agenda, Finance, account detail + ledger + transaction sheet + category picker + filter, import sheet, categorize queue, Cashflow + budgets + limit sheet + drilldown, Majetek, Akcie, position detail, Nastavení and every section, a confirm sheet, a toast, the privacy blur. Then the four cross cases of Review Focus 1, the live OS flip (2), a relaunch with Tmavý (3), a field focus without zoom (4), `content_size accessibility-extra-extra-extra-large` on Přehled, Finance, a sheet and the tab bar (5). Fix what the walk finds (one fix task per batch, sonnet).
- [ ] **Step 3: Screenshots** `docs/screenshots/mobile/dark-01-lock.png` … `dark-NN-*.png` and `type-01-overview.png`, `type-02-sheet.png`; leave the device light, default text size; stop the dev loop.
- [ ] **Step 4: Final whole-branch review (opus)** against `feat/mobile` (`d0128630..HEAD`), incl. the desktop byte-identical proof; one fix round; re-review (sonnet).
- [ ] **Step 5: Hand-over** `.superpowers/sdd/2026-10-24-mobile-phase-6a-dark/handover.md` (Czech, with the rules new screens must follow) + a copy in the shared notes folder.

---

## As built / rulings

From planning (spike results recorded in ruling 1):

1. **(Superseded by ruling 12.) No native appearance call on the phone.** tao's `Window::set_theme` is "iOS / Android: Unsupported" (a no-op), so the provider's guarded native calls do nothing there. Spike on the iPhone Air (iOS 26.3) before Task 1: with the page dark under a light OS the status bar text is white, with the page light under a dark OS it is black, and a light page with `color-scheme: dark` under a light OS keeps black text — the iOS 26 status bar follows the luminance of the content under it, not the OS and not `color-scheme`. The web theme alone keeps it legible. Costs if wrong: the deployment target is iOS 15.0 and no older runtime is installed; an iPhone on iOS 15–25 likely draws the OS style, illegible when the choice differs from the OS. The fallback is an in-repo plugin command setting the window's `overrideUserInterfaceStyle` (sketched in the hand-over); Phase 7 decides it together with the deployment target.
2. **Vzhled is a sheet from the Settings root, like Jazyk**, not a pushed section: three choices fit a half sheet, and `routes.ts` (which the sibling branches edit) stays untouched. Costs nothing if the owner wants a section later (one entry in `SETTINGS_SECTIONS`).
3. **The four phone tokens keep the shared values** (`--hero-orb #302f2b`, `--sh-seg`, `--sh-destructive`, `--loss-ring` from the dark-mode spec §4.1): §10.1 forbids platform-specific token names and the desktop must stay identical; the simulator walk judges them on the phone.
4. **Dynamic Type has a floor of 100 %**: a smaller system size keeps the design size (the 10–11 pt roles are the legibility floor); the caps sit in CSS (`clamp`), the probe publishes the raw factor.
5. **Fixed type**: the tab bar label (iOS keeps tab bar labels fixed and shows the large-content viewer instead) and chart axis ticks (the hero and legend carry the values).
6. **Fields are at least 16 px** (`m-field` = `max(16px, body)`): once `maximum-scale=1` is gone, iOS zooms into any focused field under 16 px. Costs fields 1 px over the 15 pt body at the default size.
7. **`touch-action: manipulation`** on the phone root: no double-tap zoom over buttons, pinch zoom stays (the reason the viewport lock goes).
8. **The launch screen follows the OS** (`systemBackgroundColor`): a native launch screen cannot read the app's preference. Phase 7 owns the launch screen.
9. **Android is not run**: the status-bar icon colour and the system font scale (WebView `textZoom`) are not wired; both wait for the Android build (Phase 7).
10. **The phone's theme save is the sheet's own mutation** mirroring `AppearanceCard` (D14: optimistic, no success toast, revert on error); the desktop card is not refactored.
12. **The status bar gets an explicit style** (Task 1b): the content-sampled default kept a stale style after ~30 % of live switches (white over a light page until the next scroll); an explicit `preferredStatusBarStyle` on tao's view controller, set from the resolved theme, was legible in 24 of 24 flips and also covers iOS 15–25. It replaces the getter on tao's own class only (`TaoUIViewController`), never on `UIViewController`. Costs one plugin command and a method replacement that a tao update could make redundant (check on Tauri upgrades).
11. **WebKit fires no event when the text size changes**: the factor is re-measured when the app returns (`visibilitychange`, `focus`); a change made while the app stays in front (Control Center's text-size slider) applies on the next return.

Rulings made during execution (the ledger `.superpowers/sdd/2026-10-24-mobile-phase-6a-dark/progress.md` has each with its review context):

13. **README §13's dark text is Task 2's alone**; Task 1 changed code comments only, so two tasks never rewrote the same paragraph.
14. **Commit trailers name the model that made the commit** (Sonnet for the subagents' task commits, Opus for the controller's), not one fixed name.
15. **Task 1b was added mid-plan** (ruling 12): the status-bar spike after Task 3 measured the content-sampled default as illegible after ~30 % of live switches; the explicit style was legible in 24/24 spike flips and 16/16 flips on the committed build.
16. **The swipe-panel action label stays a fixed 11 px**, like the tab bar label — an exception to §10.2's caption scaling: at 130 % (and even 110 %) the Czech and Italian labels clip in the 98 px panel, and every swipe action is also in the long-press menu. Costs larger-text users a small label during a swipe; README §13 lists it with the other fixed sizes.
17. **Shrink-to-fit fills 98 % of the fractional content width with fixed floors** (12 px for stat values, 20 px for hero and amount figures): WebKit rounds `clientWidth`, so a fit that filled the rounded width cut "Kč 486,552" with an ellipsis at the largest size on a 420 pt phone, and floors that grew with the text size could cut a figure that fits at the default size. Costs 0.5 px on a figure that fills 98–100 % of its box.
18. **The phone locks itself when its webview reloads** (`BackgroundLock`), so a Vite full reload (an `index.html` edit) or a fresh dev-bridge tab puts the app on the lock screen; subagents never type a password, they scan the lock screen, and the controller checks the unlocked screens on the simulator.
19. **System UI that the app does not draw follows the OS**, not the app's choice (the text-edit callout, the launch screen); the status bar and the keyboard accessory bar follow the page. Accepted.
