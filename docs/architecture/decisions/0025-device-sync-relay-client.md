# 0025. Device sync: the relay client, the device binding and the Recovery Kit

Date: 2026-11-09

## Status

Accepted. Records phase 2 of `docs/specs/2026-11-06-device-sync-client-design.md` as built: the
relay client, still hidden behind a runtime flag (`src-tauri/src/services/sync/`: `envelope/`,
`relay/`, `engine/`, `binding.rs`, `recovery.rs`, `keys.rs`, `setup.rs`, `join.rs`;
`commands/sync.rs`; the binding and clipboard natives of `plugins/moony-keychain`;
`src/components/settings/sync/`). Builds on ADR 0024. Where the spec's text differs, the code and
this ADR win.

## Context

Phase 1 merged other devices' changes but sent nothing. Phase 2 puts the vault on an untrusted
relay (Moony-relay), which must learn nothing and must not be able to make devices diverge by
reordering, withholding or replaying envelopes. A copied or restored profile must notice before it
forks its own chain, and the person needs a way back when every device is lost. The spec named the
fields of the wire format but not their bytes, and two of its rules failed once relay order was
real (the barrier, re-join clocks); review rounds settled them (rulings P1–P24, T8-1–T8-7, SH-1–2).

## Decision

**Wire.** Op envelope = `"MNY1" | epoch (u32 BE) | 24 random nonce bytes | XChaCha20-Poly1305
ciphertext+tag`; key = HKDF-SHA256(vault key of the epoch, salt = the vault ID's 16 raw bytes,
`"moony sync ops v1"`); AAD = `"MNY1" | epoch | vault ID`. Plaintext = raw deflate (RFC 1951) of
the changeset JSON, zero-padded to a multiple of 1 KiB; opening checks the padding and caps the
inflated JSON at 4 MiB and the envelope at the relay's 192 KiB op limit. Op ID = the first 32
characters of base64url(SHA-256(envelope)); vault token = base64url(HKDF(key, vault ID,
`"moony sync token v1"`)). Epochs start at 0, and there is one until key rotation (phase 5). A
snapshot is the phase 1 record stream, raw-deflated, sealed as one STREAM object: header
`"MNY1" | epoch | 32-byte salt | 19-byte nonce prefix`, its own key HKDF(key, salt,
`"moony sync snapshot v1"`), 64 KiB chunks, nonce = prefix | chunk counter (u32 BE) | last-chunk
flag, AAD = op AAD | relay sequence `S` (u64 BE); an input of whole chunks ends in an empty last
chunk. Uploads and downloads are capped at 100 MiB, the inflated stream at 512 MiB. The fixed
vectors in `envelope/tests.rs` are the contract with every other device.

**Rounds.** Pull before push: a round pushes only when its pull ended done, with only a quarantined
author's envelopes held, or stopped only on a newer schema (P1); any other stop, and a pull cut at
its page limit, push nothing. A snapshot is dumped only after a pull that ended done with
`sync_outbox` and `sync_unsent` empty, and a round that pushed pulls again first, so `S` covers its
own envelopes (P4, P14). Uploads are due after 2,000 envelopes or 8 MiB pulled since the vault's
newest snapshot, at most once a day after a random delay; a vault without one gets one at once.
`410` restores the vault's snapshot by merge, then pulls again. A quarantined author's envelopes
are held: the persisted cursor stays before the first held one while other authors keep applying,
so each round fetches and re-judges from there. The loop runs one task per unlocked profile
(triggers: unlock, the outbox polled every 5 s, every 5 minutes, "Sync now"), one round per profile
at a time under an async round lock, never holding the connection across a network call; it backs
off 30 s doubling to 30 minutes and drops a round at 15 minutes. Relay states are problems shown
in Settings, not errors: a vault the relay does not know pauses sync and never deletes keys,
`402` keeps pulling, and a too-large snapshot is not retried for a day (persisted).

