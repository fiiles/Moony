# Mobile app (iOS + Android) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Moony as a standalone phone app (iOS first, Android from the same code) with the
four-tab design of the spec, the statement import ladder without manual column mapping, and the
same encrypted data model as the desktop, inside the existing repository and Tauri project.

**Architecture:** One Tauri 2 project builds desktop, iOS and Android. The Rust backend (services,
SQLCipher, migrations, importers, FX, history) is shared unchanged; desktop-only pieces (updater,
process, MCP server, "open in another app") are gated with `#[cfg(desktop)]` and target-specific
dependencies, mobile-only pieces (biometrics, haptics, notifications, the in-repo keychain
plugin) with `#[cfg(mobile)]`. The frontend gets a second shell under `src/mobile/`, chosen at
build time from `TAURI_ENV_PLATFORM` (and a `VITE_MOONY_SHELL` override for browser previews), that
reuses the hooks, `tauri-api.ts`, i18n and formatting of the desktop UI and renders the mobile
pages with Tailwind on the shared tokens. The design system stays one system: `tokens.css`
(light and, from the dark-mode project, dark) plus a documented mobile component layer.

**Tech Stack:** Tauri 2.12 (mobile via `tauri ios|android`), Rust (rusqlite bundled SQLCipher,
tokio, reqwest), React 19 + TypeScript, wouter, TanStack Query, Tailwind on design tokens, Recharts,
react-i18next (10 locales), Vitest; Tauri plugins biometric, haptics, os, notification (mobile),
an in-repo mobile plugin for Keychain/Keystore and the privacy screen (Swift + Kotlin).

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` (sections §N and decisions D1–D10 below
refer to it) with the prototype `docs/specs/2026-10-16-mobile-app-prototype.html` (frames 01–16,
notes 1–21). Dark tokens: `docs/specs/2026-10-16-dark-mode-design.md` §4.1 (approved; lands on
`main` from the dark-mode branch, see Phase 6). The owner approved the spec, the prototype and
this repository layout in chat on 2026-10-16: same repo and Tauri project, worktree branch
`feat/mobile`, landing per phase, iOS first.

## Ground rules

Work happens in the worktree `.claude/worktrees/feat+mobile` (branch `worktree-feat+mobile`, to be
renamed `feat/mobile` when it lands). Other sessions use the main checkout and other worktrees;
never commit outside this worktree and check `git branch --show-current` before every commit.
`main` takes finished work only: a phase lands when the owner says so, as one `git merge --no-ff`
of a phase that leaves the desktop app unchanged in behaviour and keeps every gate green. Never
push; the owner releases.

Dev loop: `npm run ios:dev` (Task 1.1) builds and starts the app on the "iPhone 17 Pro" simulator
with Vite on port 1430 (port 1420 belongs to other sessions' desktop runs). Face ID on the
simulator: Features → Face ID → Enrolled, then `xcrun simctl spawn booted notifyutil -p
com.apple.BiometricKit_Sim.pearl.match` for a matching face. Android has no SDK on this machine
yet: Android code is written and reviewed but compiled only once the owner installs Android
Studio / the SDK + NDK (Phase 7), so every Android-specific step says "unverified on Android".

Read `AGENTS.md` (the ten rules), `docs/standards/rust-backend.md`,
`docs/standards/typescript-frontend.md`, `docs/standards/type-contract.md`,
`docs/standards/i18n.md`, `docs/standards/testing.md` and `docs/DEPRECATED.md` before coding. Use
TDD. One Conventional Commit per task, ending with `Co-Authored-By: Claude Fable 5.1
<noreply@anthropic.com>`. Every user-facing string lands in all ten locales in the same commit
(the mobile strings live in a new namespace `mobile`, registered per `docs/standards/i18n.md` §3).

Gate per task, before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri && cargo check --target aarch64-apple-ios-sim --lib
npm run lint && npm run typecheck && npm test
```

After a change to a type registered in `bindings.rs`, also `cd src-tauri && cargo test
generate_bindings -- --ignored`. A task that changes what the simulator shows ends with a
simulator run (`npm run ios:dev`, screenshot through the simulator tool) and says what was seen.

## Global constraints (from the spec)

- Both platforms, one visual language, no platform chrome: own tab bar, sheets, segmented control,
  Inter bundled; reference width 390 pt, every layout checked at 360 and 430 (§5, D1).
- Four tabs Přehled · Finance · Cashflow · Majetek, Settings behind the avatar on Přehled (§6, D2,
  D10); "add" lives where the object lives; Android system back closes a sheet or pops a detail,
  never leaves the app from a sheet.
- Same schema, migrations and encryption as the desktop; a desktop backup restores on the phone
  (§11). The only new columns are `bank_transactions.categorize_snoozed_until` (nullable) and
  notification preferences in `app_config`.
- Money, dates and numbers only through `useFormat()` / `useReportMoney` (frontend rule 8); no
  1:1 conversion, missing FX shown as "—".
