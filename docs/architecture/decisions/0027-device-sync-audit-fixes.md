# 0027. Device sync: what the security audit found and what was done about it

Date: 2026-11-12

## Status

Accepted. Amends ADR 0024, ADR 0025 and ADR 0026 where named below. Amended by
[ADR 0028](0028-self-hosted-sync-server-in-the-app.md) (the HTTP rule of L9, the allow list). Records the fixes built on
`fix/device-sync-audit` (plan `docs/plans/2026-11-12-device-sync-audit-fixes.md`) and on two relay
branches that are not merged into the relay's main: `feat/pairing-ack` and `feat/dev-grant`. Sync is
still hidden behind the runtime flag. Where the spec or an earlier ADR differs, the code and this
ADR win; the spec's §20 "As built (audit fixes)" lists the corrected claims.

## Context

An independent audit of phases 1–3
([`docs/audits/2026-11-11-device-sync-security-audit.md`](../../audits/2026-11-11-device-sync-security-audit.md),
proofs of concept in
[`2026-11-11-device-sync-audit-poc-tests.patch`](../../audits/2026-11-11-device-sync-audit-poc-tests.patch))
found no Critical issue, 4 High, 13 Medium and 16 Low ones. Its verdict: the cryptography is sound
and a relay without the key cannot read or forge anything, but there is one static vault key with no
author authentication and no revocation (H1), "no path loses data silently" was false in three
places (H2, H3, H4), pairing released the keys on one confirm whose digits a relay operator who also
sees the QR could match (M1, M2), and a relay could stop sync for good and steer the person into a
re-sync that dropped unsent changes (M3, M4).

The owner's scope (2026-11-12, "Yes, fix them"): fix H2–H4, M1–M4, M7–M9, M11–M13 and the cheap
lows; for H1 an honest statement in the UI and a corrected spec now, with removal and rotation left
to phase 5; accept M5, M6 and M10 with corrected docs and cheap hardening. A pre-flight review of
the fix plan found that the first guard design rolled back with a savepoint (C-1) and that keeping
envelopes until they come back would have stalled sealing (I-2); both were redesigned before any
code. Each proof of concept became a regression test that asserts the fixed behaviour.

## Decision

**Capture or guard (H2, M7).** An enabled profile either records every change of its synced data or
refuses to change it (`capture/guard.rs`). When capture cannot be installed at unlock, `sync::on_open`
marks the profile capture-broken and imposes the guard: an SQLite authorizer refuses inserts and
deletes on synced tables of `main`, updates of their synced columns, and `ALTER TABLE` on them.
Local columns, local tables, `sync_devices` (sync code writes it, and so do the person's rename and a re-join; a repair carries
the name forward), the `temp` schema, the
attachment store and the target of `VACUUM INTO` stay writable, so settings, sync's bookkeeping, a
backup, a repair and turning sync off keep working. `app_config` mixes synced and local keys and the
authorizer cannot see keys, so three TEMP triggers refuse the five synced keys; if they cannot be
created, the authorizer refuses all of `app_config`. The guard lives on the connection, not in a
table: a marker function (`moony_sync_guard()`) says it is on, `impose` and `lift` refuse to run
inside a transaction, and a replace or turn-off lifts it before its savepoint and imposes it again
on every failure, so no rollback can leave a profile without both capture and guard. If the guard
itself cannot be imposed, the connection becomes `PRAGMA query_only` (repair then answers
`validation.syncCaptureStillBroken`, and only turning sync off remains). A guarded or read-only
profile counts as capture-broken wherever the mark is read, so it runs no round. A refused write
answers `validation.syncChangesPaused`; the unlock self-heals that write synced columns are skipped
with one warning, and a crypto symbol that cannot be stored skips that holding. The guard covers
one connection, and the app opens one per profile: any other connection to an enabled profile must
go through `on_open`. Rejected: the audit's alternative of diffing synced tables against the
snapshot before a replace and re-applying local-only rows (it would decide, without clocks, which
side of an uncaptured edit wins, at the moment of a replace, and lose whatever it misread with no
trace), and `query_only` as the main mechanism (it also refuses local data and sync's bookkeeping,
so the way out would fail). The unlock sweep of revived seeds (M7) now covers only tables seeded by
migrations newer than `schema_seen`, does not run while capture is marked broken, and
`schema_seen` never goes down.