**Barriers and re-join.** An envelope written below this device's barrier is never applied,
wherever the relay placed it (T8-2): its author's head moves on and its relay position is kept as
`missed_below_barrier`. A snapshot heals the miss only if it was dumped at that barrier or later,
at `S` ≥ the miss, by a device with no miss of its own (the header carries it); a device with a
miss never uploads. The barrier marker is the envelope earliest in relay order at or above the
barrier (`barrier_seq`, the smallest position seen); a device past a barrier with nothing to send
seals one empty marker changeset, at most one per barrier (`marker_barrier`, P5). A device that
crosses a barrier uploads a snapshot at its first clean round, across restarts
(`snapshot_barrier`). A re-join takes a new device ID drawn above the old one; the re-queued column
clocks of the old ID keep their hlc and move to the new ID (T8-1, replacing the plan's fresh
clocks), so a re-sent edit never beats a later edit of another device. Row clocks and tombstones
keep the old ID.

**HTTP.** One parsed relay address: `https`, or `http` only for `localhost` and `127.0.0.1`; no
user, query, fragment or path; normalized (lowercase host, no default port, no trailing dot or
slash) and at most 256 characters. Redirects are off, so the `Moony-Account` header and the token
never reach another origin. Every body is read under a cap, errors carry no URL or body, the
checkout page must be `https`. Push and pull get 120 s, snapshots 600 s, everything else 15 s.

**Runtime flag.** `feature::RELEASED` is `false`. A debug build runs sync only with
`MOONY_DEV_SYNC=1` (and reads `MOONY_RELAY_URL`); a release build reads neither. Every sync
command but `sync_feature_enabled` refuses while the flag is off (a source-scan test checks it);
the dev grant (relay code compiled into debug builds only) and the capture measurement answer
only in a debug build with the flag on.

**Setup, join and repair.** Setup: account (its sync server address stored) → payment (Polar in the
browser, polled) → Recovery Kit → finish (vault, sync on, key pair, device row, binding, then a
round that pushes, pulls and uploads the first snapshot). Account and vault secrets are stored with
sync still off, so a restart resumes. Join by replace (D22) runs only after a verified local backup
(`sync-repair-<unix>.zip` next to the profile, flushed and read back with every entry's CRC), in
one savepoint under a new device ID; any failure leaves the profile as it was. Phase 2 joins only
through repair and re-sync; joining a new device (pairing, the Kit) is phase 3. Repair of a profile
whose capture broke = backup, capture reinstalled, a drain round that must end clean (pull done,
outbox and unsent empty), then join; otherwise nothing is replaced. "Re-sync this device" (T8-4) =
backup and join without a drain, after the password and a warning that unsent changes survive only
in the backup. Finishing setup, turning off, deleting synced data, repair and re-sync run one at a
time with the engine stopped (`validation.syncChangeInProgress` otherwise); the UI blocks editing
during repair and re-sync. Turning off forgets `sync_state` (the address too), clocks, outbox,
device list, binding and marker. Delete Account stops the engine, deletes the vault within 5 s if
it can, and removes the binding, the marker and the repair backups.

**This-machine binding.** `sync-binding.bin` holds SHA-256(`"moony sync binding v1"` ‖ device ID)
sealed to the machine: on macOS by a Secure Enclave P-256 key without user presence (ECIES, the
key handle inside the blob, no keychain entitlement); on Windows by DPAPI in machine scope. At
unlock, before the first round, the restore marker (left by `restore_backup`) or a binding that is
missing, sealed elsewhere or for another device ID (compared in constant time) re-joins the profile
under a new ID before anything is pushed. Unknown native answers read as "not this machine". The
binding is rewritten after setup, join and re-join, and retried at start until written.

**Recovery Kit.** An 80-character Crockford base32 code (vault ID and current key, 384 bits, plus
a 15-bit checksum that misses about one typo in 32,768), printed in 16 groups of five, and a QR of
`moony-recover:1:<16-digit account>:<code>`, with `:<base64url sync server address>` as a 4th field
only for a server that is not the default (the reader takes both, at most 512 bytes). It is saved
through the save dialog as a printable HTML page or plain text (no PDF). The Kit is prepared only
while sync is off; once sync is on it is shown only after the device password. The code is copied
by the native side (it never crosses IPC): with sync on only within 5 minutes of that password
check, a window tied to the profile's database and session and closed at lock, restore, turn-off
and deletion. macOS copies for this Mac only (no Universal Clipboard) with the concealed and
transient types; Windows keeps it out of clipboard history and the cloud clipboard. Both clear it
after 60 s, or earlier when the app exits, locks, restores a backup or deletes the account,
unless something else was copied since (the pending clear is held in app state and taken once).
Linux has no native copy: the UI copies and asks the person to clear the clipboard. The Kit and every password-bearing mutation have `gcTime: 0`;
secret types print redacted and live in `Zeroizing` buffers.

**Self-hosting.** The relay's `SELF_HOSTED=1` mode entitles exactly the accounts in
`ALLOWED_ACCOUNTS` (no expiry) and answers checkout with `409 self_hosted` (SH-1). The app's setup
has an advanced sync server address, validated and normalized as above, stored in
`sync_state.relay_url` and used for everything after; a `self_hosted` checkout shows the account
number with the instruction to add it to `ALLOWED_ACCOUNTS` and keeps waiting; a far-future paid
date reads "Self-hosted server" (SH-2). User copy says "sync server", never "relay" (glossary).

**Migrations.** 030 adds the engine's Local bookkeeping to `sync_state` (`barrier_seq`,
`marker_barrier`, `capture_broken_at`, `vault_gone_at`, `pulled_bytes`, `pulled_bytes_base`,
`snapshot_uploaded_at`, `binding_written`); 031 `missed_below_barrier`; 032
`snapshot_too_large_at`; 033 `snapshot_barrier` (existing rows `MIN(marker_barrier, barrier)`).
All are Local and additive, so the barrier stays 25, but the schema number (the migration count)
is 33: a build that knows fewer migrations judges this build's envelopes `NewerSchema` (it stops
pulling and keeps pushing) and refuses its snapshots with `validation.syncSnapshotNewerSchema`.

