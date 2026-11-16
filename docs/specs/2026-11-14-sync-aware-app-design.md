# An app that knows about sync — design

Date: 2026-11-14. Status: **draft, waiting for the owner's approval.**

Device sync works (own sync server, ADR 0024–0029), but the rest of the app behaves as if it did
not exist: the first run cannot join it, backups are still demanded, the phone shows its state
only in Settings, and the wording still describes a single-device app. The owner asked for a UX
review on 2026-11-14 and then for a design of everything it found **(owner)**.

Decisions the owner made are marked **(owner)**. Decisions Claude made are marked *(my call)* and
listed again in §11. Wording is given in Czech as a starting point; final copy in all ten locales
is the builder's (AGENTS.md rule 8).

Companion: `2026-11-14-sync-attachments-design.md` (documents and photos sync). Part C below
depends on it; parts A and B do not.

## 1. Why

- A person with Moony on a computer who installs it on a second device goes through the whole
  first run (currency, password, recovery key, areas, first account), then has to find
  Settings → Sync and agree to replace the data they just created.
- With sync on, the same data is on several devices and on the server, yet the app keeps asking
  for a manual backup every 30 days, in the agenda, on the phone's Overview and by notification.
- The desktop shows the sync state in the top bar as a label that only links to Settings; the
  phone shows it nowhere but in Settings. A sync that has been stuck for two weeks is invisible
  on the phone.
- A server that is reachable only at home is reported as a red "Offline" every time the person
  leaves the house.
- Settings → Sync opens with three equal buttons that require knowing the difference in advance.

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. A new device can join an existing sync account from the first run, in the fewest steps
  that keep the device's own password and recovery key.
- G2. The sync state is visible where the person already looks, quiet when all is well and
  clear when something needs them, on both platforms.
- G3. With healthy sync the app stops asking for manual backups; when sync stops protecting the
  data, the app says so.
- G4. Every text that explains where data lives is true with sync on and with sync off.
- G5. A person can tell the secrets apart (device password, device recovery key, Sync Recovery
  Kit, connection code).

**Non-goals**

- Changing the sync protocol, pairing, the Recovery Kit or any audited behaviour of ADR
  0024–0029, except the two small backend additions named in §5.4 and §7.2.
- Merging two existing profiles (D22 stays).
- Removing a lost device (phase 5), the Moony cloud, payments.
- Automatic scheduled backups.
- Background sync on the phone, and notifications about sync.
- Spreading a changed server address to other devices automatically.
- Flipping `feature::RELEASED`. Everything here stays behind the sync flag where it concerns
  sync.

**Anti-goals**

- A first-run join that replaces data without the person having been told, or that leaves a
  profile half created with no way forward.
- Telling a person their data is safe when a part of it exists on one device only.
- A sync indicator that is alarming in a normal situation, or calm while sync has stopped.

## 3. Approaches

**Joining from the first run**

1. **Create the device's profile first, then join inside the wizard (chosen).** The join path
   keeps the password and recovery-key steps, then runs the existing pairing or Recovery Kit
   join as the last step. No change to the join protocol; the backend only learns that a
   brand-new profile has nothing to protect.
2. **Join before any profile exists.** Fewer steps on screen, but the join needs a database and a
   device password to store the vault credentials, so the backend would have to create a
   profile silently and the device would end up without a recovery key of its own. It reopens
   audited code.
3. **Leave the wizard alone and point to Settings afterwards.** Cheapest, and it keeps today's
   detour and the "replace your data" warning for data that is ten seconds old.

**Backups with sync on**

1. **One rule: the newest safe copy (chosen).** The backup reminder looks at the later of the
   last manual backup and the last complete sync. Healthy sync keeps it silent; a stalled sync
   brings it back by itself.
2. **Lengthen the reminder to 90 days with sync on.** Simple, but it still nags people who are
   protected and says nothing sooner when sync has stopped.
3. **Turn backup reminders off whenever sync is on.** Silent even when sync has been broken for
   months.

Both choices are *(my call)*; the owner asked for the outcome.

## 4. Order

- **Part A (first run, entry to sync)** and **part B (status)** stand alone and can be built
  now.
- **Part C (backups, wording)** claims that sync protects everything, which is true only once
  documents and photos sync. It is built after the attachments design. If the owner wants C
  earlier, the one rule of §7.1 must treat a profile that holds attachments as unprotected.

