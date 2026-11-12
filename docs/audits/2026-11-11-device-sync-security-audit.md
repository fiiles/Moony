# Device sync security audit (phases 1–3)

Date: 2026-11-11/12. App at `main` 5c8d772c; relay at `main` a2ec09d and `feat/dev-grant` 4e95a4f.
Independent of earlier reviews. Nothing was deployed, pushed or changed in either repository; tests
ran in a detached scratch worktree, and the relay's own suite ran locally.

Paths are relative to `src-tauri/src/services/sync/` unless they start with another folder.
"Relay:" paths are in `Moony-relay`.

## How each finding was checked

- **PoC** — a test I added and ran; all are in `2026-11-11-device-sync-audit-poc-tests.patch`.
- **Existing test** — a test already in the repository asserts the behaviour; I ran it.
- **Read** — I read the cited lines myself.
- **Traced** — an auditor traced the path end to end; I did not re-read every line.
- **Untested** — depends on OS or hardware behaviour nobody ran.

## Verdict

The cryptographic building blocks are sound, and a relay that holds no key cannot read, forge or
alter anything. The weaknesses are one level up:

1. There is one static vault key, no author authentication and no revocation. Anyone who ever held
   the key keeps full access for good.
2. "No path loses data silently" is false in three places.
3. Pairing releases the vault keys on one confirm, and the six digits can be matched by a relay
   operator who also sees the QR and the phone.
4. A relay can stop sync permanently and steer a person into a re-sync that drops unsent changes.

No Critical findings. I would not release sync before the High items and Medium items 5–12 are
fixed or consciously accepted.

---

## High

### H1. One static key, no author authentication, no revocation
- **Where:** `engine/push.rs:150`, `engine/snapshots.rs:94` (always `keys.current()`, epoch 0);
  `envelope/mod.rs:169` (AAD is magic, epoch, vault ID only); `join.rs:222` (a rotated vault is
  refused, rotation does not exist); `envelope/mod.rs:71` (relay token derived from the same key).
- **Who holds the key:** every paired device, every Recovery Kit, every profile backup and
  `sync-repair-*.zip` made while sync was on (`keys.rs:403`).
- **Exploit:** a lost phone, a leaked Kit or an old backup plus its password. The holder reads all
  history and all future changes, writes rows as any device ID, and can delete the vault with the
  derived token. Concretely, a forged snapshot head (`snapshot/mod.rs:294`, heads merge by `MAX`)
  makes every device silently skip one device's future envelopes as replays (`chain/mod.rs:165`).
- **Impact:** permanent loss of confidentiality and integrity with no remedy but a new vault. Spec
  §13 ("removing the device stops it from following future changes") is not true in this build.
- **Fix:** ship key rotation and device removal before release, or state plainly in the UI that a
  lost device or Kit cannot be cut off. Add per-device signatures over (vault, epoch, device, seq,
  prev, ciphertext hash), at least for snapshot heads and `sync_devices` rows.
- **Checked:** Read.

### H2. Repair and re-sync discard every edit made while change capture was broken
- **Where:** `mod.rs:58` (a failed capture install still opens the profile writable);
  `join.rs:294-350`; the drain gate `join.rs:408-435` looks only at `sync_outbox`/`sync_unsent`.
- **Reproduction:** capture fails at unlock → the person keeps editing → "Repair this device" →
  backup, clean drain (the outbox is empty), replace. The uncaptured edits are gone.
- **Impact:** edits survive only inside the backup zip. The dialog says "If the changes cannot all
  be sent, nothing is replaced", which reads as "nothing is lost".
- **Fix:** open a capture-broken profile read-only, or diff synced tables against the snapshot
  before replacing and re-apply local-only rows. At minimum, say so in the dialog.
- **Checked:** Existing test `repair_backs_up_drains_and_rejoins_under_a_new_device`
  (`engine/tests/repair.rs:57`) asserts the uncaptured row is gone.