- Mobile type scale §10.2 (display 36/580, large title 30/600, title 20/650, section 16/620, stat
  17/600, row 15/600 + sub 12/500, body 15/450, caption 11, eyebrow 10/700 uppercase); touch
  targets ≥ 44 pt; side gutter 16 pt; entity row 60 pt; chevron only on rows without a trailing
  value (§10.3).
- Charts: scrub replaces hover, callout replaces tooltip, hero chart 160 pt, trend 120, bars 110,
  ring 110, four axis labels (§10.4).
- Privacy: no telemetry, no server; local notifications only; backups are encrypted blobs the
  person puts in their own cloud folder (§9).
- Dark mode from the shared token block; every mobile component uses tokens only (§10.6, D9).

## Review focus

Inputs the spec implies and no task's tests exercise directly; each line's test is pinned to the
owning task.

1. A statement file shared into a **locked** app (cold start from the share sheet): the file must
   survive unlock and open the import sheet once, never twice (Phase 2, opened-URL state test).
2. A learned format whose header signature collides with a shipped preset: the shipped preset
   wins and the learned one is reported, never silently shadowed (Phase 2, `detect_preset` test).
3. Biometric enrolment invalidated on the device (new face enrolled, passcode removed): unlock
   falls back to the password with the `auth.biometricInvalidated` message, never a crash or a
   silent loop (Task 1.8, keychain retrieve test on the Rust side with a stubbed plugin error).
4. Device locale and app language differ (Japanese UI, Czech bank file with comma decimals): the
   import still parses the file and the UI formats in the app language (Phase 2, infer test with
   a locale argument).