## 5. Part A — getting into sync

### 5.1 First run on the desktop (`components/auth/OnboardingWizard.tsx`)

The first step keeps its content. Its footer, today "Používáte Moony už na jiném počítači nebo ho
přeinstalováváte? Obnovit ze zálohy", becomes:

```
Už Moony používáte na jiném zařízení?
  [ Připojit k synchronizaci ]   [ Obnovit ze zálohy ]
```

"Připojit k synchronizaci" is shown only when the sync flag is on. It switches the wizard to the
join path, whose steps are:

1. **Soukromí** (the step the person is on; currency and country are not asked for again and
   whatever they hold is overwritten by the synced profile).
2. **Heslo pro toto zařízení.** The account step with the password fields only: name, surname
   and e-mail come from the synced profile. A lead says the password is this device's own and
   may differ from the other devices'.
3. **Obnovovací klíč** as today (it is this device's key).
4. **Připojení.** Replaces "Oblasti" and "První účet":

```
Připojit toto zařízení

Název zařízení  [ Mac                 ]

(•) Mám u sebe telefon se synchronizací
    Zobrazí se kód. V telefonu: Nastavení → Synchronizace → Přidat zařízení.
( ) Mám Záchrannou sadu synchronizace
    Zadáte kód ze sady a adresu serveru. Tak se připojuje i druhý počítač.

                         [ Pokračovat bez synchronizace ]   [ Pokračovat ]
```

   - The two choices run the existing flows (the content of `JoinSyncDialog` and
     `RecoveryJoinDialog`, shared with Settings, not copied), including the six-digit check, the
     http warning and the server check.
   - The device password is not asked again *(my call)*. Starting a pairing and joining with a
     Recovery Kit both take the device password (`sync_join_with_recovery_kit` as a parameter,
     pairing when it starts); the wizard keeps what the person typed in step 2 in memory for
     that, and drops it when the Připojení step ends either way.
   - What the synced profile brings and what stays: name, surname, e-mail and currency are
     synced columns of `user_profile` and come from the vault; `language`, `theme` and
     `menu_preferences` are local columns, so the language picked in the first run stays, as in
     a join from Settings today.
   - There is no way back from Připojení (the account exists, as on the Oblasti step today). If
     the app is closed during the step, the next launch does what it does today for a first run
     interrupted after the account was created; the join stays available in Settings →
     Synchronizace. Nothing new is stored to resume the step.
   - Because the profile is brand new, the step shows no "data will be replaced" warning, no
     list of what is at stake and no note about a kept backup (§5.4).
   - While data is being fetched the step shows progress and "Nezavírejte Moony"; on success the
     wizard ends on the Overview with a toast "Toto zařízení se teď synchronizuje".
   - "Pokračovat bez synchronizace" and any failure the person does not want to retry lead to
     the normal remaining steps (Oblasti, První účet); the profile already exists, so nothing is
     lost. A join that has received its credentials and failed for the moment keeps the existing
     "Zkusit znovu".
   - The menu after a join is the default one (everything shown); the person hides areas in
     Settings as today *(my call)*.

### 5.2 First run on the phone (`mobile/pages/FirstRun.tsx`, `mobile/lib/first-run.ts`)

Same change on the first step ("Připojit k synchronizaci" next to "Obnovit ze zálohy"). Join
path: Soukromí → Heslo → Obnovovací klíč → **Připojení** → Biometrie. The Připojení step offers
"Naskenovat kód z počítače" (lead: on the computer, Settings → Synchronizace → Přidat zařízení;
the same scanner also takes a Recovery Kit QR) and "Zadat kód Záchranné sady". The last step
"Záloha" is left out after a successful join.

For a person who does **not** join, the last step changes from "Zálohujte si data" to "Ochraňte
svá data" with the backup as the main button and one quieter line: "Máte synchronizační server?
Nastavte synchronizaci". The line is on that last step, next to "Teď ne": it finishes the first
run without a backup and opens Settings → Synchronizace instead of the Overview.

### 5.3 Settings → Sync while sync is off (desktop `SyncIntroCard`, phone `SyncSection`)

Two choices instead of three buttons:

```
Začít synchronizovat
  Toto zařízení bude první. Potřebujete připojovací kód svého synchronizačního serveru.
  Jak si server spustit ↗

Připojit toto zařízení
  Synchronizaci už máte na jiném zařízení.
```

- "Připojit toto zařízení" opens one dialog (a sheet on the phone) whose first screen is the
  choice of §5.1 step 4 (phone or Recovery Kit; on the phone: scan or type), then the existing
  flow. Here the profile may hold data, so the replace warning and the impact list stay.
- A setup that was started and not finished keeps "Pokračovat v nastavení" and "Zrušit
  nastavení" in place of "Začít synchronizovat".
- "Jak si server spustit" opens the server's installation guide in the browser. Its address is
  the owner's to give (§11); until then the link is left out.

### 5.4 Backend addition: a join into a brand-new profile

A join always writes a verified backup and marks it kept for good. For a profile with nothing in
it that is noise the person has to delete by hand.

- The backend decides whether a profile is brand new: in the replace impact it already computes
  (`services/sync/impact.rs`, `ReplaceImpact`) `accounts`, `transactions`, `documents`, `photos`,
  `pending_items` and `pending_batches` are all zero. `changes_through` is a watermark, not a
  count, and is not part of the test. A profile that never had sync on has no change capture, so
  what profile creation seeds (the profile row, categories) is not counted. The frontend never
  asserts it.
- For such a join the backup is still written, so the rule "no replace without a backup" has no
  exception, but it is not marked kept and may be pruned like a re-sync's backup that holds
  nothing unique *(my call)*.
- The join's consent check stays as it is: the wizard passes the zero impact it was shown.

## 6. Part B — status

### 6.1 States

One mapping for both platforms, in `src/utils/` (today `utils/device-sync-indicator.ts` and
`mobile/lib/sync-summary.ts` each have their own; both platforms read the same `SyncStatus` from
the same command):

| Situation | Label | Tone |
|---|---|---|
| Up to date | Synchronizováno | fresh |
| A round is running | Synchronizuji… | neutral |
| Server cannot be reached, last success under 7 days ago | Server mimo dosah | neutral |
| Server cannot be reached for 7 days or more | Nesynchronizováno N dní | stale |
| A change from another device is held | Změna je zadržena | stale |
| Paused for another reason | Synchronizace pozastavena | stale |
| Stopped by an error | Chyba synchronizace | stale |

- The 7 days are *(my call)*. Until then an unreachable server is normal for a server at home;
  the settings page says "Synchronizace se obnoví, až bude server v dosahu" without an alert
  box, and shows how many changes wait.
- Sync off or being set up: no indicator, as today.

### 6.2 Desktop top bar

`SyncStatusIndicator` becomes a popover like the price status next to it (`DataStatus`):

```
Synchronizace
Synchronizováno · před 4 min
Čeká na odeslání      0
Dokumenty a fotky     vše nahráno            (attachments design)
Server                192.168.1.2:8787 · nešifrované spojení
[ Synchronizovat ]            Nastavení synchronizace →
```

- With a problem, the first line is the problem's short text and the button is "Otevřít
  nastavení"; actions that need the device password stay in Settings.
- "Synchronizovat" runs a round and the popover shows its result.

### 6.3 Phone

- Settings row: the state plus the age, "Aktuální · před 5 min".
- Overview, Údržba card: one row when sync needs the person (the three `stale` rows of §6.1 and
  the stalled case of §6.4), with "Otevřít" leading to Settings → Synchronizace. Nothing is
  shown while sync is healthy or merely out of reach.

### 6.4 A reminder when sync has stalled

A new upkeep milestone kind `sync_stalled` (`services/milestones/upkeep.rs`), produced when sync
is on for the profile and either

- no round has succeeded for 7 days, counted from the last success or, when there has been none
  yet, from the moment sync was turned on on this device (so a device never gets the reminder on
  its first day; if that moment is not stored today it is recorded with the value of §7.2), or
- sync is paused or stopped by a problem that needs the person (every `stale` row of §6.1 except
  the unreachable server): at once, without the 7 days.

It shows in the desktop agenda and top-bar count and feeds the
phone's Údržba row, with the same hide, snooze and mute as other upkeep kinds. It reads from
`sync_state` in the profile's database; it is produced in every build, including one without the
sync flag, only if the builder finds that harmless (otherwise behind the flag).

### 6.5 Smaller status items

- Devices card: the standing paragraph about a lost device moves under a disclosure "Ztratili
  jste zařízení nebo Záchrannou sadu?".
- Change address: after success the dialog lists the other devices by name: "Totéž udělejte na:
  iPhone, Windows PC".
- A row "Co se synchronizuje" in Settings → Synchronizace (both platforms), opening a short
  text: everything entered into Moony syncs; the language, look, menu, notifications, device
  password, recovery key and backups belong to each device; prices and history are worked out
  on each device. Until documents and photos sync, the text says they stay on the device that
  added them.

## 7. Part C — backups and wording (after the attachments design)

### 7.1 One rule for the backup reminder

- **Newest safe copy** = the later of the last manual backup on this device and the last
  complete sync. A sync is complete when a round succeeded and nothing waits to upload,
  documents and photos included.
- `backup_stale` is produced when the newest safe copy is older than 30 days. A new profile's
  14 days of grace stay.
- Consequences without further rules: healthy sync never asks for a backup; a sync that stalls
  first raises `sync_stalled` (7 days) and, if it stays stalled and there is no backup, the
  backup reminder returns at 30 days; the phone's backup notification follows because it is
  driven by the same milestone.
- With sync on the reminder's text names both ways out: restore sync, or make a backup.

### 7.2 Backend addition

The milestone service reads the time of the last complete sync. `sync_state.last_success_at`
says only that a round succeeded, so the engine records one more local, unsynced value in
`sync_state` *(my call: an explicit value, not a derivation)*: the time of the last round that
succeeded and left nothing waiting (`sync_outbox` and `sync_unsent` empty, and no document or
photo waiting to upload, as the attachments design defines it). It comes with the next
append-only migration.

### 7.3 Where backups stay and what they say

- Backup and restore stay where they are and work as today. Settings → Data a zálohy (desktop)
  and Záloha a přenos (phone) get a line above the backup row while sync protects the data:
  "Vaše data chrání synchronizace (naposledy dnes 14:02, 3 zařízení). Zálohu si udělejte, když
  chcete uchovat stav k určitému dni." It is honest about the limit: a deletion made by mistake
  syncs too.
- Getting started (desktop and phone): "Udělat zálohu" becomes "Ochránit data", done when a
  backup exists **or** sync is on; its hint names both.
- Phone first run, last step: §5.2.

### 7.4 Wording that must change

| Where | Today | Becomes |
|---|---|---|
| First run, privacy points (desktop `auth` `onboarding.welcome.points`, phone `mobile` `firstRun.points`) | data is never sent anywhere | data leaves the device only if the person turns sync on, and then end-to-end encrypted to a server they choose |
| Phone, Záloha a přenos lead and `backup.restoreLead` | a backup file is how a profile moves between the computer and the phone | sync keeps devices in step; a backup file is for a one-off move or a copy to keep |
| Phone, `backup.osBackupNote` and `firstRun.backupLead` | the only copy outside the phone is a backup the person saves | true only while sync is off; with sync on it names sync |
| Sync intro (`deviceSync` `intro.description`) | "data" on all devices | names documents and photos once they sync |
| Delete account / Danger zone | deletes data from this computer | with sync on, says what happens to the synced data and the other devices |
| Repository docs (`README.md`, `PRIVACY.md`, `SECURITY.md`, AGENTS.md "No cloud sync") | no sync | optional end-to-end encrypted sync; changed when sync is released, not before |

### 7.5 Telling the secrets apart

- Settings → Zabezpečení gets a card "Co si uschovat": the device password (unlocks this
  device), the device recovery key (resets the password on this device; each device has its
  own), and, while sync is on, the Sync Recovery Kit (brings the data back when every device is
  lost; one for all devices) with "Zobrazit". The connection code is not listed: it comes from
  the server's admin and can be shown again there.
- The code inside the Kit is called "Kód záchranné sady" everywhere (today "Obnovovací kód" in
  places, one word away from the device's "Obnovovací klíč").
- The problem text `keyRotated` ("Obnovovací klíč byl změněn na jiném zařízení") names the
  right thing: the sync key was changed on another device.

## 8. Rules that apply

AGENTS.md rules 1–10: `invoke()` only in `src/lib/tauri-api.ts`; the `shared/schema.ts`
procedure for changed wire types (the milestone kind, the sync status fields); logic in
`src-tauri/src/services/`; every string in all ten locales; migrations append-only. The design
system (`docs/design-system/README.md`) decides the look; the top-bar popover copies
`DataStatus`, the phone rows copy the Údržba card.

## 9. Left to the builder

- How the join flows are shared between Settings and the first run (components, hooks).
- The exact layout of the Připojení step and of the two-choice entry.
- Whether `sync_stalled` is produced in builds without the sync flag.
- Relative-time wording and refresh in the popover and the Settings row.
- Tests: pure logic gets unit tests (`utils/`, `mobile/lib/`, `services/milestones/`,
  `services/sync/`), per `docs/standards/testing.md`; the flows are checked in the running app
  on the demo profile and, for pairing, on the simulator through the dev bridge
  (`docs/architecture/overview.md`).

## 10. Verification

- Desktop: a fresh profile joins by Recovery Kit from the first run against a local sync server
  and lands on the Overview with the synced data; "Pokračovat bez synchronizace" leads to the
  normal steps; a failed join can be retried.
- Phone (simulator and the owner's iPhone): the first-run join by scanning; the Záloha step is
  absent afterwards.
- Status: each row of §6.1 reached with the relay fake or by stopping the local server; the
  7-day switch with an injected clock.
- Milestones: `sync_stalled` and the newest-safe-copy rule in `upkeep.rs` tests.
- The locale parity test passes.

## 11. For the owner to check

Calls Claude made:

1. The first-run join keeps this device's own password and recovery-key steps.
2. The password typed in the first run is reused for the join in the same session.
3. After a first-run join the menu shows everything.
4. A join into a brand-new profile leaves no kept backup.
5. An unreachable server is neutral for 7 days, then a warning and the "sync has stalled"
   reminder; a pause or stop that needs the person raises the reminder at once.
6. The backup reminder follows the newest safe copy (30 days), so healthy sync silences it.
7. "Udělat zálohu" in Getting started becomes "Ochránit data", done by a backup or by sync.
8. The Kit's code is renamed "Kód záchranné sady".
9. Part C waits for documents and photos to sync.

Open, for the owner:

- The address of "Jak si server spustit" (the server's README, or a page on moonyfinance.com).
- Whether part C should come before the attachments work, with the stricter rule of §4.

## 12. As built (parts A and B, 2026-11-15)

The sections above are the design as approved and are not rewritten. Where the build differs:

- §5.4: "brand new" is stricter than six zero counts. A join's backup settles to plain only when
  the replace impact holds nothing, no watchlist entry, budget goal, spending plan, planned
  cashflow item, stock tag or person-made category or categorization rule exists, and the profile
  is younger than 24 hours (`impact::brand_new_profile`). Reason: the six counts ignore other rows
  a person can make, and a prunable backup holding the only copy of those would be a silent loss.
- §5.2: the last step's new title "Ochraňte svá data" and its "set up sync" line show only with
  the sync flag on; with the flag off the phone's first run is unchanged.
- §6.1: the server counts as out of reach for the problems `offline`, `relayError` and
  `rateLimited`.
- §6.4: the reminder fires by time only while the server is out of reach (7 whole UTC days from
  the last success, or from the moment sync was turned on when there was none); an idle or syncing
  status with no problem never raises it, however old its last success. It is raised at once for a
  pause or stop that needs the person. During a retry round of an unreachable server the reminder
  stays while the top bar shows the running round. The top-bar count stays deadline-only; the
  stall shows through the sync popover, the agenda and the phone's Údržba row. "When sync was
  turned on" is this device's own `sync_devices.joined_at` (`SyncStatus.enabledAt`); no migration
  was needed. The reminder exists only with the sync flag on.
- §6.2: the three "Synchronizovat" buttons cannot start a second round while one runs.
- §5.1: a person who chooses the join path and then continues without sync is asked for name and
  surname on the next step, since the join path creates the profile without them.
- Not done: the phone flows were not run in a simulator or on a device (the desktop was checked
  live against a local sync server); part C (§7) waits for documents and photos to sync.

## 13. As built (part C, 2026-11-16)

The sections above are the design as approved and are not rewritten. Where the build of part C
(plan `docs/plans/2026-11-15-sync-aware-app-part-c.md`) differs from §7 and from the plan's
decisions:

- §7.2, what "complete" means: a round with no problem at all that leaves nothing here to send:
  `sync_outbox` and `sync_unsent` empty, no `up` entry in `sync_blobs` that a row names (stuck
  uploads included: refused, a conflict, an invalid ID), and no attachment row whose blob has no
  queue entry yet. It is read from `main` only (`queue::summary`'s `content_left`). A queue that
  cannot be read counts as content left: the round's success stands and no complete time is
  recorded. The time is written with a plain `SET` (never `MAX`) in the same hold that records the
  round's success, in `sync_state.last_complete_at` (migration 039). A replace keeps it; turning
  sync off forgets it. No round is woken when a lane finishes an upload, so the time arrives with
  the next round, at most five minutes later.
- A profile whose attachments store failed to open, and that has attachment rows, never records a
  complete sync until the store opens again: its content cannot be shown to be on the server.
- §7.3 and plan Decision 4, the verdict (`SyncStatus.protectsData`, `milestones::sync_protects`):
  besides "sync on, `sync_stalled` quiet, a complete sync on one of the last 7 UTC days", nothing
  on this device waits to be sent right now (no unsent change, no upload waiting or stuck, no
  content without a queue entry, no blob held across a replace in `sync_held_blobs`), and no
  problem that needs the person is on record, also while a round runs (a running round keeps the
  last round's problem, and `sync_stalled` is quiet while a round runs; only the problems the
  engine retries by itself may stand). Reason: the complete time is not lowered by what is made
  after it, so without these checks the line could show over a document that exists on one device
  only, or flash on during every retry round of a lapsed account. Cost: the line goes away for the
  seconds between a local change and its upload. Right after unlock the engine has no problem on
  record until the first round ends, so for those seconds the verdict rests on the stored times
  alone.
- The status computes the verdict from the same pass over the attachment rows that gives
  `SyncStatus.attachments` (`queue::summary`). When that pass fails, the figures are empty and
  `protectsData` is false; never empty figures with "protected".
- Plan Decision 5 is changed: a complete-sync time dated after tomorrow (UTC) is ignored by both
  the backup reminder and the verdict (`upkeep::trusted_complete`, one helper for both, so the two
  still never show together). A manual backup time is not capped, so the reminder without sync
  reads exactly as before. The stalled-sync reminder's week also ignores a last success dated
  after tomorrow and counts from when this device joined. Reason: one round with a clock far ahead
  would otherwise silence the reminder for as long as the clock was ahead. A device that simply
  runs ahead is not affected: it stamps and reads with the same clock. Cost: a device whose clock
  was ahead at its last complete round and was corrected afterwards gets no credit for that round
  until its next complete one, at most five minutes later while the server can be reached. During
  an outage right after such a correction the stalled-sync reminder can show at once, counted from
  when the device joined, until the next round that succeeds.
- `SyncAttachments.failedUpload`: the rows among `failed` whose content is here and cannot upload
  by itself (refused, a conflict, an invalid ID), a photo once. Their content is on this device
  only and keeps every sync incomplete until "try again" on the row. The status surfaces show a
  line for them under "Documents and photos", and that row never says "All uploaded" while any
  exist.
- Migration 039 changes only a local column, but it raises the schema number every change carries.
  A paired device on an older build stops at the first change from an updated device and pauses
  with "newer schema" until it is updated too. Update both devices together.
- Wording decided during the build that differs from the text of §7 (the exact strings are in the
  locale files):
  - The protection line says where the copy is (on the sync server) and counts the devices in
    sync, with a colon before the age, so "just now" reads naturally.
  - The phone's backup row in Údržba leads with the number of days without a safe copy; its
    sub-line is a short imperative that fits one line. The desktop sub-line and the notification
    keep the longer wording.
  - Where the reminder says to get sync going again, it uses "resume" or "reactivate" wording, not
    "restore", which reads as restoring data in several languages.
  - The delete texts say the synced data on the server is deleted only when the server can be
    reached; otherwise it stays there, encrypted.
- Known gaps, left for the owner:
  - `delete_account` does not report whether the server data was deleted; the deletion is best
    effort and only logged when the server cannot be reached.
  - No list shows which document or photo has a stuck upload; the status gives only the number.
  - Getting started's "Protect your data" is ticked as soon as sync is on, before the first
    complete sync.