**Documents and photos survive a replace (H3).** Before a join, repair or re-sync clears the synced
tables, the attachment rows of this device are copied to TEMP tables and removed; after the
snapshot is restored, the rows whose parent the vault has are put back, before `repair::full` runs,
so their blobs are not swept. Rows whose parent the vault lacks are counted as orphaned and their
blobs go, as before; their only copy is the backup, which is then kept. `RestoreReport.attachments`
carries `kept` and `orphaned`.

**Replaces: one hold, the person's consent, kept backups (H4, M8, M9, L10, L11).**
- The confirmation shows what the replace puts at stake (`sync_replace_impact`: accounts and assets,
  transactions, documents, photos, changed items waiting, batches not yet back from the sync
  server) and sends its consent back: a watermark, the newest clock the device knew
  (`changes_through`: the largest of `sync_clock`, its own log's `max_hlc` and the outbox's
  `queued_at`), and for a join the count of documents and photos. Inside the hold the consent is
  checked again; a local change newer than the watermark (clocks merged from other devices do not
  count), including an edit to an item that was already queued, or a forged consent answers
  `validation.syncReplaceImpactChanged`, and the UI reloads the counts and asks again. A forged
  consent can only refuse.
- The backup and the replace run under one connection hold, so no UI or MCP write can land between
  them (M8). A re-sync first checks for a copied profile, then pushes what it can; a repair keeps
  its pre-drain backup, drains for at most 20 rounds while each round makes progress (less pending,
  or the cursor moved; two rounds without progress end it) and checks "nothing pending" in the hold.
  With nothing local pending, a waiting gap, a far-future hold (`clockAhead`, `ahead`) or a sync
  server that went back counts as drained; any other problem does not.
- Repair, re-sync, delete synced data, skipping a change and turning sync off with sync on need the
  device password (L10); the first four refuse an empty one (`validation.syncPasswordNeeded`)
  before the key check.