### H3. Join, repair and re-sync delete this device's documents and photos
- **Where:** `snapshot/mod.rs:363` (`clear()` deletes every synced table; attachment rows cascade
  from their parents); `repair/mod.rs:184` → `attachment_store::delete_unreferenced` in the same
  transaction. Attachment tables are local-only in v1, so no other device has the files.
- **Reproduction:** a policy with a contract PDF on device B → "Re-sync this device". The policy
  returns from the vault under the same ID; its document rows and blobs do not.
- **Impact:** the only copy is inside the backup zip; no dialog mentions attachments.
- **Fix:** detach local attachment rows before clearing and re-attach after the restore; skip the
  blob purge for re-attached rows. At minimum, warn with a count.
- **Checked:** Traced. The proposed test is in the auditor's notes, not run.

### H4. On phones the "verified backup" cannot be reached
- **Where:** `join.rs:450` writes into the app data folder; the iOS `Info.plist` has no file
  sharing; the phone's restore uses the system picker (`src/mobile/hooks/use-restore-backup.ts:52`).
- **Impact:** H2, H3 and M3 are permanent on a phone.
- **Fix:** offer "Save backup to Files" before the replace, or list repair backups in Restore.
- **Checked:** Traced; not exercised on a device.

---

## Medium

### M1. A QR observer who answers first gets the keys on a lone desktop confirm
- **Where:** `pairing/session.rs:369` (`read_scan`), `:412-458` (`confirm` → `send`);
  `commands/sync_pairing.rs:275` (the QR is shown only while waiting for a scan).
- **Exploit:** someone who sees "Add a device" on a shared screen writes slot 1 before the phone.
  The desktop moves to Compare and hides the QR, so the real phone never scans and the
  contested-scan defence never fires. One click on "They match" seals every key to the impostor.
- **Fix:** the holder types the digits shown on the other device, checked in Rust.
- **Checked:** PoC `audit_f1_…`.

### M2. A relay operator who sees the QR and the phone's digits can match the code
- **Where:** `pairing/session.rs:309` (the scanner shows its digits as soon as its write returns);
  `pairing/mod.rs:286` (the code depends only on DH, the mailbox and both public keys).
- **Exploit:** the relay withholds the phone's key from the desktop, reads the phone's digits,
  searches for a key that gives the same digits, and serves that. Both screens match; the holder
  seals to the attacker. No user error is needed. ADR 0026's "one chance in 10^6" holds for the
  relay and the QR observer separately, not together.
- **Fix:** the scanner shows digits only after the shower proves it used the scanner's key (a third
  slot with an HMAC under the session key), or a nonce committed in the QR and revealed afterwards.
- **Checked:** PoC `audit_f2_…` (see the grind result under "Test results").

### M3. Reordering or withholding one envelope stops a device for good
- **Where:** `chain/mod.rs:161` (Gap is a hard stop, nothing is buffered); `engine/mod.rs:120`
  (a stopped pull blocks pushing); `src/utils/device-sync-problem.ts:16` (the only advice is
  Re-sync); `join.rs:338` (re-sync replaces without a drain).
- **Impact:** the relay can drive a person to a re-sync whose unsent changes survive only in the
  backup; the replacement snapshot is also relay-chosen.
- **Fix:** hold a future-sequence envelope and continue; let a device in Gap keep pushing; never
  offer Re-sync first while the outbox holds anything.
- **Checked:** PoC `audit_m1_…`.

### M4. A push answer deletes the only copy of the sealed envelope
- **Where:** `engine/push.rs:264`.
- **Exploit:** a relay answers success and stores nothing, or a self-hosted relay is restored from
  an older backup. The author has nothing to send again; other devices hit a permanent Gap.
- **Fix:** keep sealed envelopes until the device's own pull sees them back.
- **Checked:** PoC `audit_m2_…`.

### M5. A relay can split devices or serve an old state with no signal
- **Where:** `engine/pull.rs:217` (`behind` is the relay's own head); `join.rs:99` (any genuine
  snapshot is accepted, nothing proves it is the newest).
- **Impact:** silent divergence; on a join or re-sync, deleted rows return. Contradicts spec G3.
- **Fix:** carry each author's known heads inside the envelope and flag heads not reached; warn
  when a device has not been heard from for N days.