**Backups hold the vault keys (P17).** A profile backup carries `sync_state`. Restored on another
machine it carries the keys there; the restore marker makes that copy re-join under a new device
ID, and a backup of another vault re-joins that vault. The keys stay the same person's: accepted,
the same trust as the backup itself.

## Consequences

- Sync network code goes through `services/sync/relay/http.rs`, the engine never holds the session
  mutex across a network call, and sync errors travel as `SyncError` until the command boundary
  (`docs/standards/rust-backend.md` §12).
- Known limits, accepted:
  - a below-barrier envelope that no device applied before crossing stays missed on devices that
    crossed early (the heal needs a snapshot from a device that applied it);
  - an own-ID fork below an author's head is answered `SkipReplay`, so a copy can stop on a gap at
    its own old-ID envelope; diagnostics say the sync history is inconsistent and "Re-sync this
    device" is the way out (T8-4); a quarantine re-fetches the held envelopes every round;
  - the binding: Linux and Macs without a Secure Enclave have none; a copy on the same machine (any
    user) opens it and is left to the relay's own-log check; a transient native failure lets the
    start go on;
  - MCP writes are not blocked during repair or re-sync (only the UI is); Delete Account does not
    take the one-change guard; a lock and unlock inside a sync change, or a panic in it, leave the
    engine off until the next unlock;
  - the copied code lives in a Swift string and on the system clipboard, neither of which can be
    wiped; a crash or a force-quit within 60 s of a copy runs no clear, so the code then stays on
    the clipboard;
  - sealing re-reads the whole outbox per envelope (slow only for a first drain of tens of
    thousands of rows); a resumed setup shows the custom server field empty; "behind" is not
    reported when a pull ends at its head bound;
  - mobile sync is not wired: the mobile shell mounts no sync events and has no sync screen, and
    the binding answers `Unsupported` on phones.
- Owner checks owed before release: the Windows DPAPI binding and the private clipboard at runtime
  (CI builds and runs the plugin's tests only); the Secure Enclave error a binding from another Mac
  returns (read as "not this machine" by rule, not yet seen across two Macs); Windows Hello and a
  real-profile dry run of migrations 030–033; the capture measurement on an iPhone (P20); focus of
  the password dialog over the busy modal during re-sync. At merge, check that main gained no
  migration 030–033 (renumber otherwise) and consider folding the four into one.