- A backup is written as `sync-repair-<unix>[-n].zip.partial`, flushed through a write handle (M9),
  read back (manifest and every entry's CRC; still a structural check, L11) and only then renamed.
  The mark is in the name: `-kept` holds something found nowhere else and is never pruned (every
  join and repair; a re-sync when changes stayed unsent, documents were orphaned, an envelope still
  waits to come back from the server, or the server went back); `-unused` means the replace never
  happened (pruned after 30 days, never counted); of the plain ones the newest three and everything
  under 30 days stay. A backup is marked `-kept` provisionally inside the hold and settles after it,
  so a crash or prune in between never removes it.
- Restore lists these backups (desktop Settings → Data, phone Záloha) and deletes one only after a
  confirmation, never while a sync change runs (H4). On iOS each can be copied to Files (staged as
  `moony-sync-backup-<local date>.zip`); Android has no Files copy and its text says so.

**Gaps and missing parents wait; a lost change can be skipped (M3, L2, L3).** A gap in an author's
chain and a change whose parent no envelope brought are holds, like a quarantine: the cursor stays
before the held envelope, other authors keep applying, this device keeps pushing. The status is the
new phase `waiting` with problem `gap`, not an error; a quarantine outranks a gap, so a server
cannot hide "held for safety" by withholding one envelope; a gap-only round counts as synced. No
skip is offered while `serverWentBack` or `quarantined` outranks the gap, though the held record
stands and Rust would allow it. A quarantine also outranks `relayError` and `newerSchema`, and `offline` and `rateLimited` when a
round fails after its pull held a quarantined author, so a server cannot mask it. The cost: while
a quarantine stands, a genuine newer-schema stop shows only "held for safety" and not "update this
app", although nothing past the stop is applied. Each
held author has a Local record (`app_config` `sync.held_gap.<device hex>`: the lowest held envelope
and when it was first held). After 14 days the person may skip it, behind the password and a
confirmation that names the device, how many changes go, that changes depending on them may need
skipping too and that they are lost if no device has them. A skip of a missing range moves the
author's head to just before the held envelope; a skip of a missing parent skips the held envelope
itself. The miss is recorded in `sync.skipped_miss` (the newest position); a skip made on this device
also writes `sync.skipped_miss_since` (when the oldest skipped gap was first held) and
`sync.skipped_miss_at` (when the newest skip was made), and an inherited miss writes neither; the skipping device still uploads snapshots, with the miss in the header
(`missedSkipped`; `missedBelowBarrier` carries the larger of both for older readers), and such a
snapshot never heals anyone; a clean snapshot heals it (a heal never downloads this device's own newest snapshot), and a restorer inherits only the miss position
(`sync.skipped_miss`, undated: never `since` or `at`). While a skipped miss stands, a new missing-parent hold takes the skip's
`since`, so a cascade is skippable at once, still behind the password; the inheritance is bounded:
only from a skip made on this device, and only for holds noted within 14 days of it (a miss
inherited at a restore does not make Parent holds instantly skippable). Rejected: recording the
skip as a below-barrier miss (no device would upload a snapshot again, so the relay could never
prune) and accepting a change as lost after a time without the person. A held envelope costs a
re-fetch of the tail behind it on every pull, at most 100,000 envelopes; past that the pull ends
unfinished and still shows the gap. A restore after `410` runs at most once per 6 hours (L2; only a
restore that completed or was refused on its bytes uses the window; the window is in memory). After
three unfinished pulls in a row the round pushes anyway (L3), and an endless pull with nothing held
shows `relayError`.

**Sealed envelopes stay until they come back (M4).** A push answer deletes nothing: an envelope
leaves `sync_unsent` only when this device's own pull sees it (`chain::acknowledge_own`). Each push
sends every unechoed envelope first, in order, then new ones, and a round that pushed pulls once more
for the echo (one extra call). No migration: a column would raise the schema number and stop older
devices on `NewerSchema`. Sealing stops at 1,024 unechoed envelopes and the round then shows
`relayError` if the echo brought none back; a batch of re-sends answered with none accepted is sent
once per round, and an envelope sent in an earlier round that is still not back after the echo
pull shows `relayError`, not "Up to date"; when that round's pull holds something, its own problem
stands (`quarantined`, `gap` or `clockAhead`), and a `gap` or `clockAhead` round records success.
The exception: a round whose echo pull fails transiently
or ends unfinished keeps the first pull's report and records success while an earlier envelope is
still unechoed; the next round shows it. Likewise an unfinished pull with nothing held that made
progress keeps recording success (pull progress is syncing; Task 18's ruling). A merge restore after `410` acknowledges own
envelopes up to the snapshot's head for this device. Behind a newer-schema stop the pull scans on to
the head, only while something waits for its echo, and only acknowledges own envelopes. What this
covers, and what it does not:

| The sync server… | Result |
|---|---|
| answers "stored" and loses the envelope, or never stores it | covered: it is sent again next round and other devices apply it |
| keeps the envelope's ID but never serves it | not repairable from the device (the relay ignores a repeated ID); `relayError` every round, unless that round's pull holds something, whose own problem then shows |
| is restored from an older backup (self-hosted) | **not repaired**; see below |

A device notices a server that went back when a pull page's head is below where its pull stands.
It writes the sticky Local key `sync.vault_went_back_at` and shows the problem `serverWentBack`
(phase error, no action button), which outranks `gap`, `clockAhead`, `invalidChain` and `relayError`.
Pulling and pushing go on, but "Last synced" stops. The mark clears only with a completed replace or
turning sync off; while it is set every re-sync and repair keeps its backup and no snapshot is
uploaded (a snapshot built on the old head would seed joiners with it). A device left in this
state therefore never lets the sync server prune: the vault grows toward its quota, and a later
joiner restores the snapshot from before the restore and pulls the whole tail. Nothing is repaired
automatically: envelopes the server lost and this device already saw echoed are not sent again,
devices whose cursor was above the restored head resume from their old cursor and miss what landed
in between, and later envelopes of an affected author wait as gaps. Re-sync is no remedy: it
replaces the profile with the server's old snapshot (the kept backup holds what it replaced).
Manual recovery for a self-hosted server:
1. Re-sync nothing. Pick the device with the most complete data.
2. On that device, "Delete synced data" removes the old vault from the server and turns sync off
   there (or just turn sync off and delete the vault on the server by hand).
3. Set sync up again on that device. Turning off forgot the account number, so setup creates a new
   one: add it to the server's `ALLOWED_ACCOUNTS`. Its whole profile becomes the new vault.
   *(Amended by [ADR 0028](0028-self-hosted-sync-server-in-the-app.md): there is no allow list; setup takes the person's connection code from the server's admin.)*
4. On every other device, turn sync off and join (pairing or the new Recovery Kit). Each join keeps
   its backup `-kept`; changes that only that device had are in it and must be entered again by hand.

The real remedy, re-seeding the vault from a device's own log, is phase 5.

**Pairing protocol 2 (M1, M2, L7, L8, L9).** The QR is `moony-pair:2:…`; slot 1 is `0x02 ‖ role ‖
C public`; the HKDF info stays `"moony pair v1" ‖ S public ‖ C public`, now expanded to 72 bytes:
session key, code, and an acknowledgement key (bytes 40..72). The shower reads slot 1 and writes the
new slot 3, `0x01 ‖ HMAC-SHA256(ack key, "shower->scanner" ‖ mailbox ‖ S public ‖ C public)`; the
scanner shows its six digits only after that verifies (constant time), and only a joiner shows
digits. The holder never shows digits: the person types the joiner's six digits on the holder,
checked in Rust in constant time (six digits, or 3+3 with a space or a hyphen; full-width digits
count as ASCII). Input that is not six digits spends no attempt; the third wrong code ends the
pairing, writes cancel markers and wipes the session. A relay that substitutes its key must commit
to it before the scanner reveals any digits, so grinding for a match no longer helps (M2; the grind
test stays, ignored, to show the digits alone can still be ground); an impostor that answers the QR
first leaves the person's phone with a warning and no digits to type (M1); a lost write followed by
"slot taken" can no longer hide a contested scan (L8). A contested scanner closes slots 2 and 3. A
v1 code answers "the other device needs an update", a v1 slot 1 is refused, and
`moony-pair:<n>:` with n above 2 answers "this device needs an update". A sync server without slot 3
answers `bad_slot`, read on both sides as `validation.syncServerNeedsUpdate`. The joiner's compare
step says "If you did not start this pairing, cancel." (L7). A plain-`http` sync server is allowed
only for loopback and only in debug builds, and both sides show a non-default server's host (L9).
*Amended by [ADR 0028](0028-self-hosted-sync-server-in-the-app.md): `http` is allowed for any host but the Moony cloud in every build, and the dialogs warn each time; showing the host stays.*
The vectors in `pairing/tests.rs` (ack key `f2131f57…e644`, ack tag `b473ae20…bced` for the frozen S
and C) are the contract. Rejected: a second mailbox derived from the first ID (no relay change, but
two mailboxes to expire, cancel and burn) and a nonce committed in the QR (a QR change and still a
third message); cost: every sync server must be redeployed with slot 3. The `hmac` crate's inner
state is not zeroized, the same limit as the HKDF PRK.

**The relay.** `feat/pairing-ack` adds slot 3 (write-once, read-once, burned with the mailbox; slot
4 is `bad_slot`) and makes a vault deletion resumable (L14): the record and a pending-wipe marker
change in one transaction, an alarm finishes the R2 and account calls and retries every 60 s, and
re-creating the vault meanwhile answers `409 vault_deleting` (the app treats it as a passing server
error that backs off). `feat/dev-grant` answers the dev grant
only with `DEV_GRANT=1` and a loopback request host, ships the example with the line commented out,
and tests the off state without reading `.dev.vars` (M13). The host check reads the Host header,
which a proxy may set, so it is defence in depth only; a server is safe because it never has
`DEV_GRANT=1` or runs `SELF_HOSTED=1`. The home sync server must be redeployed from
`feat/pairing-ack` before pairing works against it.

**Builds without the flag (M12, L12).** Capture or guard runs in every build, so a release build
keeps recording an enabled development profile's changes (they are never sent from that build).
The audit's alternative, returning early from `on_open`, was rejected: edits would be written with no
trigger, never sent, and invisible to a later sync build, so devices would diverge silently.
`sync_turn_off` and the read-only `sync_profile_state` answer without the flag, and an unreadable
`sync_state` row counts as sync on, so it can still be turned off (besides
`sync_feature_enabled`), and Settings shows a leftover card (desktop Settings → Data, a row on the
phone) that turns sync off. CI runs the release-build flag test (`cargo test --release`).

**The binding fails closed (M10, L13).** A native `Failed` answer stops the start: no round, the
problem `secureStorage` (phase paused). It counts once per app start, process-wide (a later attempt in the same process does not count, and
neither does a restart of the sync engine), in the Local key
`sync.binding_failed_starts`; the third start in a row re-joins under a new device ID, any other
answer resets the count, turning sync off removes the key, and repair and setup checks never count, so one locked keychain cannot
force a re-join.

**Far-future clocks and the public key (M6, L1).** A pulled change dated more than 10 years past this
device's wall clock is held, not applied (one day to ten years ahead applies, with the existing
far-future warning). The hold is a third skippable held-gap kind, `ahead`, with the same 14 days,
password, confirmation and snapshot-header miss rule; for a device whose every envelope is more than
ten years ahead, each skip drops one envelope and the next is held for another 14 days, so "then
apply" holds only for later envelopes within the bound. Both snapshot restores (merge and replace,
joins included) refuse a snapshot whose newest clock passes the same bound; a refused snapshot
shows `invalidChain` in a round and `validation.syncSnapshotClockAhead` in a join or re-sync. A device whose own clock
is more than a day past its wall clock shows a note with the date its changes carry until then
(`clockAheadUntil`): until that date its edits win over edits made elsewhere. A 24-hour hold was
built first and rejected: an honest clock set days or weeks ahead turned into held edits and silent
divergence. Cost: a key holder can move every device's clock up to 10 years ahead in one envelope.
That is not only cosmetic: edits made on a device that has not yet pulled the bump lose
last-writer-wins to earlier edits for up to ten years (the counter cannot be saturated, and the
clock is read as a time only for "last change"). A residual of a vault-key holder, who can already
forge any author. `sync_devices.public_key`: a key stamped with the device's own ID beats one stamped with
another ID, and between two alike the older one wins (write-once). That stamp is not proof: any
holder of the vault key writes changesets under any device ID, so the column must not be trusted
for rekeying until changes are signed per device. A re-join ID is drawn at most 2^96 above the old
one, so 10,000 re-joins never run out of IDs (L1).

**The Recovery Kit (M11, L16).** Every showing needs the device password, the first one in setup
included. Copying and saving need the 5-minute window that password opens (closed at lock, restore,
turn-off and deletion). The Kit leaves the screen after 5 minutes, when the app is hidden (on the
desktop only on minimise or hide, not on blur, so a password manager can be used) and when setup
moves past it. While it shows, the desktop window is protected from screen capture. The code is
copied natively only: any failure but "unsupported" answers `validation.syncCopyFailed`, with no
ordinary-clipboard fallback; where there is no native copy (Linux, Android) the webview copies and
clears the clipboard after 60 s if it is unchanged. Saving runs in Rust: Rust opens the save
dialog, checks the contents hold this Kit's code (at most 256 KiB; a page with a script, an inline
handler or a `javascript:` URL is refused), refuses the data folder, stages under a random
`.<name>.<uuid>.tmp` created exclusively with mode 0600 (Unix; on Windows the file takes the
folder's permissions), renames it, and warns when the folder looks cloud-synced (a hint). Phones
keep their text export. The main database runs `PRAGMA secure_delete = ON`, which zeroes the
deleted vault key's cells in the profile file's free pages; that is not physical erasure. Profiles
use a rollback journal, so the WAL truncation turning sync off also runs is a no-op there, and the
journal file is unlinked, not wiped. Cancelling a setup deletes its secrets; abandoning it keeps them until resume or
cancel, behind the password. The unlock password no longer stays in mutation state (L16).

**Telling the truth about H1 and M5.** The device lists and the Kit screens say that a lost device or
Kit cannot be cut off yet and can already read what it holds, and what to do (delete the synced data,
set up again with a new Kit, join the others again); turning off says it cuts off nothing; deleting
says setup starts over with a new account and Kit. A device with no changes for more than 30 days
(by the newest clock it wrote) carries a neutral "No changes from this device in N days" marker.

## Consequences

- Invariants to keep: an enabled profile is captured, guarded or read-only, never writable
  uncaptured; every connection to a profile goes through `on_open`; a push answer never deletes an
  envelope; a replace backs up and replaces in one hold within the consent; a change to the pairing
  bytes is a new protocol number with new vectors; a backup marked `-kept` is deleted only by the
  person.
- New Local `app_config` keys (no migration, never captured): `sync.held_gap.<device hex>`,
  `sync.skipped_miss`, `sync.skipped_miss_since`, `sync.skipped_miss_at`, `sync.vault_went_back_at`,
  `sync.binding_failed_starts`. New phase `waiting`; new problems `clockAhead`, `secureStorage`,
  `serverWentBack`; `missingParent` stays in the wire enum but is no longer produced.
- Accepted residuals:
  - H1: one vault key, no author authentication, no removal or rotation until phase 5; the UI says
    so. Every paired device, Kit, profile backup made while sync was on, `moony.db.bak-*` copy and
    `sync-repair-*.zip` holds the key, also after sync is turned off.
  - M5: a sync server can withhold, delay or fork, and a join takes any genuine snapshot; only the
    quiet-device marker hints at it. A relay that keeps an envelope's ID but never serves it is
    reported, not repaired.
  - M6: a key holder can still forge an author (the public-key stamp included) and push a change that
    deletes most of a table; there is no mass-deletion guard.
  - M10: the device secret key and vault keys stay in the database, not in the sealed blob, so a copy
    with the password can re-seal the binding; Windows DPAPI is machine-scope, so a disk, image or VM
    clone should pass on both machines; no TPM-backed key.
  - A server restored from an older backup is noticed but not repaired (above).
  - A device whose wall clock is more than ten years behind holds every change from other devices as
    `ahead` (problem `clockAhead`), refuses every snapshot and so cannot join, until its clock is
    corrected; skipping the changes after 14 days loses them on that device.
  - Endless pulls fill the unechoed cap: a persistent `relayError` with a growing pending count
    (while the pull holds something, that hold's problem shows in its place).
  - A skipped envelope that arrives later is below its author's head and is not applied; only a clean
    snapshot brings its rows (applying it would be possible in principle, an improvement left open).
  - L4 (a replay at a huge position jumps the cursor), L6 (a maximum snapshot costs memory before its
    first chunk is authenticated), L7 (a person who confirms on the joiner despite the warning can be
    steered into an attacker's vault), L13 (unknown native binding errors read as another machine;
    at worst a re-join), L15 (the account number alone creates vaults): availability or
    social-engineering limits behind a hostile relay or key holder.
  - The clipboard holds the code up to 60 s and is not cleared after a crash; phone screen capture of
    the Kit is not blocked; a visible but unfocused desktop window shows the Kit up to 5 minutes; the
    cloud-folder warning misses synced Desktop and Documents folders; `secure_delete` and the WAL
    truncation (a no-op on a rollback journal) do not reach SSD remapped blocks, file system snapshots or earlier copies.
  - The relay still learns the account-to-vault link, the device count, activity timing and padded
    sizes.
- Owner decisions still open:
  - deleting the synced data or cancelling a paid setup forgets the account number, so a paid
    account is orphaned (account continuity, phase 5 and pricing);
  - merging `feat/pairing-ack` and `feat/dev-grant` into the relay's main and redeploying the home
    sync server from it (and confirming its `.dev.vars` has no `DEV_GRANT=1`, or that it runs
    `SELF_HOSTED=1`);
  - phase 5 scope: device removal and key rotation, per-device signatures (at least snapshot heads and
    `sync_devices` rows), re-seeding a vault from a device after a rollback, a mass-deletion guard;
  - the change-password card and the setup and recover mutations still keep passwords or the recovery
    key in mutation state (follow-up to L16).
- Checks owed on real hardware before release: one full Windows join (M9's flush) and Windows Hello;
  the error a binding blob from another Mac or iPhone returns (M10, L13); a disk or VM clone against
  the Windows binding; concealed clipboard types against the current macOS clipboard history; content
  protection on macOS and Windows; the iPhone's Files copy of a sync backup and an Android restore
  from the list; the iPhone checks of ADR 0026; the live pairing with the typed code on desktop and
  phone (both directions, wrong codes, three strikes, an old sync server, a self-hosted server's host
  line, expiry, cancel and a contested scan) and the Kit's password, expiry and save on both
  platforms.