- **Checked:** Read (`pull.rs`); the rest Traced.

### M6. A key holder can pin values forever and take over a device's public key
- **Where:** `changeset.rs:203` (any clock up to `i64::MAX`, no skew bound);
  `merge/columns.rs:59` (write-once columns: the older clock wins, no lower bound).
- **Exploit:** one row at the maximum clock saturates every device's clock; later edits tie and
  stop propagating with no error. A `sync_devices` row at clock 1 replaces another device's public
  key, the key a future rekey would seal to. There is also no guard against a changeset that
  deletes most of a table.
- **Fix:** reject or quarantine clocks more than a bounded skew ahead; only the device itself may
  set its public key; hold and back up before a mass deletion.
- **Checked:** PoCs `audit_maximum_clock_…` and `audit_forged_older_clock_…`.

### M7. The unlock sweep deletes ordinary rows, not only seeded ones
- **Where:** `repair/mod.rs:203` (`delete_revived_seeds` walks every synced table), called from
  `mod.rs:87` before capture is installed, with cascades and no backup.
- **Reproduction:** a row deleted and then re-created under the same ID during a capture-broken
  session (a re-imported statement, a re-added ticker) disappears at the next unlock.
- **Fix:** restrict the sweep to keys a migration seeds; skip it while capture is marked broken.
- **Checked:** Existing test `opening_a_synced_profile_deletes_revived_seeds_before_capture`.

### M8. Writes between the backup and the replace are lost
- **Where:** `join.rs:152`, `:303-320`, `:346`; only the round lock is held, and the local API/MCP
  server keeps running during a sync change.
- **Fix:** take the backup and replace under one connection hold, or abort if `data_version` moved.
- **Checked:** Traced.

### M9. Windows: the backup flush probably fails, so join, repair and re-sync never run
- **Where:** `join.rs:441`, `File::open(&dest)?.sync_all()?` on a read-only handle.
- **Impact:** fail-safe, but the feature would be unusable on Windows and each attempt leaves a zip.
- **Fix:** open with write access, or sync the handle the zip writer returns.
- **Checked:** Read; Untested (needs a Windows run).

### M10. The device binding is a tripwire, and it fails open
- **Where:** `binding.rs:107-152`, `:248`; `keys.rs:445` (the device's X25519 secret and the vault
  keys live in the database, not in the sealed blob); `plugins/moony-keychain/src/desktop/windows.rs:160`.
- **Issues:**
  - The blob seals only a hash of the device ID. A copy with the password can clear
    `binding_written` or re-seal the hash and run as the original device, holding its private key.
  - `binding.rs:147`: a native `Failed` answer lets the start go on, while a file read error is
    handled strictly. The iOS "momentary" code list is broad, and no one has observed what a blob
    from another Mac or iPhone returns.
  - Windows uses machine-scope DPAPI, so a disk, image or VM clone should pass on both machines.
- **Fix:** keep `device_secret_key` inside the sealed blob; treat `Failed` as "no round, retry";
  use a TPM-backed key on Windows.
- **Checked:** Read (`binding.rs`, the DPAPI flags); clone behaviour Untested.

### M11. Recovery Kit exposure
- **Where and what:**
  - `commands/sync.rs:396`: the 5-minute password window gates only the native copy. The code and
    QR stay in React state while the dialog is open, and "Save as…" runs in the webview
    (`src/components/settings/sync/RecoveryKitPanel.tsx:42`).
  - The Kit is saved as a plaintext file with default permissions in whatever folder the OS offers,
    which may be cloud-synced.
  - `commands/sync.rs:566`: any native copy failure, not only "unsupported", falls back to a plain
    clipboard write with no expiry. Linux and Android always take that path.
  - `setup.rs:164`: during setup the Kit needs no password, and the secrets persist if setup is
    abandoned.
  - `state.rs:196`: turning sync off deletes the row without `secure_delete`, so the key stays in
    freed pages.
