# Mobile Phase 5 — Settings, backup, notifications, first run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the phone app its Nastavení (settings root, Profil, Zabezpečení, Připomínky, Oznámení,
Záloha a přenos, language, lock, delete), backup to Files and restore / profile transfer, local
notifications scheduled from the milestones, and the phone first-run wizard with the Biometrie and
Záloha steps; iOS app shortcuts if the budget allows.

**Architecture:** Everything is frontend under `src/mobile/` on the Phase 1/2a shell: Settings is the
existing `/settings` page (reached from the avatar) whose rows push section pages under
`/settings/:section`; transactional dialogs are sheets. Device-level preferences (background lock,
hide on launch, notification preferences) live in `localStorage` as Phase 1 began; profile and
milestone preferences go through the existing commands. Backup on iOS: the dialog plugin's `save`
exports whatever file already sits at `Documents/<fileName>` through the Files picker, so a backup is
staged there by two small `#[cfg(mobile)]` commands (stage without recording, finish = record + remove)
and the recovery key is staged by the fs plugin under a one-file scope. Notifications are planned by
a pure function from the milestone list and scheduled on every unlock through
`@tauri-apps/plugin-notification`. The phone first run is a new component tree (`FirstRun.tsx` +
step files) reusing the desktop wizard's flow hooks and schema; the desktop wizard stays untouched.

**Tech Stack:** React 19 + TypeScript, wouter, TanStack Query, react-hook-form + zod, Tailwind on the
design tokens, `@tauri-apps/plugin-dialog` 2.8 (iOS `UIDocumentPicker` export / open), `-fs` 2.7,
`-notification` 2.7, `@tauri-apps/api/path`; Rust (two thin commands in `commands/backup.rs`); Vitest.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.1, §8.2, §8.9, §9, §10 (prototype
`docs/specs/2026-10-16-mobile-app-prototype.html` frames 01 and 15, note 20); the spec wins over the
prototype (R7). Outline and rulings: `docs/plans/2026-10-16-mobile-app.md` "Phase 5", rulings R11
(phone first-run variant: 44 pt controls, phone wording, Biometrie and Záloha steps), R23 (after an
invalidation the enrolment sheet re-offers), R25 (the owner decides OS device backups — this plan
implements nothing for it and presents both options in the hand-over). Design: `docs/design-system/README.md`
§13 and `docs/design-system/mobile.css`.

## Global Constraints

- Both platforms, one visual language, no platform chrome; reference width 390 pt, layouts checked at 360 and 430 (spec §5, D1).
- Four tabs; Settings behind the avatar on Přehled (§6, D2, D10); Android system back closes a sheet or pops a detail, never leaves the app from a sheet.
- Same schema, migrations and encryption as the desktop; a desktop backup restores on the phone (§11). No new tables or columns in this phase; the backup zip is the desktop's archive unchanged.
- Money, dates and numbers only through `useFormat()` / `useMoneyText()`; phone code never calls `fmt.money` itself (README §13 "Hide amounts"; `grep -rn "fmt.money(" src/mobile` lists only `use-money-text.ts`).
- Mobile type scale §10.2 (`text-m-*` roles); touch targets ≥ 44 pt; side gutter 16 pt; entity row 60 pt; chevron only on rows without a trailing value, except the settings-style `KeyValueRow` with `onClick` (README §13).
- Privacy: no telemetry, no server; local notifications only; backups are encrypted blobs the person puts in their own folder (§9). The recovery key reaches disk only as the staged file the person exports, and it is removed right after.
- Tokens only, no `.dark` classes, Lucide 24 / 1.75 in bars, 16 in row chips.
- Every user-facing string lands in all ten `src/i18n/locales/*/mobile.json` in the same commit (`src/i18n/locales.test.ts` parity); keys under `settings`, `backup`, `firstRun`, `notifications`. Desktop wording is reused through its own namespaces (`milestones:groups.*`, `settings:data.*`, `settings:dangerZone.*`, `auth:onboarding.*`, `common:*`) wherever the text is the same.
- `invoke()` only in `src/lib/tauri-api.ts` (rule 1); the Rust commands are thin, use `AppError`, no `.unwrap()` outside tests (rules 4, 5); no new Rust types (nothing to register in `bindings.rs`).
- Capability changes in `src-tauri/capabilities/mobile.json` only; the keychain plugin's permissions are never widened.
- `Cargo.lock` keeps `libc = 0.2.189`; never a blanket `cargo update`.
- Shared files touched minimally and additively (sibling branches edit them too): `src/mobile/shell/routes.ts`, `src/mobile/shell/MobileShell.tsx`, `src/mobile/components/UpkeepCard.tsx` (one target line), `src/mobile/pages/Lock.tsx` (the wizard swap), `docs/design-system/README.md` §13.
- One Conventional Commit per task ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`; stage explicit paths only; never `git stash`.

## Review Focus

Inputs the spec implies and no task's tests exercise directly; each line's test is pinned to the
owning task.

1. **A deadline notification must fire at 09:00 local time.** The iOS plugin parses `schedule.at.date` with `DateFormatter("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'")` and no time zone set, i.e. the digits are read as local wall-clock time, and it throws `pastScheduledTime` for a date already past. Test: `scheduleDateString` emits the local digits with the literal `Z` and `buildNotificationPlan` drops every item whose local time is not after `now` (Task 2).
2. **A cancelled export is not a backup.** The Files picker dismissed after the archive was staged must leave no file in Documents and must not move the "last backup" date (the `backup_stale` reminder would then stay silent for 30 days while no backup exists). Test: `finish_backup_export` with `exported = false` removes the staged file and records nothing; the path and name validators refuse anything outside `Documents/moony-backup-YYYY-MM-DD.zip` (Task 3, Rust).
3. **A picked path arrives as a `file://` URL.** The iOS picker returns URLs; the dialog crate simplifies `file:` URLs to paths, but a percent-encoded or scheme-prefixed value must still become a filesystem path before `inspect_backup`. Test: `dialogPathToFsPath` (Task 3).
4. **Restore under biometric unlock.** After a restore the keychain still holds the previous profile's master key; the next Face ID unlock must not loop or crash. The restore flow switches biometric unlock off first and clears the "asked" flag so the first password unlock offers it again (Task 3, simulator check with biometrics on; `restoreBackupPlan` test for the order of steps).
5. **Monthly upkeep day beyond the month.** Day 29–31 does not exist in every month; the picker offers 1–28 only and `nextUpkeepDate` for `upkeepDay = 28` on 2026-02-28 at 10:00 lands on 2026-03-28 09:00 (Task 2).

## File structure

| File | Responsibility |
|---|---|
| `src/mobile/shell/routes.ts` (modify, additive) | `routes.settingsSection = '/settings/:section'`, `SETTINGS_SECTIONS`, `settingsPath(section)` |
| `src/mobile/shell/MobileShell.tsx` (modify, additive) | the section route; mounts `useNotificationSchedule()` |
| `src/mobile/pages/Settings.tsx` (rewrite) | the settings root: profile head, grouped rows with state sub-lines, lock, delete, version |
| `src/mobile/pages/settings/SettingsSection.tsx` (create) | routes `:section` to the section pages; unknown → `/settings` |
| `src/mobile/pages/settings/ProfileSection.tsx`, `SecuritySection.tsx`, `RemindersSection.tsx`, `NotificationsSection.tsx`, `BackupSection.tsx` (create) | one pushed page per section |
| `src/mobile/components/SettingsRow.tsx` (create) | the settings row: `EntityRow` with a square chip, title, state sub-line, chevron, `danger` tone |
| `src/mobile/components/SectionPage.tsx` (create) | a pushed settings page: `Screen` without tab bar, back to Nastavení, one lead sentence |
| `src/mobile/sheets/LanguagePickerSheet.tsx`, `DeleteProfileSheet.tsx`, `DayPickerSheet.tsx`, `RestoreBackupSheet.tsx`, `CurrencyPickerSheet.tsx`, `CountryPickerSheet.tsx` (create) | the sheets |
| `src/mobile/lib/settings-summary.ts` (+ test) | pure sub-line builders for the root rows |
| `src/mobile/lib/background-lock.ts`, `hide-amounts.ts` (modify, additive) | `writeBackgroundLockMinutes`, `readHideAmountsOnLaunch` / `writeHideAmountsOnLaunch` |
| `src/mobile/hooks/use-biometric-enrolment.ts` (create) | enable / disable mutations with the toasts; `EnableBiometricsSheet` calls it |
| `src/mobile/lib/notification-prefs.ts` (+ test), `src/mobile/hooks/use-notification-prefs.ts` | the stored preferences |
| `src/mobile/lib/notification-plan.ts` (+ test) | pure schedule math |
| `src/mobile/hooks/use-notification-schedule.ts`, `use-notification-permission.ts` | scheduling on unlock; permission state |
| `src/mobile/lib/export-file.ts` (+ test) | file names, staged paths, path normalisation, the restore step order |
| `src/mobile/hooks/use-backup.ts`, `use-export-text-file.ts` | create / export / restore flows; text export through Documents |
| `src-tauri/src/commands/backup.rs`, `src-tauri/src/services/backup.rs` (modify), `src-tauri/src/lib.rs` (register) | `stage_backup_export`, `finish_backup_export`; `is_export_file_name`, `is_staged_export_path` |
| `src-tauri/capabilities/mobile.json` (modify) | `fs:allow-write-text-file` and `fs:allow-remove` scoped to `$DOCUMENT/moony-recovery-key.txt` |
| `src/lib/tauri-api.ts` (modify) | `dataApi.stageBackupExport`, `dataApi.finishBackupExport` |
| `src/mobile/pages/FirstRun.tsx`, `src/mobile/pages/first-run/*Step.tsx`, `src/mobile/hooks/use-first-run.ts`, `src/mobile/lib/first-run.ts` (+ test), `src/mobile/components/StepProgress.tsx` (create) | the phone first run |
| `src/mobile/pages/Lock.tsx` (modify) | renders `FirstRun` instead of `OnboardingWizard` |
| `src/mobile/components/UpkeepCard.tsx` (modify, one line) | `backup_stale` → `settingsPath('backup')` |
| `src/i18n/locales/*/mobile.json` (modify, ten files) | `settings`, `backup`, `firstRun`, `notifications` |
| `docs/design-system/README.md` §13 (modify) | rows for the settings row, section page, step progress, sheets; the backup export and notification paragraphs |
| `src-tauri/Info.ios.plist`, `src-tauri/plugins/moony-keychain/ios/Sources/MoonyKeychainPlugin.swift` (optional Task 5) | shortcut items and their event |

