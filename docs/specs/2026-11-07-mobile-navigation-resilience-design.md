# Phone: navigation and resilience

Date: 2026-11-07 · Audit item 4 (`docs/audits/2026-11-06-mobile-ux-audit.md`: C3, C4, C5, C6, with
C7, C8 and two C11 bullets) · Reference: the desktop (owner, 2026-11-06) · The owner delegated the
decisions; the calls marked **(call)** are mine.

## Why

The phone loses the person's place and work in ordinary use, and hides failures:

- **C3:** every unlock lands on Přehled, even after a minute in the background. A tab tap always
  opens the tab's root, so "account → glance at Cashflow → back" loses the account. A tap on the
  active tab pushes its root again and does not scroll up.
- **C4:** a notification opens the app and nothing else. Spec §6 and §9 say it opens its target.
- **C5:** a tap on the scrim, a drag or back closes a half-filled sheet without asking. On Android,
  back does not close a desktop dialog shown as a sheet; it pops the screen under it (spec §5:
  "closes a sheet, … never leaves the app from a sheet").
- **C6:** a failed load reads as "no accounts yet" or "not found", with no way to try again.
- **C7:** on Android the backup row, notification and first-run step lead to a disabled button.
- **C8:** the new recovery key from "Zapomenuté heslo?" can only be copied.
- **C11:** toasts sit above the tab bar on screens without one. The lock has no "Obnovit ze zálohy",
  which the desktop lock has.

## Goals

1. After an unlock the phone is where it was: the same screen, scrolled where it was.
2. Each tab keeps its own place for the session. Re-tapping the active tab goes to its root, or
   scrolls the root to the top.
3. A notification tap opens its target, after the unlock when the app is locked.
4. A sheet holding typed input asks "Zahodit změny?" before an implicit dismissal.
5. Android back closes every sheet, the desktop dialogs on the phone included.
6. One "error + Zkusit znovu" component; an error is never shown as empty or not found.
7. The C7, C8 and C11 fixes listed above.

## Non-goals

- **Home Screen quick actions / app shortcuts** (spec §6, §9). They need native code (Info.plist
  shortcut items and an app-delegate hook, an Android shortcut XML). This is a follow-up for the
  owner-side native phase. Spec §6 and §9 stay as the target.
- **Keeping an open sheet or its draft through a lock.** The lock unmounts every screen and clears
  the query cache. Keeping a draft would mean keeping decrypted input alive behind the lock.
- **Notification taps that start the app from cold.** The plugin delivers a tap only to a running
  process (see Risks).
- **The dirty guard on desktop dialogs.** Turning those forms into phone forms is audit item 5
  (C1). They do join the back handling here.
- **Android notification scheduling.** It stays iOS-only (`schedulesOnThisDevice`). The target
  still rides in `extra`, so Android works once scheduling does.

## Design

### Lock return (C3)

- `MobileShell` keeps, in a module store (`lock-return.ts`), the path it last showed while
  unlocked. When the app locks on a shell path, it records `{ path, depth }`, where `depth` is 1
  (the `/auth` entry the redirect pushes) plus the sheet history entries open at that moment.
  Their sheets unmount with the screen, but their entries stay in the history.
- While the walk runs (`returnWalk`, from its `history.go` until its `popstate` has been handled
  everywhere), nothing else touches the history: a sheet that opens meanwhile pushes its entry
  once the walk has landed, the walk's `popstate` closes no sheet, and the Face ID offer and the
  shared-statement import wait. A walk that lands after the fallback is undone by the same steps
  forward.
- After a successful unlock, `Lock` takes the record and goes back `depth` entries. The history and
  the nav stack are then exactly as they were before the lock: the screen comes back with its kept
  scroll and its back label. If no `popstate` arrives, or the walk lands on `/auth` again, `Lock`
  replaces `/auth` with the recorded path instead.
- Without a record (a cold start, the first run), `Lock` replaces `/auth` with Přehled. It no
  longer pushes, so back from Přehled does not bounce through the lock.
- The record is cleared when there is no profile (`needs_setup`). It is read only by the unlock
  path, so the lock is never weakened.

### Tab memory (C3)

- Pure (`tab-memory.ts`):
  - `tabTrail(stack, tab)`: the top run of nav entries that belong to `tab`, starting at the tab's
    last root entry when the run has one.
  - `tabTapPlan(stack, tab, memory)`: one of `scrollTop`, `popTo(index)`, `push(entries)`.
- `MobileShell` updates the memory of the top screen's tab on every stack change. The memory lives
  for the app session only and is never written to storage (as `view-memory.ts`).
- A tap on another tab pushes the remembered trail: root first, then each pushed screen. Back then
  walks the tab's own stack. The kept scroll of each screen is restored. An empty memory pushes the
  root.
- A tap on the active tab:
  - at its root: scrolls the root to the top (an event that `Screen` listens to);
  - deeper, with the root below in the trail: goes back to it in one `history.go`. `navReduce`
    learns a pop with a known target (`pop` with `to`), so the nav stack drops every screen above
    the root in one step;
  - deeper, entered from another tab (no root in the trail): pushes the root.

### Notification targets (C4)

- `buildNotificationPlan` gives every item a `path`:
  - a deadline: `milestonePath(m)`, else Co vás čeká;
  - the monthly upkeep: Přehled;
  - the backup: Nastavení → Záloha.