- **Fix:** drop the Kit from state after a short time and on blur; write the file from Rust with
  owner-only permissions and warn about synced folders; fall back only on "unsupported" and clear
  after 60 s; require the password once secrets exist; enable `secure_delete` on the main database.
- **Checked:** Read.

### M12. Release build: capture triggers ignore the sync flag
- **Where:** `src-tauri/src/db/mod.rs:97` → `mod.rs:80`; only `sync_state.enabled` is checked.
- **Impact:** a profile enabled in a debug build grows its outbox under a release build, and the
  hidden UI cannot turn it off. No network. Does not affect profiles that never had the flag.
- **Fix:** return early from `on_open` when the feature is off; add a test.
- **Checked:** Read.

### M13. Relay: the dev-grant gate depends on a file in the working directory
- **Where:** Relay: `src/dev.ts:14`, `.dev.vars.example` (ships `DEV_GRANT=1`),
  `test/self-hosted/dev-grant-off.test.ts`.
- **Reproduction:** `npm test` on this machine: 97 pass, the "grant is off" test fails with
  `expected 200 to be 404`, because the local `.dev.vars` switches the route on in the project that
  never asks for it. The home-server instructions run `wrangler dev` from the same file.
- **Impact:** inert in self-hosted mode; on a home server started without `SELF_HOSTED=1`, anyone
  can grant themselves a paid period. A Cloudflare deploy does not read `.dev.vars`.
- **Fix:** comment the line out in the example; also require a loopback request host; make the
  off-state test independent of `.dev.vars`.
- **Checked:** ran the relay suite.

---

## Low

| # | Finding | Where | Checked |
|---|---|---|---|
| L1 | Forced re-joins exhaust the device-ID space: mean 88, worst 116 of 200 runs | `chain/rejoin.rs:44` | PoC |
| L2 | A relay that answers `410` every round forces a full snapshot restore each round, no back-off | `engine/mod.rs:296-330` | Traced |
| L3 | An endless pull blocks pushing while the round records success | `engine/status.rs:172` | Read |
| L4 | Replays at huge positions jump the cursor for good | `engine/pull.rs:198`, `state.rs:310` | Traced |
| L5 | `paidUntil + DELETE_AFTER_LAPSE` is unchecked: debug panic, wrapped date in release | `engine/status.rs:98` | Read |
| L6 | Restoring a maximum-size snapshot can take several GiB; 100 MiB is allocated before the first chunk is authenticated | `envelope/stream.rs:161`, `snapshot/stream.rs:274` | Traced |
| L7 | Joining from the phone can be steered to an attacker's vault if the person confirms despite the warning | `pairing/session.rs:347` | PoC |
| L8 | A lost write followed by "slot taken" hides a contested scan | `pairing/session.rs:313` | PoC |
| L9 | A QR can name a plain-http loopback server in release builds; the scanner never shows the server | `relay/http.rs:93` | Read |
| L10 | The holder's confirm, turning sync off and repair need no password | `commands/sync_pairing.rs:577`, `commands/sync.rs:656` | Traced |
| L11 | The backup is checked structurally only (zip, manifest, CRC); written in place; never pruned | `join.rs:470` | Read |
| L12 | Release isolation rests on `debug_assertions` alone; the release-ignores-env test never runs in CI | `feature.rs:16-47`, `.github/workflows/ci.yml:99` | Traced |
| L13 | iOS and macOS binding error tables differ; some transient DPAPI codes force a re-join | `plugins/moony-keychain/src/models.rs:36`, `desktop/dpapi_errors.rs:21` | Traced |
| L14 | Relay: vault deletion drops the record before the R2 and account calls; a failure leaves ciphertext and the slot | Relay: `src/vault.ts:510` | Read |
| L15 | Relay: the account number alone creates vaults; five fill the account and the owner cannot remove them | Relay: `src/index.ts:218`, `src/account.ts:129` | Read |
| L16 | Unlock password may linger in mutation state; no screen-capture protection on desktop | `src/hooks/use-auth.tsx:263` | Traced |

---

## Verified sound