---

### Task 1: Settings root, section pages, Zabezpečení, Profil, language, lock, delete, version

**Files:**
- Modify: `src/mobile/shell/routes.ts`, `src/mobile/shell/routes.test.ts`, `src/mobile/shell/MobileShell.tsx`
- Rewrite: `src/mobile/pages/Settings.tsx`
- Create: `src/mobile/pages/settings/SettingsSection.tsx`, `ProfileSection.tsx`, `SecuritySection.tsx`; `src/mobile/components/SettingsRow.tsx`, `SectionPage.tsx`; `src/mobile/sheets/LanguagePickerSheet.tsx`, `DeleteProfileSheet.tsx`; `src/mobile/hooks/use-biometric-enrolment.ts`; `src/mobile/lib/settings-summary.ts` + `.test.ts`
- Modify: `src/mobile/lib/background-lock.ts` + test, `src/mobile/lib/hide-amounts.ts` + test, `src/mobile/sheets/EnableBiometricsSheet.tsx` (uses the hook), ten `mobile.json`
- Section pages for Připomínky, Oznámení and Záloha are created in Tasks 2 and 3; until then `SettingsSection` renders `PlaceholderSheet`-style text for them (replaced in those tasks).

**Interfaces:**
- Consumes: `EntityRows`/`EntityRow`/`RowChip` (`chip`, `title`, `sub`, `href`/`onClick`, `chevron`), `Screen`, `NavTextButton`, `SwitchRow`, `KeyValueRow`, `Segment`, `Sheet`, `PickGroup`/`PickRow`, `PlaceholderSheet`, `useAuth()` (`user`, `lockMutation`, `appStatus`), `useSettingsMutations().updateProfile`, `useLanguage()` (`src/i18n/I18nProvider.tsx`), `LANGUAGES`/`languageInfo` (`src/i18n/languages.ts`), `biometricApi`, `BIOMETRIC_STATUS_KEY`, `biometricCopy`, `biometricFailure`, `markEnrolmentAsked`, `readBackgroundLockMinutes`, `BACKGROUND_LOCK_OPTIONS`, `readHideAmounts`, `getVersion` (`@tauri-apps/api/app`), `authApi.deleteAccount`, `currencyName` (`@shared/currencies`), `useCurrency()` (`src/lib/currency`).
- Produces:
  - `routes.ts`: `export const SETTINGS_SECTIONS = ['profile', 'security', 'reminders', 'notifications', 'backup'] as const; export type SettingsSection = (typeof SETTINGS_SECTIONS)[number]; routes.settingsSection = '/settings/:section'; export const settingsPath = (section: SettingsSection) => \`/settings/${section}\`; export function isSettingsSection(value: string): value is SettingsSection`.
  - `SettingsRow({ icon: LucideIcon, title: string, sub?: ReactNode, href?: string, onClick?: () => void, danger?: boolean })` — an `EntityRow` with `chip={<RowChip square>…</RowChip>}` (danger: `bg-loss-soft text-loss`), `chevron`, and `title` in `text-loss` when `danger`.
  - `SectionPage({ title: string, lead?: string, children })` — `Screen` with `tabBar={false}`, leading `NavTextButton` back to `routes.settings` (`useBack(routes.settings)`), the lead as the first body line (13 / 450 `ink-3`, `mx-0.5 mb-3.5`).
  - `settings-summary.ts`: `export interface TextRef { key: string; params?: Record<string, string | number> }`; `securitySummary(input: { enrolled: boolean; biometryType: BiometryType; lockMinutes: BackgroundLockMinutes; hideOnLaunch: boolean }): TextRef[]` → `[biometric part, lock part, hide part?]` with keys `settings.security.biometricsOn` (`{{method}}` = `settings.security.methodFace|methodTouch|methodGeneric`) or `settings.security.biometricsOff`, `settings.security.lockNow` or `settings.security.lockAfter` (`{{count}}` minutes, plural), and `settings.security.hideOnLaunchShort` only when on; `lockAfterOption(minutes): TextRef` for the segment labels (`settings.security.optionNow`, `settings.security.optionMinutes` with count).
  - `background-lock.ts`: `writeBackgroundLockMinutes(storage: Pick<Storage,'setItem'> | null | undefined, minutes: BackgroundLockMinutes): void`.
  - `hide-amounts.ts`: `readHideAmountsOnLaunch(storage): boolean`, `writeHideAmountsOnLaunch(storage, on: boolean): void` (`'1'` / `'0'` under `HIDE_AMOUNTS_ON_LAUNCH_KEY`).
  - `use-biometric-enrolment.ts`: `useBiometricEnrolment(): { enable: UseMutationResult<void, Error, void>; disable: UseMutationResult<void, Error, void> }` — `enable` calls `biometricApi.enable(t('biometrics.enableReason'))`, toasts exactly as `EnableBiometricsSheet` does today (`biometrics.enabled`, `biometrics.notEnabled` by `biometricFailure`), `disable` calls `biometricApi.disable()` and toasts `settings.security.biometricsDisabled`; both invalidate `BIOMETRIC_STATUS_KEY` on settle and call `markEnrolmentAsked(localStorage)`.
  - `DeleteProfileSheet({ open, onOpenChange })` — the typed word is the desktop's `'DELETE'` (`DELETE_ACCOUNT_CONFIRM_WORD`, exported from `src/components/settings/DangerZoneCard.tsx` or duplicated as a const with a comment); on success `localStorage.clear()` then `window.location.reload()` (desktop semantics; `MobileShell` then sees `needs_setup`).
  - `LanguagePickerSheet({ open, onOpenChange })` — `PickRow` per `LANGUAGES` entry (label `native`), the current checked; a pick runs `updateProfile.mutate({ language }, { onSuccess: () => setLanguage(language) })` (as `LanguageCard`), closes on success.

- [ ] **Step 1: Write the failing tests**