- The scheduler sends `extra: { path }`. Android returns `extra` on a tap. iOS does not (the
  plugin's `ActiveNotification` has no `extra`), so the scheduler also keeps an in-memory map from
  id to path.
- `NotificationTargetHost` (mounted always, like `ReceivedFileHost`) listens with `onAction`
  (`actionId === 'tap'`). It accepts only shell paths (`isShellPath`) and holds the target in a
  module store. While unlocked and away from `/auth`, it takes the target and pushes it. While
  locked, `Lock` takes it after the unlock and replaces `/auth` with it, so back goes to the screen
  before the lock.
- When the listener cannot be registered (desktop shell, a platform without the plugin), the
  failure is swallowed: the app opens as it does today.

### Dirty sheets and Android back (C5)

- `createSheetHistory`: a sheet's `close` may refuse (`false`). The store then pushes the sheet's
  entry again, so the sheet keeps owning one entry.
- `Sheet` gets `dirty?: boolean`. When it is dirty, the scrim, Escape, a drag and back open a nested
  confirm sheet instead of closing:
  - title "Zahodit změny?", with one sentence;
  - buttons "Pokračovat v úpravách" and the destructive "Zahodit".

  "Zrušit" in the sheet's footer closes without asking.
- `Dialog` on the phone wraps Radix `Root` and joins `useSheetHistory`. A dialog that withholds its
  close (`hideCloseButton`, it is busy) refuses back.
- `formChanged(start, now)` (pure, `form-dirty.ts`) is the dirty test of the form sheets.

### Error states (C6)

- `loadState({ isLoading, isError, hasData })` (pure) returns `loading | error | notFound | ready`.
  An error with data already shown stays `ready`: a failed refetch keeps the last answer.
- `LoadError` is the phone's one "failed + Zkusit znovu" block: a sentence and a secondary 48 pt
  button, `role="alert"`.
- It is used on Přehled (the metrics), Finance (accounts, loans, policies, the queue), Majetek (the
  metrics) and every detail: account, loan, policy, position, coin, property, other asset and plan.
  The class pages' `LoadFailed` gains the retry.
- Each detail hook returns `isError` and `retry` next to `notFound`. `notFound` is now
  `!isLoading && !isError && !entity`.

### C7, C8, C11

- **C7:**
  - `buildUpkeepRows` leaves out the backup row while `canBackup` is false.
  - `buildNotificationPlan` leaves out the backup reminder.
  - `firstRunSteps` drops the backup step. The flow finishes when no step follows.
- **C8:** the recovered key's screen gets "Uložit do Souborů" (iOS), using the change-password
  file name and title. Both are a key replacing the old one.
- **C11 toasts:** the shell sets `data-tabbar` on `<html>` while the tab bar shows, and the toast
  offset lifts by `--m-toast-lift` only then.
- **C11 restore:** the lock's password view gets the desktop's "Máte zálohu? Obnovit ze zálohy…".
  It uses `useRestoreBackup` and `RestoreBackupSheet` (not `fresh`: a profile is replaced).

## Decisions

- **(call) Every lock returns to the screen, the explicit lock button included.** The desktop has
  no back stack to compare with. Returning is the less surprising half. Cost if wrong: one tap on
  Přehled after an explicit lock.
- **(call) The return walks back the history instead of pushing the path again.** This keeps the
  back stack, the back label and the scroll exactly. Cost if wrong: a history out of step lands
  elsewhere. The fallback then replaces `/auth` with the recorded path, so the person is never
  stuck on the lock page while unlocked.
- **(call) The tab memory is the trail of screens, not only the last path.** Back from a restored
  account then goes to Finance, as on iOS, not to the tab the person came from. Cost: a deep trail
  is pushed as several entries. Android back walks them, which is the tab's own stack.
- **(call) Re-tapping a tab that was entered from another tab pushes its root.** There is no root
  below to go back to. Cost: one extra history entry in that rare case.
- **(call) Upkeep notifications open Přehled.** Its Údržba card carries every due action: balances,
  queue, import and backup. Cost: one tap more than a direct import sheet. The import needs an
  account picked first anyway.
- **(call) The dirty guard asks only on implicit dismissals.** "Zrušit" is a deliberate choice, as
  on the desktop, where no dialog asks. Cost: a mis-tap on Zrušit still discards.
- **(call) The guard covers the phone's form sheets** whose draft costs typing: Zaznamenat zůstatky,
  Přidat transakci, Dividenda, Náklad, Nové aktivum, Výdajový plán, Přecenění. Pickers and
  confirmations are excluded. Cost: other sheets still close silently.
- **(call) The notification target replaces the lock entry.** Back from it goes to the screen
  before the lock. Cost: none found.
- **(call) A detail's "not found" test is screen-wide.** `MissingOrFailed` shows the error when any
  active query of the screen failed without data (other than the backend's `notFound`), not only
  the detail's own queries: their keys differ per hook and are not exposed. Cost if wrong: a
  shell-level query failing at the same moment turns a real "not found" into an error with a
  retry, the safer misreport.
- **(call) The restore link lives on the password view of the lock,** under "Zapomenuté heslo?", as
  on the desktop. The biometric view reaches it through "Použít heslo". Cost: one tap more with
  Face ID on.

## Risks for the device check

- **The iOS plugin can crash on a notification tap.** `NotificationHandler.toActiveNotification`
  force-unwraps `notificationsMap[request.identifier]`. That map is filled only by scheduling in
  the running process. A tap on a notification scheduled by an earlier process (the app killed,
  then relaunched by the tap, before the first unlock reschedules) can crash. This needs a check on
  the iPhone. A fix would patch the plugin, which is out of scope here.
- Android back with a desktop dialog: this needs an Android build (audit "Ověřit na zařízení" 5).