- **Envelopes:** XChaCha20-Poly1305 with a random 24-byte nonce, no persisted nonce state; AAD
  binds magic, epoch and vault ID; length and inflate caps before use (`envelope/mod.rs:88-231`).
- **Key derivation:** ops, token, snapshot and pairing keys are separated by HKDF info strings; the
  vault key is never used raw.
- **STREAM snapshots:** per-stream salted key, counter nonces with a last-chunk flag, AAD binds the
  relay sequence, header sequence re-checked (`envelope/stream.rs:49-191`,
  `engine/snapshots.rs:180`). Truncation, reordering and re-labelling fail.
- **Chain:** replay, fork and gap are each detected per author; own envelopes are judged against a
  log that is never pruned, so the relay cannot force a re-join or a quarantine (`chain/mod.rs:125`).
- **Pairing against a relay alone:** the shower's key travels only in the QR; low-order keys are
  refused; slot 2's AD binds direction, mailbox and both keys; confirm gates hold against
  out-of-order IPC; expiry is enforced on the device.
- **Recovery code:** 384 random bits, checksum is typo detection only, no input echoed in errors.
- **Join atomicity:** one savepoint; the snapshot is fully read and checked before any write; a
  backup failure stops before any relay call.
- **HTTP:** https only (http for loopback), no redirects, capped bodies, no certificate overrides,
  errors carry no URL, body or token (`relay/http.rs:74-137`, `:280-318`).
- **Secrets in Rust:** redacted `Debug`, `Zeroizing` buffers, no secret in release logs.
- **Webview:** no permission for the native copy or binding commands; strict CSP.
- **Release isolation:** no environment variable, file, workflow input or tag makes a release build
  run sync or contact a relay; the fake relay and e2e tests are test-only.
- **Relay server:** token hashes compared in constant time, bodies read under a running cap,
  mailbox write-once/read-once and its 10-minute lifetime, webhook signature and replay window.
- **Dependencies:** `cargo audit` reports only RUSTSEC-2026-0459. `npm audit`: 12 moderate in the
  app, all lint/build tooling (0 with `--omit=dev`); 0 in the relay.

## Test results

- Nine added PoC tests pass in the scratch worktree (they assert the faulty behaviour).
- The two existing tests cited under H2 and M7 pass.
- The M2 grind test (`#[ignore]`) passes: it found a scanner key whose six digits equal the phone's
  after 16,605,568 loop steps in 7,115 s. The loop counter overstates the work eight times over,
  because X25519 clamping maps eight consecutive seeds to one key, so about 2.1 million distinct
  keys were tried (the expected number is 10^6). The run was one thread of an unoptimised test
  build, about 2,300 derivations a second, which does not fit the mailbox's 10-minute lifetime.
  An optimised build on several cores should do 10^6 derivations in seconds; that figure is an
  estimate and was not measured. The test proves that a matching key exists and is accepted by the
  showing device, not that the attack fits the window.
- Relay suite on `feat/dev-grant`: 97 pass, 1 fails (M13).

## Residual risks

**Reject before release**
- H1 without either rotation or an explicit UI statement.
- H2, H3, H4 and M7: silent data loss.
- M1 and M2: the fix (typed digits, an acknowledgement slot) is small next to the impact.
- M3 and M4 together: a relay fault becomes lost changes.
- M9 until a Windows run proves or disproves it.

**Accept, with the documentation corrected**
- A relay can delay, withhold and fork (M5), once the spec stops claiming this is visible.
- A key-holding device can damage the vault (M6), once removal exists.
- The binding as a tripwire (M10), if it is described as one and fails closed.
- Metadata the relay sees: account-to-vault link, device count, activity timing, padded sizes.
- Kit on the clipboard for up to 60 s on macOS, Windows and iOS; no clear after a crash.
- L1–L6 as availability issues behind a hostile relay or key holder.

**Owed checks on real hardware**
- The error a binding blob from another Mac or iPhone returns.
- A disk or VM clone against the Windows binding.
- Windows: one full join.
- Concealed clipboard types against the current macOS clipboard history.