`src/mobile/lib/settings-summary.test.ts`:
```ts
it('names the biometry and the lock delay', () => {
  expect(securitySummary({ enrolled: true, biometryType: 'faceId', lockMinutes: 1, hideOnLaunch: false }))
    .toEqual([
      { key: 'settings.security.biometricsOn', params: { method: 'settings.security.methodFace' } },
      { key: 'settings.security.lockAfter', params: { count: 1 } },
    ]);
});
it('says Ihned for zero minutes and adds the hide-on-launch part only when on', () => {
  expect(securitySummary({ enrolled: false, biometryType: 'none', lockMinutes: 0, hideOnLaunch: true }))
    .toEqual([
      { key: 'settings.security.biometricsOff' },
      { key: 'settings.security.lockNow' },
      { key: 'settings.security.hideOnLaunchShort' },
    ]);
});
```
`background-lock.test.ts` (append): `writeBackgroundLockMinutes(store, 15)` then `readBackgroundLockMinutes(store)` is `15`; a throwing store does not throw.
`hide-amounts.test.ts` (append): `writeHideAmountsOnLaunch(store, true)` → `readHideAmountsOnLaunch(store)` true and `readHideAmounts(store)` true even with `moony-hide-amounts = '0'`; `writeHideAmountsOnLaunch(store, false)` → false.
`routes.test.ts` (append): `tabForPath('/settings/security')` is `null`; `settingsPath('backup')` is `'/settings/backup'`; `isSettingsSection('appearance')` is false.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/mobile/lib/settings-summary.test.ts src/mobile/lib/background-lock.test.ts src/mobile/lib/hide-amounts.test.ts src/mobile/shell/routes.test.ts`
Expected: FAIL (missing exports).

- [ ] **Step 3: Implement the helpers and routes** (signatures above). `tabForPath` needs no change: `/settings/*` is not a tab root.

- [ ] **Step 4: Run the tests to verify they pass**

- [ ] **Step 5: Build the settings root** (`Settings.tsx`): `Screen title={tc('nav.settings')} tabBar={false}` with the existing back button; body in order:
  1. Head: the 42 pt avatar chip (initials as on Přehled), name + surname (`text-m-title`), e-mail (13 / 450 `ink-3`) — `mb-4`.
  2. `EntityRows`: Profil (icon `User`, sub `settings.rows.profileSub` = e-mail or `settings.rows.profileNoEmail`), Jazyk (icon `Globe`, sub = `languageInfo(language).native`; opens `LanguagePickerSheet`).
  3. `EntityRows`: Zabezpečení (`Lock`, sub = `securitySummary(...)` parts translated and joined with ` · `), Záloha a přenos (`Upload`, sub = `backup.lastOn` with `fmt.dateTime(lastBackupAt)` or `backup.never`; `lastBackupAt` from `onboardingApi.getProgress()` under query key `['onboarding-progress']`), Oznámení (`Bell`, sub = `notifications.permission.granted|denied|default` from `useNotificationPermission()` — Task 2 provides it; in Task 1 use a local `useQuery(['notification-permission'], isPermissionGranted)` and the two keys `granted`/`notGranted`, replaced in Task 2), Připomínky (`CalendarClock`, sub = `settings.rows.remindersSub` with `{{on}}` groups on of `{{total}}` from `useMilestoneMutedKinds` + `MILESTONE_GROUP_IDS`/`kindsOf`).
  4. `EntityRows`: Import (`FileText`, sub `placeholder.import` text) → `PlaceholderSheet`.
  5. `EntityRows`: Zamknout aplikaci (`Lock`, `tc('nav.lockApp')`, `onClick` → `lockMutation.mutate()`), Smazat všechna data (`Trash2`, `danger`, title `settings:dangerZone.deleteAll`, sub `settings.rows.deleteSub`) → `DeleteProfileSheet`.
  6. Footer caption (`text-m-caption text-ink-4`, centred, `mt-6`): `settings.version` = "Moony {{version}} · {{privacy}}" with `getVersion()` (state, `''` until resolved) and `settings.noTelemetry`.
  The rows use `href={settingsPath(section)}`.

- [ ] **Step 6: Section route and page shell.** `routes.settingsSection` → `SettingsSection` in `MobileShell` (a `ProtectedRoute` placed after `routes.settings`). `SettingsSection` reads `useParams().section`; not `isSettingsSection` → `<Redirect to={routes.settings} />`; `reminders`, `notifications`, `backup` render `SectionPage` with the lead `shell.placeholder` until Tasks 2–3.

- [ ] **Step 7: Zabezpečení** (`SecuritySection.tsx`, lead `settings.security.lead`):
  - `SwitchRow id="biometrics"`: label `settings.security.biometrics` with `{{method}}` from `biometricCopy`-style kind (`settings.security.methodFace|methodTouch|methodGeneric`), sub = `status.reason` (translated through `t(reason)`) when `!status.available`, else `biometrics.lead*` by kind; `checked = status.enrolled`; `disabled` while the status is pending, while a mutation runs, or when `!available && !enrolled`; on → `enable.mutate()`, off → `disable.mutate()`.
  - Lock after: a label line `settings.security.lockAfterLabel` (12 / 600 `ink-2`) over a `Segment` with the four `BACKGROUND_LOCK_OPTIONS` (labels from `lockAfterOption`), value from `readBackgroundLockMinutes(deviceStorage())` in state, `onChange` → `writeBackgroundLockMinutes` + state; sub-sentence `settings.security.lockAfterHint`. `useBackgroundLock` reads the key on every return to the foreground, so nothing else changes.
  - `SwitchRow id="hide-on-launch"`: `settings.security.hideOnLaunch`, sub `settings.security.hideOnLaunchHint`, state from `readHideAmountsOnLaunch`, writes through `writeHideAmountsOnLaunch`.
  - Change password is not in this phase (ruling 4 below).

- [ ] **Step 8: Profil** (`ProfileSection.tsx`): a form of `name`, `surname`, `email` (48 pt `Input`s with the lock screen's `field48` sizing, labels from `common:labels.*`, the desktop `ProfileCard` zod schema reused — import it if exported, else the same `updateProfileSchema` from `@shared/schema`), primary 48 pt "Uložit změny" (`common:buttons.saveChanges`) → `updateProfile.mutate({ name, surname, email })`; under it a `KeyValueRow` "Hlavní měna" (`settings:currency.main`) with value `${code} · ${currencyName(code, fmt.locale)}` and sub `settings.profile.currencyReadOnly` ("Hlavní měnu změníte v Moony na počítači") — no `onClick` (ruling 3).

- [ ] **Step 9: Strings** (ten locales; cs/en by hand, the eight others translated in the same style as the existing `mobile.json`):
  `settings.rows.profile` "Profil", `settings.rows.profileNoEmail` "Jméno a e-mail" (the sub-line is the e-mail itself when there is one), `settings.rows.language` "Jazyk", `settings.rows.security` "Zabezpečení", `settings.rows.backup` "Záloha a přenos", `settings.rows.notifications` "Oznámení", `settings.rows.reminders` "Připomínky", `settings.rows.remindersSub` "{{on}} z {{total}} skupin", `settings.rows.import` "Import", `settings.rows.deleteSub` "po potvrzení napsáním slova", `settings.version` "Moony {{version}} · {{privacy}}", `settings.noTelemetry` "žádná telemetrie", `settings.security.lead` "Odemykání, zámek na pozadí a skrývání částek na tomto telefonu.", `settings.security.biometrics` "Odemykat {{method}}", `settings.security.methodFace` "obličejem", `methodTouch` "otiskem", `methodGeneric` "biometrií", `settings.security.biometricsOn` "Odemykání {{method}}", `settings.security.biometricsOff` "Jen heslo", `settings.security.biometricsDisabled` "Odemykání biometrií vypnuto", `settings.security.lockAfterLabel` "Zamknout po přechodu na pozadí", `settings.security.lockAfterHint` "Po návratu do aplikace se po této době znovu odemyká.", `settings.security.optionNow` "Ihned", `settings.security.optionMinutes` "{{count}} min", `settings.security.lockNow` "zámek ihned", `settings.security.lockAfter_one/few/other` "zámek po {{count}} minutě / minutách / minutách" (en: "lock after {{count}} minute(s)"), `settings.security.hideOnLaunch` "Skrývat částky po spuštění", `settings.security.hideOnLaunchHint` "Každé spuštění začne se skrytými částkami; oko na Přehledu je zase ukáže.", `settings.security.hideOnLaunchShort` "částky skryté po spuštění", `settings.profile.lead` "Jméno se zobrazuje v pozdravu na Přehledu.", `settings.profile.currencyReadOnly` "Hlavní měnu změníte v Moony na počítači.", `settings.language.title` "Jazyk aplikace", `settings.language.lead` "Čísla a data se formátují podle jazyka.", `settings.delete.title` "Smazat všechna data?", `settings.delete.lead` "Trvale smaže databázi, klíče a přílohy z tohoto telefonu. Kopie zálohy mimo telefon zůstanou. Nejdřív si udělejte zálohu — je to jediná cesta zpět.", `settings.delete.confirmLabel` "Pro potvrzení napište {{word}}", `settings.delete.confirm` "Smazat data", `settings.delete.failed` "Smazání selhalo", `backup.lastOn` "Poslední záloha {{date}}", `backup.never` "Zatím žádná záloha", `notifications.permission.granted` "Povoleno", `notifications.permission.notGranted` "Nepovoleno".

- [ ] **Step 10: Gate and commit**

Run: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check` — all clean.
```bash
git add src/mobile/shell/routes.ts src/mobile/shell/routes.test.ts src/mobile/shell/MobileShell.tsx src/mobile/pages/Settings.tsx src/mobile/pages/settings src/mobile/components/SettingsRow.tsx src/mobile/components/SectionPage.tsx src/mobile/sheets/LanguagePickerSheet.tsx src/mobile/sheets/DeleteProfileSheet.tsx src/mobile/sheets/EnableBiometricsSheet.tsx src/mobile/hooks/use-biometric-enrolment.ts src/mobile/lib/settings-summary.ts src/mobile/lib/settings-summary.test.ts src/mobile/lib/background-lock.ts src/mobile/lib/background-lock.test.ts src/mobile/lib/hide-amounts.ts src/mobile/lib/hide-amounts.test.ts src/i18n/locales
git commit -m "feat(mobile): settings root with section pages, security switches, profile, language, lock and delete"
```

---

### Task 2: Připomínky and Oznámení with local notifications

**Files:**
- Create: `src/mobile/pages/settings/RemindersSection.tsx`, `NotificationsSection.tsx`; `src/mobile/sheets/DayPickerSheet.tsx`; `src/mobile/lib/notification-prefs.ts` + `.test.ts`; `src/mobile/lib/notification-plan.ts` + `.test.ts`; `src/mobile/hooks/use-notification-prefs.ts`, `use-notification-permission.ts`, `use-notification-schedule.ts`
- Modify: `src/mobile/pages/settings/SettingsSection.tsx` (wire both), `src/mobile/pages/Settings.tsx` (the Oznámení sub-line uses `useNotificationPermission`), `src/mobile/shell/MobileShell.tsx` (mount `useNotificationSchedule()` next to `EnableBiometricsSheet`), ten `mobile.json`

**Interfaces:**
- Consumes: `useMilestones()`, `useMilestoneMutedKinds()`, `useMilestoneActions().setGroupMuted`, `MILESTONE_GROUP_IDS`, `kindsOf`, `isDeadline`, `DEADLINE_KINDS`, `daysBetween` (`src/utils/milestones.ts`), `utcDayFloor` (`src/utils/chart-axis`), `useSettingsMutations().updateProfile` (insurance notice days, `parseNoticeDays` from `src/utils/insurance-notice`), `@tauri-apps/plugin-notification` (`isPermissionGranted`, `requestPermission`, `sendNotification`, `cancelAll`, `pending`), `useAuth().appStatus`, `whenVisible`.
- Produces:
  - `notification-prefs.ts`: `export const NOTIFICATION_PREFS_KEY = 'moony-notifications'; export interface NotificationPrefs { deadlines: boolean; upkeep: boolean; upkeepDay: number; backup: boolean }; export const DEFAULT_NOTIFICATION_PREFS: NotificationPrefs = { deadlines: true, upkeep: true, upkeepDay: 1, backup: true }; export const UPKEEP_DAYS = 1..28; readNotificationPrefs(storage): NotificationPrefs` (missing / unparsable / out-of-range fields fall back field by field); `writeNotificationPrefs(storage, prefs): void`.
  - `notification-plan.ts`:
    ```ts
    export const NOTIFY_HOUR = 9;          // local
    export const MAX_PENDING = 60;         // iOS keeps 64 pending requests
    export interface LocalTime { year: number; month: number; day: number; hour: number; minute: number } // month 1–12
    export type PlannedKind = 'deadline14' | 'deadline1' | 'upkeep' | 'backup';
    export interface PlannedNotification { id: number; kind: PlannedKind; at: LocalTime; title: TextRef; body: TextRef } // TextRef from settings-summary.ts (Task 1)
    export function localTimeOf(date: Date): LocalTime;                       // the date's local components
    export function isAfter(a: LocalTime, b: LocalTime): boolean;
    export function scheduleDateString(at: LocalTime): string;               // "2026-11-02T09:00:00.000Z" — local digits, literal Z (plugin quirk, Review Focus 1)
    export function utcDayToLocalTime(day: number, hour: number): LocalTime; // the UTC day's calendar date at `hour` local
    export function nextUpkeepDate(now: LocalTime, dayOfMonth: number): LocalTime; // this month at 09:00 if still ahead, else next month
    export function notificationId(kind: PlannedKind, discriminator: string): number; // stable 31-bit FNV-1a of `${kind}:${discriminator}`, never 0
    export function buildNotificationPlan(input: { now: LocalTime; milestones: readonly Milestone[]; mutedKinds: readonly MilestoneKind[]; prefs: NotificationPrefs }): PlannedNotification[];
    export function planSignature(plan: PlannedNotification[]): string;      // ids and dates joined, for the effect's dependency
    ```
    Rules of `buildNotificationPlan`: (a) `deadline14` / `deadline1` for every milestone with `isDeadline(m)` and `m.kind` not in `mutedKinds`, on the calendar day of `m.actionDay ?? m.dueDay` minus 14 and minus 1, at 09:00 local, title `{ key: 'notifications.deadline.title', params: { title: m.title } }`, body `{ key: 'notifications.deadline.in14' | 'notifications.deadline.tomorrow', params: { whatKey: 'notifications.deadline.kind.<m.kind>' } }` (the scheduler translates every param whose name ends in `Key` before interpolating, so the body reads "Konec fixace za 14 dní"), only when `prefs.deadlines`; (b) one `upkeep` at `nextUpkeepDate(now, prefs.upkeepDay)` when `prefs.upkeep` (title `notifications.upkeep.title`, body `notifications.upkeep.body`); (c) one `backup` at tomorrow 09:00 local when `prefs.backup`, a `backup_stale` milestone is in the list and `'backup_stale'` is not muted (title `notifications.backup.title`, body `notifications.backup.never` when `sinceDay === null`, else `notifications.backup.stale`); (d) every item whose `at` is not after `now` is dropped; (e) sorted by `at`, cut to `MAX_PENDING`; ids from `notificationId(kind, m.key | 'upkeep' | 'backup')`.
  - `use-notification-permission.ts`: `useNotificationPermission(): { state: 'granted' | 'denied' | 'default' | 'unknown'; request: () => Promise<void>; isPending: boolean }` — `useQuery(['notification-permission'], () => isPermissionGranted())` mapped to `granted` / `default`; `request` runs `requestPermission()` and writes the answer (`'granted'` | `'denied'`) into the query cache (`setQueryData`); a rejected call (desktop shell, pane without the plugin) reads as `unknown`.
  - `use-notification-prefs.ts`: `useNotificationPrefs(): [NotificationPrefs, (next: NotificationPrefs) => void]` — state seeded from `readNotificationPrefs(deviceStorage())`, the setter writes and dispatches a `window` event `moony-notification-prefs` so `useNotificationSchedule` (mounted elsewhere) re-reads.
  - `use-notification-schedule.ts`: `useNotificationSchedule(): void` — while `appStatus === 'unlocked'` and milestones + muted kinds have loaded and permission is `granted`: builds the plan, and when `planSignature` changed since the last run: `await cancelAll(); for each item await sendNotification({ id, title: t(title), body: t(body), schedule: { at: { date: scheduleDateString(at), repeating: false, allowWhileIdle: false } } as unknown as Schedule })`; errors are `console.warn`ed, never toasted. Runs again when the prefs event fires or the milestone data changes. When permission is not `granted` it does nothing (nothing to cancel either: the plugin refuses without permission).
  - `DayPickerSheet({ open, onOpenChange, value: number, onPick: (day: number) => void })` — `PickGroup` of `UPKEEP_DAYS` rows labelled through `fmt.number` + `notifications.upkeep.dayLabel` ("{{day}}. den v měsíci").

- [ ] **Step 1: Write the failing tests**

`notification-plan.test.ts` (fixed `now = { 2026, 10, 17, 10, 0 }`, milestones built with a small factory):
```ts
it('schedules T-14 and T-1 at 09:00 local on the action day of an unmuted deadline', ...) // actionDay = 2026-11-15 → items 2026-11-01 09:00 and 2026-11-14 09:00
it('uses dueDay when actionDay is null and skips muted kinds and info-tone items', ...)
it('drops items that are not after now and sorts the rest', ...)                  // T-14 already past, T-1 ahead
it('plans the monthly upkeep: this month when the day is ahead, next month otherwise', () => {
  expect(nextUpkeepDate({ year: 2026, month: 2, day: 28, hour: 10, minute: 0 }, 28)).toEqual({ year: 2026, month: 3, day: 28, hour: 9, minute: 0 });
  expect(nextUpkeepDate({ year: 2026, month: 10, day: 17, hour: 10, minute: 0 }, 20)).toEqual({ year: 2026, month: 10, day: 20, hour: 9, minute: 0 });
});
it('plans one backup reminder tomorrow only while backup_stale is present, unmuted and enabled', ...)
it('emits local digits with a literal Z', () => {
  expect(scheduleDateString({ year: 2026, month: 11, day: 2, hour: 9, minute: 0 })).toBe('2026-11-02T09:00:00.000Z');
});
it('caps the plan at MAX_PENDING and gives stable non-zero ids', ...)
```
`notification-prefs.test.ts`: defaults on empty storage; a stored `upkeepDay: 31` falls back to 1 while the other fields are kept; round trip.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/mobile/lib/notification-plan.test.ts src/mobile/lib/notification-prefs.test.ts` — FAIL.

- [ ] **Step 3: Implement the two libs** (signatures and rules above).

- [ ] **Step 4: Run the tests to verify they pass**

- [ ] **Step 5: Připomínky page** (`RemindersSection.tsx`, lead `milestones:settings.description`): ten `SwitchRow`s (`id = milestone-group-<id>`, label `milestones:groups.<id>.label`, sub `.hint`, `checked = !kindsOf(group).every(k => muted.includes(k))`, `disabled` until the list has loaded, `onCheckedChange(on) → setGroupMuted(group, !on)`); then the insurance notice days as a 48 pt numeric text field (`inputMode="numeric"`, label `milestones:settings.insuranceNotice`, hint `.insuranceNoticeHint`, commit on blur / Enter exactly as `MilestoneSettingsCard`).

- [ ] **Step 6: Oznámení page** (`NotificationsSection.tsx`, lead `notifications.lead`):
  1. Permission: a `KeyValueRow` label `notifications.permission.label` value by state (`granted` / `denied` / `default` → `notifications.permission.granted|denied|notAsked`); under it, when `default`: a 48 pt primary `notifications.permission.allow` → `request()`; when `denied`: the sentence `notifications.permission.deniedHint` ("Povolte je v Nastavení telefonu → Moony → Oznámení."); when `unknown`: `notifications.permission.unavailable`.
  2. `SwitchRow` `deadlines` (label `notifications.deadlines.label`, sub `.hint`), `SwitchRow` `upkeep` (`notifications.upkeep.label`, sub `.hint` with `{{day}}`), a `KeyValueRow` `notifications.upkeep.day` with value `{{day}}.` opening `DayPickerSheet` (only while `upkeep` is on), `SwitchRow` `backup` (`notifications.backup.label`, sub `.hint`). All through `useNotificationPrefs`.
  3. A secondary 48 pt button `notifications.test.send` ("Poslat zkušební oznámení") that schedules `notificationId('upkeep', 'test')` 10 s ahead with `notifications.test.title` / `.body`, then toasts `notifications.test.sent` — the person's own check that the phone shows Moony's notifications (and the simulator's).
  Every switch is disabled with sub `notifications.permission.needed` while permission is not `granted`.

- [ ] **Step 7: Mount the scheduler** in `MobileShell` (a `<NotificationScheduler />` null component calling the hook, rendered beside `EnableBiometricsSheet`), wire both pages into `SettingsSection`, switch the root's Oznámení sub-line to `useNotificationPermission`.

- [ ] **Step 8: Strings**: `notifications.lead` "Moony připomíná jen z tohoto telefonu, bez serveru. Plánují se při každém odemknutí.", `notifications.permission.label` "Oznámení", `.granted` "Povoleno", `.denied` "Zakázáno", `.notAsked` "Zatím nepovoleno", `.notGranted` "Nepovoleno", `.allow` "Povolit oznámení", `.deniedHint`, `.unavailable` "Na tomto zařízení nejsou dostupná.", `.needed` "Nejdřív povolte oznámení.", `notifications.deadlines.label` "Termíny", `.hint` "14 dní a 1 den před termínem.", `notifications.upkeep.label` "Měsíční údržba", `.hint` "{{day}}. den v měsíci: zůstatky a výpisy.", `.day` "Den v měsíci", `.dayLabel` "{{day}}. den v měsíci", `.title` "Je čas na údržbu dat", `.body` "Zaznamenejte zůstatky a naimportujte výpisy.", `notifications.backup.label` "Záloha", `.hint` "Když je poslední záloha starší než 30 dní.", `.title` "Zálohujte Moony", `.stale` "Poslední záloha je starší než měsíc.", `.never` "Zatím nemáte žádnou zálohu.", `notifications.deadline.title` "{{title}}", `.in14` "{{what}} za 14 dní", `.tomorrow` "{{what}} zítra", `notifications.deadline.kind.insurance_anniversary` "Termín pro výpověď pojistky", `.insurance_end` "Konec pojistky", `.loan_fixation_end` "Konec fixace", `.bond_maturity` "Splatnost dluhopisu", `.account_termination` "Konec účtu", `.savings_rate_end` "Konec zvýhodněné sazby", `notifications.test.send` "Poslat zkušební oznámení", `.title` "Zkušební oznámení", `.body` "Takhle vypadá připomínka z Moony.", `.sent` "Přijde za 10 sekund".

- [ ] **Step 9: Gate and commit**

Run: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`.
```bash
git add src/mobile/pages/settings src/mobile/pages/Settings.tsx src/mobile/sheets/DayPickerSheet.tsx src/mobile/lib/notification-prefs.ts src/mobile/lib/notification-prefs.test.ts src/mobile/lib/notification-plan.ts src/mobile/lib/notification-plan.test.ts src/mobile/hooks/use-notification-prefs.ts src/mobile/hooks/use-notification-permission.ts src/mobile/hooks/use-notification-schedule.ts src/mobile/shell/MobileShell.tsx src/i18n/locales
git commit -m "feat(mobile): reminder groups, notification preferences and local notifications planned from the milestones"
```

---

### Task 3: Záloha a přenos — backup to Files, restore and profile transfer

**Files:**
- Modify: `src-tauri/src/services/backup.rs` (two pure validators + tests), `src-tauri/src/commands/backup.rs` (two `#[cfg(mobile)]` commands), `src-tauri/src/lib.rs` (register under `#[cfg(mobile)]`), `src-tauri/capabilities/mobile.json`, `src/lib/tauri-api.ts`
- Create: `src/mobile/lib/export-file.ts` + `.test.ts`, `src/mobile/hooks/use-backup.ts`, `src/mobile/hooks/use-export-text-file.ts`, `src/mobile/pages/settings/BackupSection.tsx`, `src/mobile/sheets/RestoreBackupSheet.tsx`
- Modify: `src/mobile/pages/settings/SettingsSection.tsx`, `src/mobile/components/UpkeepCard.tsx` (`backup_stale: settingsPath('backup')`), ten `mobile.json`

**Interfaces:**
- Consumes: `backup::create_backup(conn, &dir, &dest, APP_VERSION)`, `onboarding::record_backup_created(conn, at)`, `app.path().document_dir()`, `dataApi.inspectBackup`, `dataApi.restoreBackup`, `biometricApi.status/disable`, `clearEnrolmentAsked`, `@tauri-apps/plugin-dialog` `save` / `open`, `@tauri-apps/plugin-fs` `writeTextFile` / `remove`, `@tauri-apps/api/path` `documentDir`, `todayIsoUtc`, `formatBytes`, `useFormat`, `translateApiError`, `ConfirmSheet`-style footer, `queryClient`.
- Produces:
  - Rust `services/backup.rs`: `pub fn is_export_file_name(name: &str) -> bool` (exactly `moony-backup-YYYY-MM-DD.zip`, digits only, nothing else); `pub fn is_staged_export_path(document_dir: &Path, path: &Path) -> bool` (`path.parent() == Some(document_dir)` after no normalisation beyond `Path` equality, and the file name passes `is_export_file_name`); `pub fn unix_now() -> i64` if no equivalent exists (reuse the one `create_backup` uses for `created_at`).
  - Rust `commands/backup.rs` (both `#[cfg(mobile)]`, `#[tauri::command]`, async, `AppError`):
    - `stage_backup_export(app: AppHandle, db: State<'_, Database>, file_name: String) -> Result<String>` — refuses a name that fails `is_export_file_name` (`AppError::Validation`), writes `document_dir()/<file_name>` through `backup::create_backup` **without** `record_backup_created`, returns the path as a string.
    - `finish_backup_export(app: AppHandle, db: State<'_, Database>, path: String, exported: bool) -> Result<()>` — refuses a path that fails `is_staged_export_path`, removes the file (a missing file is fine), and when `exported` records `record_backup_created(conn, unix_now())`.
  - `tauri-api.ts`: `dataApi.stageBackupExport: (fileName: string) => Promise<string>`, `dataApi.finishBackupExport: (path: string, exported: boolean) => Promise<void>` with a comment that both exist on phones only.
  - `mobile.json` capability: add `{ "identifier": "fs:allow-write-text-file", "allow": [{ "path": "$DOCUMENT/moony-recovery-key.txt" }] }` and `{ "identifier": "fs:allow-remove", "allow": [{ "path": "$DOCUMENT/moony-recovery-key.txt" }] }`; `fs:deny-default` stays; the description sentence explains the one staged file and why (iOS export needs the file in Documents first).
  - `export-file.ts`: `export const RECOVERY_KEY_FILE = 'moony-recovery-key.txt'; backupFileName(isoDate: string): string` (`moony-backup-${isoDate}.zip`); `stagedPath(dir: string, name: string): string` (one slash between); `dialogPathToFsPath(value: string): string` (strips `file://`, percent-decodes, leaves a plain path alone); `export type ExportOutcome = 'saved' | 'cancelled'`; `restoreBackupPlan(enrolled: boolean): Array<'disable-biometrics' | 'clear-asked' | 'restore' | 'reload'>` (`['disable-biometrics','clear-asked','restore','reload']` when enrolled, else `['clear-asked','restore','reload']`).
  - `use-export-text-file.ts`: `useExportTextFile(): { exportText: (fileName: string, contents: string) => Promise<ExportOutcome> }` — `writeTextFile(stagedPath(await documentDir(), fileName), contents)`, then `save({ defaultPath: fileName })`, then `remove(staged)` in `finally`; `null` from `save` → `'cancelled'`.
  - `use-backup.ts`: `useBackup(): { createBackup: () => Promise<ExportOutcome>; pickRestore: () => Promise<BackupInspection | null>; restore: (inspection: BackupInspection) => Promise<void>; busy: 'backup' | 'restore' | null; lastBackupAt: number | null }` —
    - `createBackup`: `name = backupFileName(todayIsoUtc())`, `staged = await dataApi.stageBackupExport(name)`, `exported = (await save({ defaultPath: name, filters: [{ name: 'Moony backup', extensions: ['zip'] }] })) !== null`, `finally await dataApi.finishBackupExport(staged, exported)`; on `'saved'` toast `settings:data.backupSaved` and invalidate `['milestones']`, `['onboarding-progress']`; on `'cancelled'` toast `backup.notSaved`; errors toast `common:status.error` with `translateApiError`.
    - `pickRestore`: `open({ multiple: false, directory: false, filters: [{ name: 'Moony backup', extensions: ['zip'] }] })`, `dialogPathToFsPath`, `dataApi.inspectBackup(path)`.
    - `restore(inspection)`: follows `restoreBackupPlan(status.enrolled)`: `biometricApi.disable()` (errors ignored), `clearEnrolmentAsked(localStorage)`, `dataApi.restoreBackup(inspection.archivePath)`, toast `settings:data.restored` + `.restoredDescription`, `window.setTimeout(() => window.location.reload(), 800)` (desktop semantics).
  - `RestoreBackupSheet({ inspection: BackupInspection | null, onOpenChange, onConfirm, isPending })` — title `settings:data.restoreConfirmTitle`, description `settings:data.restoreConfirmDescription` with `date`, `version`, `attachments`, `size`; `newerThanApp` → the `settings:data.restoreNewer` line in `text-loss` and the destructive button disabled; footer Zrušit + destructive `settings:data.restoreConfirm` / `.restoring`.
  - `BackupSection.tsx` (lead `backup.lead`): `KeyValueRow` `backup.last` with `fmt.dateTime(lastBackupAt)` or `backup.never`; primary 48 pt `settings:data.backupNow` (loading while `busy === 'backup'`); a paragraph `backup.what` (what the archive holds, in the phone's words); `SectionHead`-style heading `backup.restoreTitle`, sentence `backup.restoreLead` (restore from this phone **or transfer the profile from Moony on the computer** — same file, same password), secondary 48 pt `settings:data.restore` → `pickRestore` → `RestoreBackupSheet`; a last caption `backup.osBackupNote` (one sentence: the phone's own device backup also contains the encrypted profile — kept until the owner decides R25; the sentence is neutral and names no setting).

- [ ] **Step 1: Write the failing tests**

Rust (`services/backup.rs` tests module):
```rust
#[test] fn export_file_name_accepts_only_the_dated_pattern() { assert!(is_export_file_name("moony-backup-2026-10-17.zip")); for bad in ["moony-backup-2026-10-17.zip.txt", "../moony-backup-2026-10-17.zip", "moony-backup-20261017.zip", "other.zip", ""] { assert!(!is_export_file_name(bad), "{bad}"); } }
#[test] fn staged_export_path_must_sit_directly_in_documents() { let dir = Path::new("/c/Documents"); assert!(is_staged_export_path(dir, Path::new("/c/Documents/moony-backup-2026-10-17.zip"))); assert!(!is_staged_export_path(dir, Path::new("/c/Documents/sub/moony-backup-2026-10-17.zip"))); assert!(!is_staged_export_path(dir, Path::new("/c/Documents/moony.db"))); assert!(!is_staged_export_path(dir, Path::new("/c/moony-backup-2026-10-17.zip"))); }
```
TS `export-file.test.ts`: `backupFileName('2026-10-17')`; `stagedPath('/a/Documents/', 'x.zip')` and `stagedPath('/a/Documents', 'x.zip')` both `/a/Documents/x.zip`; `dialogPathToFsPath('file:///private/var/a%20b/c.zip')` → `/private/var/a b/c.zip`, a plain path unchanged; `restoreBackupPlan(true)` / `(false)` orders.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test backup::tests` (FAIL to compile) and `npx vitest run src/mobile/lib/export-file.test.ts` (FAIL).

- [ ] **Step 3: Implement the Rust validators, the two commands, their registration and the API wrappers.** The commands live beside `create_backup`; desktop builds see neither (`#[cfg(mobile)]` on the functions, the `generate_handler!` entries and any mobile-only `use`).

- [ ] **Step 4: Run the tests, fmt and clippy**

Run: `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test backup`, `npx vitest run src/mobile/lib/export-file.test.ts` — all pass. (The iOS clippy `cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings` runs in Task 6 before the one build.)

- [ ] **Step 5: Capability, hooks, sheet, page** per the interfaces; wire `backup` into `SettingsSection`; change the `UpkeepCard` target.

- [ ] **Step 6: Strings**: `backup.lead` "Záloha je šifrovaný soubor: databáze, klíče a přílohy. Otevře ji jen vaše heslo.", `backup.last` "Poslední záloha", `backup.what` "Uložte ji do iCloud Drive nebo jiné složky v Souborech. Moony k ní nemá přístup.", `backup.notSaved` "Záloha nebyla uložena", `backup.restoreTitle` "Obnovení a přenos", `backup.restoreLead` "Obnovte zálohu z tohoto telefonu, nebo přeneste profil z Moony na počítači: vyberte soubor zálohy a po obnovení odemkněte heslem, které platilo při jejím vytvoření.", `backup.osBackupNote` "Šifrovaný profil je i v záloze telefonu (iCloud nebo počítač); bez hesla ho nikdo neotevře.", plus `backup.lastOn` / `backup.never` from Task 1.

- [ ] **Step 7: Gate and commit**

Run: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`; the Rust gate of Step 4 again after any Rust edit.
```bash
git add src-tauri/src/services/backup.rs src-tauri/src/commands/backup.rs src-tauri/src/lib.rs src-tauri/capabilities/mobile.json src/lib/tauri-api.ts src/mobile/lib/export-file.ts src/mobile/lib/export-file.test.ts src/mobile/hooks/use-backup.ts src/mobile/hooks/use-export-text-file.ts src/mobile/pages/settings src/mobile/sheets/RestoreBackupSheet.tsx src/mobile/components/UpkeepCard.tsx src/i18n/locales
git commit -m "feat(mobile): backup to Files through a staged export, restore and profile transfer from a picked archive"
```

---

### Task 4: Phone first run (ruling R11)

**Files:**
- Create: `src/mobile/pages/FirstRun.tsx`; `src/mobile/pages/first-run/PrivacyStep.tsx`, `AccountStep.tsx`, `RecoveryStep.tsx`, `AreasStep.tsx`, `FirstAccountStep.tsx`, `BiometricsStep.tsx`, `BackupStep.tsx`; `src/mobile/hooks/use-first-run.ts`; `src/mobile/lib/first-run.ts` + `.test.ts`; `src/mobile/components/StepProgress.tsx`; `src/mobile/sheets/CurrencyPickerSheet.tsx`, `CountryPickerSheet.tsx`
- Modify: `src/mobile/pages/Lock.tsx` (import and render `FirstRun` where `OnboardingWizard` was; nothing else), ten `mobile.json`
- Untouched: `src/components/auth/OnboardingWizard.tsx` and the other desktop auth components.

**Interfaces:**
- Consumes: `useAuth()` (`appStatus`, `setupMutation`, `confirmSetupMutation`, `recoveryKey`, `clearRecoveryKey`), `setupSchema`, `MIN_PASSWORD_LENGTH`, `MenuPreferences` (`@shared/schema`), `FX_SUPPORTED_CODES`, `currencyName`, `CurrencyCode` (`@shared/currencies`), `regionFromLocale`, `defaultCurrencyForRegion`, `packIdForRegion`, `countryOptionsFromPacks`, `RULE_PACK_COUNTRY_IDS` (`src/utils/locale-defaults`), `pickVerificationGroups`, `recoveryKeyGroups`, `verifyGroups`, `recoveryKeyFileContents` (`src/utils/recovery-key-check`), `authApi.updateProfile`, `categorizationApi.getRulePacks/setRulePacksEnabled`, `onboardingApi.setFlag`, `useBankAccountMutations().createAccount`, `todayIsoUtc`, `utcDayStart`, `PasswordInput` (`touch`), `RecoveryKeyBox` (`touch`), `AmountField`, `parseAmountInput` (`src/mobile/lib/amount-input`), `SwitchRow`, `KeyValueRow`, `PickGroup`/`PickRow`, `SearchField`, `Sheet`, `BiometryIcon`, `biometricApi.status`, `useBiometricEnrolment()` (Task 1), `markEnrolmentAsked`, `useBackup().createBackup` (Task 3), `useExportTextFile().exportText` (Task 3), `MoonMark`.
- Produces:
  - `first-run.ts`: `export const FIRST_RUN_STEPS = ['privacy', 'account', 'recovery', 'areas', 'firstAccount', 'biometrics', 'backup'] as const; export type FirstRunStep = (typeof FIRST_RUN_STEPS)[number]; firstRunSteps(input: { biometryAvailable: boolean }): FirstRunStep[]` (drops `biometrics` when unavailable); `nextStep(steps, current): FirstRunStep | null`; `previousStep(steps, current): FirstRunStep | null` (null from `privacy`, from `areas` and later: the account exists, no way back — null); `stepIndex(steps, current): { index: number; total: number }` (1-based); `canLeaveWithoutAccount(step)` false from `areas` on.
  - `StepProgress({ index, total, label })` — "2 / 7 · Účet" as `text-m-caption text-ink-3` and a 3 px `well-2` bar with the `dark` fill at `index / total`, `mb-4`.
  - `use-first-run.ts`: `useFirstRun({ language, onFinished }): { step, steps, goNext, goBack, currency, setCurrency, country, setCountry, areas, setAreas, submitAccount(data), backToAccount(), createDatabase(), saveAreas(), createFirstAccount({ name, balance, currency }), skipFirstAccount(), finish(), pending: { setup, confirm, areas, account, finishing }, biometryAvailable }` — the desktop wizard's flow (`saveAreas` builds `MenuPreferences` and enables the country pack exactly as `OnboardingWizard.saveAreas`; `createFirstAccount` as `createFirstAccount(false)` without the import branch; `finish` sets `onboarding.completedAt` and `moony-first-run`); `step` moves to `recovery` when `recoveryKey` arrives, to `areas` after `confirmSetupMutation` succeeds; `biometryAvailable` from `useQuery(BIOMETRIC_STATUS_KEY, biometricApi.status)` read once the app is unlocked (`enabled: appStatus === 'unlocked'`); the step list is recomputed from it.
  - `FirstRun({ language, onFinished })` — the frame of `Lock.tsx`'s wizard branch (scrolls inside itself, safe areas), brand row (MoonMark + "moony" + `auth:onboarding.brandTagline`), `StepProgress`, the current step; every primary button 48 pt full width, every field 48 pt (`field48`), text links 44 pt.
  - Steps:
    - `PrivacyStep`: title `auth:onboarding.welcome.title`, lead `firstRun.privacyLead` (phone wording: "v tomto telefonu"), the three points (`auth:onboarding.welcome.points.*`) in a `well` box, `KeyValueRow` Hlavní měna (value `code · currencyName`) → `CurrencyPickerSheet`, `KeyValueRow` Země bankovnictví (value name or `auth:onboarding.welcome.countryNone`) → `CountryPickerSheet`, hints as 12 pt captions, primary `common:buttons.continue`.
    - `AccountStep`: `react-hook-form` + `setupSchema`; fields name, surname, e-mail (optional), password (+ strength sentence as the desktop), confirm; primary `common:buttons.continue` (loading = `setupMutation.isPending`); text link back.
    - `RecoveryStep`: title/lead `auth:onboarding.recovery.title` / `firstRun.recoveryLead` ("…mimo tento telefon…"), `RecoveryKeyBox touch` and under it a 48 pt secondary `firstRun.saveToFiles` ("Uložit do Souborů") → `exportText(RECOVERY_KEY_FILE, recoveryKeyFileContents(key, { title, note }))` (toast `auth:onboarding.recovery.saved` on `'saved'`), the two verification fields (48 pt, `inputMode="text"`, `autoCapitalize="characters"`), the verified / mismatch sentence, primary `auth:onboarding.recovery.create` disabled until verified (loading + `auth:onboarding.recovery.creating` while confirming), text link back (`backToAccount`, discards the key as the desktop).
    - `AreasStep`: title/lead `auth:onboarding.areas.title` / `firstRun.areasLead`, four `SwitchRow`s (accounts: checked + disabled, sub `auth:onboarding.areas.alwaysOn`; investments, realEstate, liabilities with their descriptions), the country rules sentence (`auth:onboarding.areas.rulesTitle` as a caption when a country is set), primary `common:buttons.continue`.
    - `FirstAccountStep`: title/lead `auth:onboarding.firstAccount.title` / `firstRun.firstAccountLead` ("Stačí název a zůstatek. Výpis naimportujete později."), name field, `AmountField` (unit = chosen currency, `negative` toggle), `KeyValueRow` Měna → `CurrencyPickerSheet`, primary `auth:onboarding.firstAccount.create`, text link `auth:onboarding.firstAccount.skip`.
    - `BiometricsStep` (only when in `steps`): the 76 pt icon square, title `biometrics.titleFace|titleTouch|titleGeneric`, lead `biometrics.lead*`, primary `biometrics.enable` → `enable.mutate()` then next; text link `biometrics.notNow` → `markEnrolmentAsked` + next.
    - `BackupStep`: title `firstRun.backupTitle` ("Zálohujte si data"), lead `firstRun.backupLead` ("Telefon je jediná kopie vašich dat. Uložte šifrovanou zálohu do iCloud Drive nebo vybrané složky; Moony ji nikdy nevidí."), primary `settings:data.backupNow` → `createBackup()` then `finish()` whatever the outcome (a cancelled picker toasts `backup.notSaved` and the Údržba card keeps asking), text link `biometrics.notNow` → `finish()`.
  - `CurrencyPickerSheet({ open, onOpenChange, value, onPick, options?: readonly CurrencyCode[] })` — full sheet with `SearchField` filtering code and name, `PickRow`s (label `code · name`), check on `value`.
  - `CountryPickerSheet({ open, onOpenChange, value, onPick })` — `PickRow`s from `countryOptionsFromPacks` plus the none row first.

- [ ] **Step 1: Write the failing tests** (`first-run.test.ts`)

```ts
it('lists seven steps with biometry and six without', ...)
it('walks forward and back; no way back once the database exists', () => {
  const steps = firstRunSteps({ biometryAvailable: true });
  expect(nextStep(steps, 'privacy')).toBe('account');
  expect(previousStep(steps, 'account')).toBe('privacy');
  expect(previousStep(steps, 'areas')).toBeNull();
  expect(nextStep(steps, 'backup')).toBeNull();
});
it('numbers the steps from one', () => expect(stepIndex(firstRunSteps({ biometryAvailable: false }), 'backup')).toEqual({ index: 6, total: 6 }));
```

- [ ] **Step 2: Run the test to verify it fails** — `npx vitest run src/mobile/lib/first-run.test.ts`.

- [ ] **Step 3: Implement `first-run.ts`**, then the hook, the steps, the sheets, `FirstRun.tsx`, the one-line swap in `Lock.tsx`.

- [ ] **Step 4: Run the test to verify it passes**

- [ ] **Step 5: Strings** (`firstRun.*`): `privacyLead` "Moony ukládá vše do šifrované databáze v tomto telefonu. Žádný cloud, žádný účet u nás.", `recoveryLead` "Jediná cesta k datům, když heslo zapomenete. Uložte ho mimo tento telefon: do správce hesel, do Souborů nebo na papír.", `saveToFiles` "Uložit do Souborů", `areasLead` "Vyberte, co chcete v Moony sledovat. Změnit to můžete kdykoli v Nastavení.", `firstAccountLead` "Stačí název a zůstatek. Výpis naimportujete později.", `stepLabel` "{{index}} / {{total}} · {{step}}", `steps.biometrics` "Biometrie", `steps.backup` "Záloha", `backupTitle` "Zálohujte si data", `backupLead` "Telefon je jediná kopie vašich dat. Uložte šifrovanou zálohu do iCloud Drive nebo vybrané složky; Moony ji nikdy nevidí.", `currencyPick` "Hlavní měna", `countryPick` "Země bankovnictví", `searchCurrency` "Hledat měnu".

- [ ] **Step 6: Gate and commit**

Run: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`.
```bash
git add src/mobile/pages/FirstRun.tsx src/mobile/pages/first-run src/mobile/hooks/use-first-run.ts src/mobile/lib/first-run.ts src/mobile/lib/first-run.test.ts src/mobile/components/StepProgress.tsx src/mobile/sheets/CurrencyPickerSheet.tsx src/mobile/sheets/CountryPickerSheet.tsx src/mobile/pages/Lock.tsx src/i18n/locales
git commit -m "feat(mobile): phone first run with 44 pt controls, Uložit do Souborů, the Biometrie and Záloha steps"
```

---

### Task 5 (optional, last): iOS app shortcuts

Skipped with a hand-over note when the budget runs out. Design when built:

**Files:** `src-tauri/Info.ios.plist` (`UIApplicationShortcutItems`: `com.moonyfinance.mobile.record-balances`, `.import`, `.add-transaction`, titles localized through `InfoPlist.strings` later — English titles now), `src-tauri/gen/apple/moony_iOS/Info.plist` (the same keys, regenerated by hand as commit 2d2c8d7b did), `src-tauri/plugins/moony-keychain/ios/Sources/MoonyKeychainPlugin.swift` (in `load`: add `application:performActionForShortcutItem:completionHandler:` to the app delegate's class at runtime with `class_addMethod` when it is missing, and the handler `trigger("shortcut", data: ["type": item.type])`; no new command, no permission change), `src/mobile/shell/MobileShell.tsx` (`listen('shortcut')` from `@tauri-apps/api/event` — the plugin event name is `plugin:moony-keychain:shortcut`? — verify with `Plugin.trigger` docs; route: record-balances → Přehled with the record sheet open, import → `routes.finance`, add-transaction → `routes.finance`), `src/mobile/lib/shortcuts.ts` + test (`shortcutRoute(type): { path, action }`).

**Verification** needs a cold launch from the icon's long-press menu on the simulator (a hand step through the simulator tool) — recorded as unverified if the long press cannot be driven.

---

### Task 6: Gates, iOS build, simulator verification, screenshots, README §13, final review, hand-over

Run by the controller.

- [ ] **Step 1: Gates at the head**: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`; `cd src-tauri && cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test backup`, `cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`; `grep "libc" src-tauri/Cargo.lock` shows `0.2.189`.
- [ ] **Step 2: Disk check** (`df -h /` ≥ 25 GB), then the ONE iOS build from this worktree in the background: `npm run tauri -- ios dev "iPhone 17 Pro" --config '{"build":{"devUrl":"http://localhost:1430","beforeDevCommand":"npm run dev -- --port 1430 --strictPort"}}'`.
- [ ] **Step 3: Simulator checks** (UDID `E3EF021E-F419-4E45-96E4-E305A5B1B326`, demo profile Alex Novák / `demo1234`): unlock → avatar → Settings root (sub-lines) → Zabezpečení (switch biometrics on with Face ID, change lock to 5 min, hide on launch) → Připomínky (toggle a group, see Údržba follow) → Oznámení (allow, test notification banner screenshot, day picker) → Záloha (Zálohovat nyní → Files picker → save to On My iPhone; the file lands; last backup date updates; cancel path leaves no Documents file) → restore round trip (pick the saved zip → sheet → restore → lock → unlock with `demo1234`) → language sheet → Smazat všechna data (typed word) → first run with the seven steps (test values only), Uložit do Souborů, Biometrie, Záloha → restore the demo profile afterwards (container app-data folder backed up to the scratchpad first). Screenshots `docs/screenshots/mobile/settings-01…NN.png` via `xcrun simctl io <UDID> screenshot`.
- [ ] **Step 4: README §13** rows (settings row, section page, step progress, day/currency/country/language pickers, restore sheet, delete sheet) and two paragraphs: "Backup export on iOS" (staged in Documents, `save` exports, `finish_backup_export` records and removes) and "Notifications" (planned on unlock, local digits, 09:00, cap 60).
- [ ] **Step 5: Final whole-branch review (opus)** against `feat/mobile` (`693a3ca5..HEAD`), one fix round, re-review (sonnet).
- [ ] **Step 6: Hand-over** `.superpowers/sdd/2026-10-17-mobile-phase-5-settings/handover.md` (Czech) + a copy in the shared notes folder; `pkill -f "tauri ios dev"`, `pkill -f "vite --port 1430"`.

---

## As built / rulings

Recorded as the tasks land; the first set from planning:

1. **Settings stays a pushed page, sections are pushed pages under `/settings/:section`.** Phase 1 made `/settings` a route with a back button to Přehled; the spec's "sheet from the avatar" with "sections, each a pushed page" is honoured by the rows and the pushed sections, and Android back pops them one by one. Costs the sheet's slide-up look if the owner wants it literally.
2. **Device-level preferences in `localStorage`**: background lock minutes (`moony-background-lock-minutes`, Phase 1), hide on launch (`moony-hide-amounts-on-launch`, Phase 1) and the notification preferences (`moony-notifications`). Which phone rings is a per-device matter and no `app_config` command exists for free keys; the spec's "notification preferences in `app_config`" (§11) is deferred to the sync design. Costs a migration of three keys if they move into the profile.
3. **Main currency is read-only on the phone** (the brief); the row says where to change it. Costs one picker if the owner wants it editable here.
4. **Change password is not in Phase 5** (the brief's Zabezpečení list has the biometric switch, the lock delay and hide on launch). Follow-up.
5. **Biometric unlock switched off in Settings keeps R23**: the enrolment sheet does not re-offer after an explicit "off" (the "asked" flag stays), it re-offers only after an invalidation or a new profile. Costs nothing.
6. **Backup on iOS is manual, through the Files export picker**; no automatic cadence to a picked folder. The dialog plugin returns no persistent (security-scoped bookmark) access to a folder, so an automatic backup would need native code; the `backup_stale` reminder and its notification are the cadence. Follow-up with a note for the owner.
7. **Two mobile-only Rust commands for the export** (`stage_backup_export`, `finish_backup_export`): staging through the existing `create_backup` would record a backup that the person may then cancel in the picker, and removing the staged file from the webview would widen the fs scope to `Documents/*.zip`. Costs two thin commands and one iOS build.
8. **The recovery key file is staged by the fs plugin** under a one-file scope (`$DOCUMENT/moony-recovery-key.txt`) and removed in `finally`; the key is already in the webview (the box shows it), so no Rust is needed. A crash between write and remove leaves the file in Documents (iOS data protection, inside the sandbox) until the next export overwrites it — noted in the hand-over.
9. **Notification times are 09:00 local**, T−14 and T−1 on the action day (notice deadline) or the due day, the monthly upkeep on a chosen day 1–28, the backup reminder the morning after a stale backup is found; everything replanned on every unlock (cancel all, schedule the plan). The plugin's date parsing (local digits, literal `Z`) is encoded once in `scheduleDateString`.
10. **The phone first run is its own component tree** (`FirstRun.tsx` + steps) and the desktop wizard is untouched; the flow logic is copied into `use-first-run.ts` rather than extracted from the desktop component (extracting would touch the desktop and the two UIs differ in steps). Costs ~150 duplicated lines of flow; a later shared hook is a refactor, not a behaviour change.
11. **The Záloha step of the first run makes the first backup now** instead of "switching on" an automatic backup (ruling 6). Wording per spec D7 ("Telefon je jediná kopie vašich dat…").
12. **R25 is not decided here**: nothing excludes the app data dir from iOS device backups; the Záloha section carries one neutral sentence; both options with consequences go to the owner in the hand-over.
13. **`mobile.html` gets no new frames**; the new screens are the existing components (entity rows with chevrons, switch rows, segment, key-value rows, sheets) — the 360 check is done in the browser pane during Task 6 and recorded in the ledger.
14. **Shortcuts (Task 5) are the last task and are skipped with a design note when the budget runs out**; its verification needs a cold launch from the icon menu that the simulator tool may not drive.