5. A 360 dp Android width with German strings: no horizontal overflow on the tab bar, nav bar,
   stat row and entity rows (Task 1.3, a Vitest-free check: the design-system `mobile.css` demo
   page at 360 reviewed in the browser pane, recorded in the task's run notes).

---

## Phase 0 — Spike and plan (done 2026-10-16)

Findings that bind the later tasks:

- **Toolchain.** Tauri CLI 2.12.2, Xcode 26.3, iPhone 17 Pro simulators, Rust targets
  `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` installed. `cocoapods`,
  `xcodegen`, `ios-deploy` present (Homebrew). No Android SDK/NDK.
- **`tauri ios init --ci`** generated `src-tauri/gen/apple` (xcodegen project, `project.yml`,
  `Info.plist` sources, `LaunchScreen.storyboard`). It warns about no signing certificate; the
  simulator needs none. `gen/apple` is committed (only `gen/schemas` is ignored); build products
  live in Xcode's DerivedData.
- **`cargo check --target aarch64-apple-ios-sim --lib` passes** after three changes: (1)
  `libc` pinned to 0.2.189 in `Cargo.lock` — 0.2.190 made `mach_task_self` macOS-only and
  `num_threads` 0.1.7 (pulled by `time`'s `local-offset`, which `tauri-plugin-log` enables)
  fails on iOS; upstream issue jhpratt/num_threads#24 (open, 2026-10-05); revisit when a release
  fixes it, and keep the pin until then (Dependabot bumps of `libc` must keep this check green);
  (2) `services::document_files::open_with_system` has a mobile variant that returns an
  `AppError::Internal` (the mobile UI uses the share sheet, never this); (3) `lib.rs` `run()` is a
  plain function running the Tauri builder inside `tauri::async_runtime::block_on`, because
  `#[tokio::main]` hid the future from `tauri::mobile_entry_point`. Every dependency, including
  `rusqlite` with bundled SQLCipher, `axum`/`rmcp` (MCP) and `tauri-plugin-updater`, compiles for
  iOS; the desktop `cargo check` and `cargo test` stay green.
- **Simulator run:** `npm run tauri -- ios dev "iPhone 17 Pro" --config '{"build":{"devUrl":
  "http://localhost:1430","beforeDevCommand":"npm run dev -- --port 1430 --strictPort"}}'`
  starts Vite on 1430 and builds through Xcode (the "Build Rust Code" script phase runs cargo for
  the simulator). **Result (2026-10-16):** the app deploys and starts on the iPhone 17 Pro
  simulator — the log shows `Moony 0.13.0 starting on ios aarch64`, the webview loads the dev
  server, the only error is the expected `get_price_status` call before unlock ("Database is
  locked"), and the desktop UI renders at phone width until Task 1.1 switches the shell. The
  linker warns that `__eh_frame` exceeds 16 MB (debug build; harmless).
- **Plugins (official, Tauri 2, mobile):** `tauri-plugin-biometric` (authenticate only, no secret
  storage; `NSFaceIDUsageDescription` required), `tauri-plugin-notification` (local scheduling:
  `Schedule.at(date, repeating)`, `Schedule.interval`, `Schedule.every`; `pending()`, `cancel()`;
  Android channels), `tauri-plugin-haptics`, `tauri-plugin-os`, `tauri-plugin-dialog`/`fs`/
  `opener` already used and mobile-capable; `tauri-plugin-updater` and `-process` are desktop-only
  → target-specific dependencies.
- **Share-in** needs no custom native code: `bundle.fileAssociations` in `tauri.conf.json`
  (`ext`, `mimeType`, `exportedType` for Apple), Android intent filters Send/SendMultiple/View by
  default, and `tauri::RunEvent::Opened { urls }` on macOS/iOS/Android; cold-start URLs are kept
  in managed state and read by a command at startup (Tauri docs "File Associations on Mobile").
- **Secret storage** for the biometric unlock has no official plugin: Keychain (iOS, access
  control `biometryCurrentSet`) and Keystore (Android, `setUserAuthenticationRequired`) are
  implemented as a small in-repo mobile plugin `src-tauri/plugins/moony-keychain` (Swift +
  Kotlin), which also hosts the privacy screen and the app-shortcut events.
- **Identifier:** the phone app is `com.moonyfinance.mobile` (set in `tauri.ios.conf.json` and
  `tauri.android.conf.json`; the desktop keeps `com.moonyfinance.desktop`).
- **Bank export inventory (T1):** `docs/plans/2026-10-16-mobile-app-bank-exports.md` (written
  by the research agent from public documentation; facts marked verified / secondary /
  unverified; nothing was tested on a device). Headline: structured data straight from the
  phone app is documented only for mBank CZ (CSV, ABO), Wise (CSV/XLSX) and Revolut (Excel),
  probably Partners Banka; the large Czech and German banks (ČS George, Fio, KB+, Air Bank,
  Raiffeisenbank, MONETA, ČSOB, DKB, ING, Sparkasse) and N26/Chase give **PDF statements** in
  the app and keep CSV / ABO / MT940 / CAMT on their web banking. Consequences for Phase 2
  (binding when it is expanded): (a) the per-bank export guide must cover the web-banking route
  on the phone ("open internet banking in Safari/Chrome → export CSV → save to Files → share to
  Moony") as a first-class path, not a fallback; (b) a PDF-statement reader moves from "v2
  research" to a Phase 2 spike on the three most common Czech PDF layouts (ČS, Fio, KB), with a
  go/no-go for the owner; (c) George, KB+ and ČSOB Smart need a real-device check before their
  guides are written (owner's phone or TestFlight), and nothing is hard-coded as "the app cannot
  export" without one.

## Phase plan

Each phase is a landable increment that leaves the desktop unchanged. Phase 1 is written in full
below; Phases 2–7 are outlined and are expanded into tasks when the previous phase lands (the
rolling-plan rule: a phase's tasks are written against the code as it is then, never against a
guess).

| Phase | Delivers | Lands when |
|---|---|---|
| 1 | Mobile shell, lock with biometrics, first run, Přehled with Údržba, hide amounts, background lock, privacy screen, design-system mobile layer | the owner sees the simulator unlock with Face ID and the overview on demo data |
| 2 | Finance tab (accounts, loans, policies), account detail and ledger, category picker and queue, the import ladder (share-in, structured, presets + catalog refresh + export guides, guided recognition, learned formats, contribute) | an unknown CSV imports from the share sheet without a mapping grid |
| 3 | Cashflow with budgets, manage budgets, drilldown | |
| 4 | Majetek, class pages, details (position, bond edit, property with camera, other asset, loan with what-if, policy with documents), watchlist | |
| 5 | Settings sheet, backup to Files/SAF, restore and profile transfer, notifications, app shortcuts, Oznámení and Import sections | |
| 6 | Dark mode on mobile from the shared tokens (after the dark-mode branch lands), ten-locale parity for the `mobile` namespace | |
| 7 | Release: icons, launch screen, Android build (SDK), CI jobs, README/ROADMAP/PRIVACY, TestFlight | |

---

## Phase 1 — Shell, lock, overview

### Task 1.1: Platform switch, mobile entry and dev loop

**Files:**
- Create: `src/lib/platform.ts`, `src/lib/platform.test.ts`, `src/mobile/MobileApp.tsx`,
  `src-tauri/tauri.mobile-dev.conf.json`
- Modify: `vite.config.ts` (envPrefix), `src/main.tsx`, `src/vite-env.d.ts`, `index.html`
  (viewport `viewport-fit=cover`), `package.json` (scripts `ios:dev`, `android:dev`)

**Interfaces:**
- Produces: `isMobileShell(env: { TAURI_ENV_PLATFORM?: string; VITE_MOONY_SHELL?: string }): boolean`
  — `VITE_MOONY_SHELL` of `mobile` / `desktop` wins; otherwise true for platform `ios` or
  `android`. `MobileApp` default export: the provider stack of `App.tsx` without
  `UpdaterProvider` and `UpdateNotification`, rendering `MobileShell` (Task 1.3; until then a
  placeholder `<main>` with the text key `mobile:shell.placeholder`).

- [ ] **Step 1: Write the failing test** `src/lib/platform.test.ts`: `isMobileShell({TAURI_ENV_PLATFORM:'ios'})` is true, `'android'` true, `'macos'` false, `{}` false, `{TAURI_ENV_PLATFORM:'macos', VITE_MOONY_SHELL:'mobile'}` true, `{TAURI_ENV_PLATFORM:'ios', VITE_MOONY_SHELL:'desktop'}` false.
- [ ] **Step 2: Run** `npx vitest run src/lib/platform.test.ts` — FAIL (module missing).
- [ ] **Step 3: Implement** `isMobileShell` in `src/lib/platform.ts`; add `envPrefix: ['VITE_', 'TAURI_ENV_*']` to `vite.config.ts`; declare `TAURI_ENV_PLATFORM` and `VITE_MOONY_SHELL` in `src/vite-env.d.ts`; in `src/main.tsx` render `MobileApp` (dynamic `import('./mobile/MobileApp')`) when `isMobileShell(import.meta.env)`, else `App`.
- [ ] **Step 4: Dev loop.** `src-tauri/tauri.mobile-dev.conf.json` = `{"build":{"devUrl":"http://localhost:1430","beforeDevCommand":"npm run dev -- --port 1430 --strictPort"}}`; scripts `"ios:dev": "dotenv -- tauri ios dev \"iPhone 17 Pro\" --config src-tauri/tauri.mobile-dev.conf.json"` and `"android:dev": "dotenv -- tauri android dev --config src-tauri/tauri.mobile-dev.conf.json"`.
- [ ] **Step 5: Run** the test (PASS), `npm run typecheck`, `npm run lint`; then `npm run ios:dev` and screenshot the simulator: the placeholder text is on screen. Record the result in the commit body.
- [ ] **Step 6: Commit** `feat(mobile): platform switch, mobile entry and simulator dev loop`.

### Task 1.2: Mobile targets in Cargo, plugins, capabilities and identifier

**Files:**
- Create: `src-tauri/tauri.ios.conf.json`, `src-tauri/tauri.android.conf.json`,
  `src-tauri/Info.ios.plist`, `src-tauri/capabilities/mobile.json`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`,
  `src-tauri/src/commands/auth.rs` (MCP start under `#[cfg(desktop)]`), `package.json`

**Interfaces:**
- Produces: plugins registered on mobile: biometric, haptics, notification, os; on desktop:
  updater, process (unchanged). Capability `mobile` (platforms iOS, android) grants
  `core:default`, `opener:default`, `log:default`, `dialog:default`, `fs:deny-default`,
  `biometric:default`, `haptics:default`, `notification:default`, `os:default`; `default`
  gets `"platforms": ["macOS", "windows", "linux"]`.

- [ ] **Step 1:** Cargo: move `tauri-plugin-updater` and `tauri-plugin-process` under `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]`; add `tauri-plugin-biometric = "2"`, `tauri-plugin-haptics = "2"`, `tauri-plugin-notification = "2"` under `[target.'cfg(any(target_os = "ios", target_os = "android"))'.dependencies]`; add `tauri-plugin-os = "2"` for all targets. npm: `@tauri-apps/plugin-biometric`, `-haptics`, `-notification`, `-os`.
- [ ] **Step 2:** `lib.rs`: wrap the updater/process `.plugin(...)` calls in `#[cfg(desktop)]` blocks (a `builder` variable mutated in two `cfg` blocks) and add the mobile plugins under `#[cfg(mobile)]`. `commands/auth.rs::unlock`: the MCP server start under `#[cfg(desktop)]`.
- [ ] **Step 3:** `tauri.ios.conf.json` and `tauri.android.conf.json`: `{"identifier": "com.moonyfinance.mobile"}`; `Info.ios.plist`: `NSFaceIDUsageDescription` = "Moony unlocks your data with Face ID."; regenerate `gen/apple` (`rm -rf src-tauri/gen/apple && npm run tauri -- ios init --ci`) and check `PRODUCT_BUNDLE_IDENTIFIER` is `com.moonyfinance.mobile`.
- [ ] **Step 4: Run** the full gate (desktop check, clippy, tests, iOS check) and `npm run ios:dev`; the app still starts.
- [ ] **Step 5: Commit** `build(mobile): target-specific plugins, mobile capability and identifier`.

### Task 1.3: Mobile shell — tab bar, nav bar, routes, safe areas — and the design-system mobile layer

**Files:**
- Create: `src/mobile/shell/MobileShell.tsx`, `src/mobile/shell/TabBar.tsx`,
  `src/mobile/shell/NavBar.tsx`, `src/mobile/shell/Screen.tsx`, `src/mobile/shell/routes.ts`,
  `src/mobile/pages/{Overview,Finance,Cashflow,Wealth}.tsx` (placeholders),
  `src/i18n/locales/*/mobile.json` (10 files), `docs/design-system/mobile.css`,
  `docs/design-system/mobile.html` (demo at 360/390/430)
- Modify: `src/index.css` (safe-area custom properties `--safe-top`, `--safe-bottom` from
  `env(safe-area-inset-*)`, the mobile type-scale utilities), `tailwind.config.ts` (font-size
  names `m-display`, `m-title-lg`, `m-title`, `m-section`, `m-stat`, `m-row`, `m-body`, `m-sub`,
  `m-caption`), `src/i18n/index.ts` (namespace `mobile`), `docs/design-system/README.md` (§13
  Mobile), `src/mobile/MobileApp.tsx` (render `MobileShell`)

**Interfaces:**
- Produces: `routes` = `{ overview: '/', finance: '/finance', cashflow: '/cashflow', wealth: '/wealth', settings: '/settings', auth: '/auth' }`; `<Screen title large? leading? trailing?>` (nav bar + scroll area with bottom padding for the tab bar); `<TabBar active>`; `MobileShell` = wouter `Switch` of the four roots inside `ProtectedRoute`, `/auth` outside (Task 1.4). Tab bar: paper, hairline top, 56 pt + `--safe-bottom`, Lucide 24, active `ink`, inactive `ink-4`, labels 10/600 (§10.3).

- [ ] **Step 1:** Write `docs/design-system/mobile.css` (entity row, stat row, hero, tab bar, nav bar, sheet, segmented, buttons at phone sizes — the classes of prototype notes 7, 14) and `mobile.html` showing them at 360, 390 and 430 with German sample strings; open in the browser pane and record no horizontal overflow at 360 (review focus 5).
- [ ] **Step 2:** Add the `mobile` i18n namespace with the shell strings (tabs, placeholder) in all ten locales; `npx vitest run src/i18n/` PASS.
- [ ] **Step 3:** Implement the shell components with Tailwind on tokens (no raw hex, no `.dark` classes); `MobileShell` routes; placeholders show their tab name.
- [ ] **Step 4:** `npm run ios:dev`; screenshot: four tabs switch, the tab bar clears the home indicator, titles collapse on scroll.
- [ ] **Step 5:** README §13 Mobile (what `mobile.css` is, the type scale, the row rule). Commit `feat(mobile): shell with tab bar and nav bar; design-system mobile layer`.

### Task 1.4: Lock screen (password) and first run on the phone

**Files:**
- Create: `src/mobile/pages/Lock.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (`/auth` route), `src/components/auth/OnboardingWizard.tsx` only if it breaks at 390 (it is already single-column below `lg`)

**Interfaces:**
- Consumes: `useAuth()` (`appStatus`, `unlockMutation`, `recoverMutation`, …), `GREETING_NAME_KEY`, `LOCKED_AT_KEY`.
- Produces: `Lock` page per prototype frame 01: brand, "Vítejte zpět, {name}", password field, "Odemknout", "Zapomenuté heslo?" (the desktop recovery flow components reused), language segment, caption. `needs_setup` renders `OnboardingWizard`.

- [ ] **Step 1:** Implement `Lock` and the `/auth` route; `ProtectedRoute` redirects there as on the desktop.
- [ ] **Step 2:** Simulator: first run creates a test profile (values generated here, never the owner's), unlock shows the four tabs, lock from a temporary button on Overview (removed in Task 1.6) returns to Lock with the greeting.
- [ ] **Step 3:** Commit `feat(mobile): lock screen and first run`.

### Task 1.5: Přehled — hero with scrub, stats, agenda, allocation, recent moves

**Files:**
- Create: `src/mobile/pages/Overview.tsx`, `src/mobile/components/Hero.tsx`,
  `src/mobile/components/StatRow.tsx`, `src/mobile/components/EntityRow.tsx`,
  `src/mobile/components/AgendaRow.tsx`, `src/mobile/components/ScrubChart.tsx`,
  `src/mobile/lib/scrub.ts`, `src/mobile/lib/scrub.test.ts`
- Modify: `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: the hooks `src/pages/Dashboard.tsx` uses (portfolio metrics and history for the period, milestones, recent moves, allocation) — read that page and `src/hooks/use-portfolio.ts`; `useFormat()`.
- Produces: `ScrubChart` (Recharts line, 160 pt, touch/mouse scrub → `onScrub(point | null)`; big-step marks tappable), `nearestPoint(points, x, width)` pure in `scrub.ts`; `EntityRow` (§10.3: chip, title, sub, value, valueSub, chevron only without value, `onSwipeLeft`/`onSwipeRight` with labels); `AgendaRow` (date chip, dark ≤ 7 days, swipe right "Za týden", swipe left "Skrýt", long press menu via the shadcn dropdown).

- [ ] **Step 1: Write the failing test** `scrub.test.ts`: `nearestPoint([{x:0},{x:10},{x:20}], 12, 20)` → index 1; `x` beyond the end → last; empty → null.
- [ ] **Step 2:** Run (FAIL), implement `scrub.ts` (PASS).
- [ ] **Step 3:** Implement the components and the page in the order of prototype frame 02 (without Údržba, Task 1.6): hero rewrites value and date while scrubbing and snaps back on release; three stats; three agenda rows + "Zobrazit vše"; allocation ring (reuse the desktop ring component if it takes a size, else an SVG ring) with the legend; five recent moves.
- [ ] **Step 4:** Simulator on the demo-like profile: screenshot each section; scrub works with a drag.
- [ ] **Step 5:** Commit `feat(mobile): overview with scrub hero, stats, agenda, allocation and recent moves`.

### Task 1.6: Údržba card and the record-balances sheet

**Files:**
- Create: `src/mobile/lib/upkeep.ts`, `src/mobile/lib/upkeep.test.ts`,
  `src/mobile/components/UpkeepCard.tsx`, `src/mobile/sheets/RecordBalancesSheet.tsx`,
  `src/mobile/components/Sheet.tsx` (bottom sheet on the shadcn `Sheet` with `side="bottom"`,
  drag handle, footer on `well`, Android back closes it)
- Modify: `src/mobile/pages/Overview.tsx`

**Interfaces:**
- Consumes: milestones (`kind`, `dueDay`, `sourceId`), bank accounts (`balanceAsOf`, last import
  from `getImportBatches`), the uncategorized count the Finance tab also uses
  (`bankAccountsApi.getBankTransactions` with the uncategorized filter, count only).
- Produces: `buildUpkeepRows(input: { todayDay: number; milestones: Milestone[]; accounts: BankAccount[]; lastImportByAccount: Record<string, number | null>; uncategorized: number }): UpkeepRow[]` with `UpkeepRow = { kind: 'balances_stale' | 'uncategorized' | 'statement_due' | 'backup_stale' | 'valuation_stale'; title: string-key params; accountIds?: string[] }` ordered balances → uncategorized → statement → backup → valuation; `statement_due` when an account has imports and the newest is older than 35 days; `balances_stale` when a manual account's `balanceAsOf` is older than 30 days. `RecordBalancesSheet(accountIds)`: one amount field per account prefilled empty with the last balance as placeholder, date today, one save through the existing balance mutation (`use-bank-account-mutations.ts`), toast "Zůstatky zaznamenány".

- [ ] **Step 1: Failing tests** for `buildUpkeepRows`: 30/35-day thresholds (inclusive edges), ordering, empty input → `[]`, an account with imports newer than 35 days produces no `statement_due`.
- [ ] **Step 2:** Run (FAIL), implement (PASS).
- [ ] **Step 3:** Card + sheet; the card is absent when `rows` is empty; "Zařadit", "Importovat", "Zálohovat", "Přecenit" navigate to their tabs (their flows arrive in Phases 2, 5, 4).
- [ ] **Step 4:** Simulator: with a stale balance the card shows; saving through the sheet removes the row.
- [ ] **Step 5:** Commit `feat(mobile): upkeep card and record-balances sheet`.

### Task 1.7: Keychain plugin (Rust + Swift + Kotlin) with privacy screen

**Files:**
- Create: `src-tauri/plugins/moony-keychain/` (`Cargo.toml`, `build.rs`, `src/lib.rs`,
  `src/mobile.rs`, `src/desktop.rs`, `permissions/default.toml`, `ios/Package.swift`,
  `ios/Sources/MoonyKeychainPlugin.swift`, `android/build.gradle.kts`,
  `android/src/main/java/com/moonyfinance/keychain/KeychainPlugin.kt`, `guest-js/index.ts`)
- Modify: `src-tauri/Cargo.toml` (path dependency, mobile targets), `src-tauri/src/lib.rs`
  (`#[cfg(mobile)] .plugin(moony_keychain::init())`), `src-tauri/capabilities/mobile.json`

**Interfaces:**
- Produces (Rust): `trait KeychainExt { fn keychain(&self) -> &Keychain<R> }` with
  `Keychain::store(&self, key: &str, value_hex: &str) -> Result<()>` (biometry-protected item),
  `retrieve(&self, key: &str) -> Result<Option<String>>` (prompts the person; `Err(KeychainError::Invalidated)` when the enrolment changed, `Ok(None)` when nothing stored), `remove(&self, key)`, `set_privacy_screen(&self, enabled: bool)`. Swift: `SecItemAdd` with `SecAccessControlCreateWithFlags(kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, .biometryCurrentSet)`; blur `UIVisualEffectView` on `willResignActive`, removed on `didBecomeActive`. Kotlin: AndroidKeyStore AES-GCM key with `setUserAuthenticationRequired(true)`, `setInvalidatedByBiometricEnrollment(true)`, ciphertext in app-private storage, `BiometricPrompt` with `CryptoObject`; `FLAG_SECURE`-free blur overlay in `onPause`. The desktop implementation returns `KeychainError::Unsupported`.

- [ ] **Step 1:** Scaffold with `npm run tauri -- plugin new moony-keychain --no-api --ios --android` into `src-tauri/plugins/` (or by hand following `v2.tauri.app/develop/plugins/develop-mobile`), wire the path dependency and registration.
- [ ] **Step 2:** Rust unit tests for the error mapping (`KeychainError` ↔ `AppError::Auth("auth.biometricInvalidated")`, review focus 3) in `src/lib.rs` of the plugin.
- [ ] **Step 3:** Implement Swift; build and run on the simulator with Face ID enrolled: `store` then `retrieve` prompts and returns the value; after "Features → Face ID → Enrolled" toggled off and on, `retrieve` reports `Invalidated`.
- [ ] **Step 4:** Implement Kotlin (unverified on Android until Phase 7; keep it compiling by review).
- [ ] **Step 5:** Commit `feat(mobile): in-repo keychain plugin with biometry-protected storage and privacy screen`.

### Task 1.8: Biometric unlock — commands, lock page, enrolment prompt

**Files:**
- Create: `src-tauri/src/services/session_key.rs`, `src-tauri/src/commands/biometric.rs`,
  `src/mobile/sheets/EnableBiometricsSheet.tsx`, `src/mobile/lib/biometrics.ts`
- Modify: `src-tauri/src/services/auth.rs` (unlock hands the master key to `SessionKey`),
  `src-tauri/src/commands/auth.rs` (`logout` clears it), `src-tauri/src/lib.rs` (manage + handlers),
  `src-tauri/src/bindings.rs`, `shared/schema.ts`, `src/lib/tauri-api.ts` (`biometricApi`),
  `src/hooks/use-auth.tsx` (`unlockWithBiometricsMutation`), `src/mobile/pages/Lock.tsx`

**Interfaces:**
- Produces: managed `SessionKey(Mutex<Option<Zeroizing<[u8; 32]>>>)` set on every successful
  unlock, cleared on logout; commands (mobile only, registered under `#[cfg(mobile)]`)
  `biometric_unlock_status() -> BiometricStatus { available: bool; enrolled: bool; reason: Option<String> }`,
  `enable_biometric_unlock()` (stores the session key under `moony.master-key`),
  `disable_biometric_unlock()`, `unlock_with_biometrics() -> UserProfile` (retrieve → `Database::open_with_key` → profile; maps `Invalidated` to `auth.biometricInvalidated` and disables). `biometricApi` in `tauri-api.ts` mirrors them; new strings `auth.biometricInvalidated`, `mobile:lock.*`, `mobile:biometrics.*` in all locales.

- [ ] **Step 1: Failing Rust tests** in `session_key.rs`: set/take/clear; `unlock_with_biometrics` service function with a stub `KeychainLike` trait returning `Invalidated` → `AppError::Auth("auth.biometricInvalidated")`.
- [ ] **Step 2:** Run (FAIL), implement (PASS); register types in `bindings.rs`, regenerate, add to `schema.ts` and `tauri-api.ts`.
- [ ] **Step 3:** Lock page: when `enrolled`, show "Odemknout obličejem" primary and auto-prompt on mount; "Použít heslo" link; after the first password unlock on a device with biometrics `available && !enrolled`, show `EnableBiometricsSheet` once (remembered in `localStorage` `moony-biometrics-asked`).
- [ ] **Step 4:** Simulator with Face ID enrolled: password unlock → sheet → Zapnout → lock → Face ID unlock (`notifyutil … pearl.match`); toggle enrolment off → unlock falls back with the message.
- [ ] **Step 5:** Commit `feat(mobile): biometric unlock`.

### Task 1.9: Background lock and hide amounts

**Files:**
- Create: `src/mobile/lib/background-lock.ts` + test, `src/mobile/hooks/use-background-lock.ts`,
  `src/mobile/lib/hide-amounts.ts` + test, `src/mobile/hooks/use-hide-amounts.ts`,
  `src/mobile/components/Money.tsx`
- Modify: `src/mobile/MobileApp.tsx`, `src/mobile/pages/Overview.tsx` (eye button, `Money`)

**Interfaces:**
- Produces: `shouldLockOnResume(hiddenAt: number | null, now: number, afterMinutes: 0 | 1 | 5 | 15): boolean`; the hook calls `lockMutation` on `visibilitychange` → visible when due (setting `localStorage` `moony-background-lock-minutes`, default 1) and calls `set_privacy_screen(true)` once on mount. `hideAmounts` state in `localStorage` `moony-hide-amounts`; `<Money value currency …>` renders `•••• Kč` when hidden; `useHideAmounts()` returns `{ hidden, toggle }`.

- [ ] **Step 1: Failing tests** for both pure modules (0 = immediately; exact boundary; null hiddenAt never locks; toggle persists).
- [ ] **Step 2:** Run (FAIL), implement (PASS); wire the eye on Přehled; all Overview amounts through `Money`.
- [ ] **Step 3:** Simulator: background the app (home) and return after the threshold → Lock; the eye masks every amount.
- [ ] **Step 4:** Commit `feat(mobile): background lock, privacy screen on, hide amounts`.

### Task 1.10: Phase gate and hand-off note

- [ ] **Step 1:** Full gate (desktop clippy/tests, iOS check, lint, typecheck, vitest); `npm run ios:dev` smoke: first run → unlock → tabs → overview → Face ID lock/unlock; screenshots saved under `docs/screenshots/mobile/` (overview, lock).
- [ ] **Step 2:** Desktop run unchanged: `npm run tauri dev` on the demo profile opens and unlocks as before (the `run()` change).
- [ ] **Step 3:** Update `docs/architecture/overview.md` (a "Mobile shell" paragraph: the entry switch, `src/mobile/`, the plugin, what is `cfg(mobile)`), `CONTRIBUTING.md` (mobile dev loop), this plan's Phase 0 notes with the simulator result. Commit `docs: mobile shell architecture and dev loop`.
- [ ] **Step 4:** Report to the owner: branch, commits, what the simulator shows, what is unverified (Android), and ask whether Phase 1 lands.

---

## Phase 2 — Finance and the import ladder (outline; expand when Phase 1 lands)

Backend: `opened_urls` state + command + `RunEvent::Opened` emit (`src-tauri/src/commands/opened.rs`); `bundle.fileAssociations` for csv, txt, xml, ofx, qfx, qif, sta/940; reading an opened URL into bytes (`services/opened_files.rs`, iOS `file://` in the app Inbox, Android `content://` through the fs plugin or the plugin layer); `services/csv_import/infer.rs` — content-based column inference `infer_columns(headers, sample_rows, locale_hint) -> InferredMapping { date, amount | debit+credit, balance, currency, iban, counterparty, description[], transaction_id, confidence per field, sign_verified_by_balance }` with tests on the preset fixtures and on synthetic unknown files; command `infer_csv_columns`; learned formats in `app_config` key `bank_import_formats` (header signature → mapping), `csv_presets::detect_preset` consulting them after the shipped presets (review focus 2); preset `mobileExport` block + schema + fixture test; catalog refresh (`services/csv_presets/catalog.rs`: versioned index from GitHub raw, cached under app data, switch `app_config.catalog_refresh_enabled`); migration `0NN` adding `bank_transactions.categorize_snoozed_until`; queue command `get_uncategorized_queue(account_id?) -> rows with suggestion {categoryId, source}`; contribute URL builder (pure, tested).

Frontend: Finance page (sections Účty / Úvěry / Pojištění, §8.4), account detail (hero, flows strip, ledger with infinite scroll, filters sheet, swipe to categorize, transaction sheet, category picker §8.4), categorize queue page (§8.5), import sheet with the ladder states (§8.6: file → structured/preset → "Takhle tomu rozumím" → per-field correction → preview → done with "Zařadit N plateb", "Uložit jako formát", "Poslat formát"), share-in handling on cold and warm start (review focus 1), per-bank export guide view.

## Phase 3 — Cashflow with budgets (outline)

Cashflow page (§8.7) on `get_cashflow_report` + `get_budgeting_report` for the month view; category rows with share or limit bar; tap a bar switches the month; Spravovat rozpočty page and the limit sheet (Od/Do, overwrite warning) on the existing budgeting commands; drilldown list with recategorize.

## Phase 4 — Majetek and details (outline)

Majetek page (§8.8), Akcie/Krypto class pages with the TWR stat (`get_stock_performance`), position detail with cost line and trades, bonds list with the dated events list and edit sheet, real estate detail with stacked sections and camera capture (`<input type="file" accept="image/*" capture="environment">` → `add_photo_to_batch` with raw bytes, as the desktop does), other asset light detail, loan detail with the what-if sheet, policy detail "facts first" with documents, watchlist segment and detail.

## Phase 5 — Settings, backup, notifications, shortcuts (outline)

Settings sheet sections (§8.9); backup to a picked folder through the dialog plugin (iOS Files, Android SAF) writing the existing backup zip; restore / profile transfer from a picked zip (`inspect_backup` + `restore_backup`); automatic backup cadence; notifications scheduled from milestones on unlock (`tauri-plugin-notification` `Schedule.at`, T−14, T−1, monthly upkeep day, backup stale), per-group switches on the milestone mute list; app shortcuts (iOS `UIApplicationShortcutItems` in `Info.ios.plist` handled in the keychain plugin → event; Android shortcuts.xml) opening record-balances, import, add-transaction; Import section (learned formats, catalog switch).

## Phase 6 — Dark mode and locales (outline)

After the dark-mode branch lands on `main`: merge `main` into `feat/mobile`, `data-theme` on `<html>` from the shared Vzhled setting and `prefers-color-scheme`, every mobile component reviewed on the dark tokens, the chart marks' white replaced by tokens; ten-locale review of the `mobile` namespace by a translator pass with the glossary; CJK and German width checks at 360.

## Phase 7 — Release (outline)

Icons and launch screen (`tauri icon` + storyboard), Android SDK/NDK install (owner), `tauri android init`, Kotlin plugin compile and the Android run, CI jobs (macOS runner for iOS build, Android build), store metadata, README (mobile section, how to transfer a profile), ROADMAP (phone app shipped), PRIVACY (backups, notifications, catalog refresh request), TestFlight / internal track with the owner's accounts.
