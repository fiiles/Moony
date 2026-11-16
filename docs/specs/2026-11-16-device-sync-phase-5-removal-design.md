# Device sync, phase 5: removing a device and changing the key — design

Date: 2026-11-16. Status: approved for building by the owner ("implementuj fázi 5", 2026-11-16);
the decisions marked **Owner decision** below were made by this spec and may be vetoed later
without blocking the build. Sync stays hidden behind the runtime flag (`feature::RELEASED` is
`false`).

Builds on `docs/specs/2026-11-06-device-sync-client-design.md` (§9.2–9.4, §11, §12, §13, §17 phase 5,
and the "As built" sections of §20, which supersede the text above them) and on ADRs 0024–0027,
0029 and 0030. Where this spec and the 2026-11-06 §11.4 (D23) differ, this spec wins: §11.4 was
written before signatures, the self-hosted server and attachments existed.

It is rebased onto `fix/sync-live-test` before building and assumes what that branch changes:
(a) deleting the profile leaves the shared vault alone unless the person ticks an opt-in
(`delete_account(…, delete_synced_data)`), (b) the status-refresh race fix, (c) "Heslo zařízení" is
"Heslo do Moony" / "Moony password" in every locale, (d) manual pairing-code entry exists in release
builds, (e) the holder polls quickly for about two minutes after pairing so the new device appears,
(f) phone list rows wrap instead of truncating. Phase 5 uses (a) for "leaving", (c) in every password
prompt, (d) and (e) in pairing, and (f) in the phone device rows; it re-implements none of them.

## 1. Why

Today a device that is lost, stolen, sold or simply retired keeps the vault key for good, and the
app says so (ADR 0027, audit H1: "a lost device or Kit cannot be cut off yet"). The device list
also only grows: a wiped and re-paired iPhone leaves its old row next to the new one, and nothing
can remove either (the owner saw exactly that in the 2026-11-16 live test). Phase 5 makes a device
removable, makes removal mean something cryptographically (a new key the removed device never
gets), and says honestly what it does not do.

## 2. What phase 5 is, in one paragraph

Every device gets a signing identity. Each device row in the vault carries a certificate that says
how the device got in (it created the vault, a named device paired it, it used the Recovery Kit, or
it is the re-join successor of an older ID), and every device keeps its own local, pinned list of the
identities it has verified. Removing a device creates a new vault key, seals it separately to every
remaining verified device, signs the bundle, and rotates the server token in one call, after which
the removed device can read nothing new and write nothing. Devices that were offline pick the new
key up on their next round, re-seal what they had not yet sent and carry on. The remover shows a new
Recovery Kit. A device can also leave on its own (turning sync off or deleting the profile) and a
stale row can be taken off the list without changing the key. The sync server needs no change.

## 3. Scope

In phase 5:

1. Device identities, certificates and a pinned local trust store (§5).
2. **Removing a device with a key change** ("Odebrat a vyměnit klíč"), from any other verified
   device, desktop or phone (§6, §9).
3. **Removing a row from the list without a key change** ("Jen odebrat ze seznamu"), for devices that
   left, were superseded, or are known to be wiped (§6.4).
4. **Leaving**: turning sync off and deleting the profile (without the vault) sign a "left" record
   and send it, best effort (§6.5).
5. **Changing the key without removing anyone** ("Vyměnit klíč synchronizace"), for a lost Recovery
   Kit or after list-only removals. Same code path as 2 with nobody removed (§6.6).
6. **Picking up a new key** on every other device, including one that was offline (re-sealing its
   unsent changes), and the 30-day limit (§7).
7. **What the removed device sees** and does (§7.4).
8. **A new Recovery Kit** after every key change, the old Kit refused for joins, the Kit join of a
   vault whose key changed (§8).
9. **Documents and photos across key epochs** (§7.6).
10. Pairing protocol 3: the holder vouches for the joiner (§5.3).

Not in phase 5: §13 lists everything that stays open, with reasons.

## 4. Owner decisions this spec makes

Each is marked where it is argued; collected here so they can be vetoed in one place.

| # | Decision | Alternative not taken |
|---|---|---|
| O1 | Removing an active device changes the key by default; "only remove from the list" is offered too, with a plain warning, and is the default for rows that left or were superseded. | Key change always (a wiped phone's row would then always cost a new Kit), or never offer list-only. |
| O2 | Leaving (turn off, delete the profile) does **not** change the key. It signs a "left" record; the next key change made anywhere leaves that device out. | Leaving rotates: the leaving device would generate, and so know, the new key; pointless. |
| O3 | Vaults set up before phase 5 (all of them are development vaults, the flag is off in every release) are **not upgraded**. They keep syncing; removal and key change are unavailable there, and Settings says to set sync up again once. | A trust-on-first-upgrade migration (one more task, weaker first step). |
| O4 | A row whose certificate this device cannot verify never receives a new key. It is listed as "Neověřené" with the reason; the remedy is to pair that device again. No override. | A tick box "send the key anyway". |
| O5 | After a key change the remover shows the new Recovery Kit as the last step. The step can be closed; a card "Uložte si novou záchrannou sadu" stays at the top of Settings → Sync until the person saves it or ticks that it is saved. | A hard block until the Kit is saved. |
| O6 | A device that receives a **verified** removal notice turns sync off by itself, keeps all local data and says who removed it. An unverifiable rotation (no key for this device, nothing signed) only pauses sync and offers "Vypnout synchronizaci". | Always ask; or always turn off (a server could then tear sync down). |
| O7 | Per-change signatures (author authentication of every envelope) and signed snapshots are **deferred** (§13). Phase 5 signs what decides who gets the key: device certificates, leave and removal records, and key-change bundles. | Sign every envelope now. |
| O8 | "Vyměnit klíč synchronizace" (a key change that removes nobody) is included. | Leave a lost Kit without a remedy. |
| O9 | Whoever changes the key first wins. A stolen, unlocked device can remove the others before they remove it; they then see who did it, keep their data and set up a new vault. No server arbitration. | A server-side owner role. |
| O10 | A device joined with the Recovery Kit is trusted and labelled "Připojeno záchrannou sadou"; nobody vouched for it, and the list says so. | Treat Kit joins as unverified (a person who lost every device would lock themselves out at the next key change). |
| O11 | A superseded row (an older ID of a device that re-joined) receives a new key only if it wrote a change after its successor appeared (then it is a live copy); otherwise it is left out and listed with that reason. | Always include (old backups keep access) or always exclude (a live copy is cut off). |
| O12 | A removal that loses a race against another device's key change is not retried automatically: the person sees the new list and confirms again. | Automatic retry when nobody new would get the key. |

## 5. Trust model

### 5.1 What is protected, against whom

The relay never had the key and still cannot read or forge anything (ADR 0025); nothing here
changes that. The new adversary is a **holder of vault material who should not be able to keep or
regain access**: a lost or stolen device (with its password), an old Recovery Kit, a profile or sync
backup, and a device that was removed. The audit's point (ADR 0027): `sync_devices.public_key` can be
written by any holder of the vault key, so "seal the new key to the listed public keys" is only as
good as that list. Phase 5's answer is to make the list verifiable and pinned:

- **Who can add a recipient of a new key?** Only a device this device already trusts (by pairing),
  the person with the current Recovery Kit (visibly labelled), or a device's own successor after a
  re-join (visibly labelled). Never the server, never an old Kit, never a removed device after its
  removal.
- **Can an honest device's key be swapped?** No: the first verified certificate for a device ID is
  pinned locally and never replaced; a different one is ignored and reported.
- **Can someone make an honest device lose the key?** Only by signing a removal as a trusted
  device, which the list then shows ("Odebralo: …"), or by changing the key first (O9).

### 5.2 Identities

Each device has two key pairs, both generated on the device from the OS random source and stored
in its `sync_state` row (Local, inside the SQLCipher database, like the vault keys today):

- the existing X25519 **box key** (`device_secret_key`), which receives sealed vault keys;
- a new Ed25519 **signing key** (`device_sign_key`, the 32-byte seed), which signs its certificate's
  proof of possession, its leave record, removals it makes and key-change bundles it makes.

A re-join (clone, restore, binding failure) creates both pairs afresh, as it creates the device ID.

### 5.3 Certificates

A certificate binds a device ID to its two public keys inside one vault and says how the device got
in. Bytes (all integers big-endian, every signature Ed25519 over the bytes before it, with the
domain string first):

```
body    = "moony device cert v1" ‖ kind (u8) ‖ vault_id (16) ‖ device_id (16)
          ‖ sign_pub (32) ‖ box_pub (32) ‖ issued_at (i64, unix s)
          ‖ issuer_device (16) ‖ issuer_sign_pub (32)
ticket  = ticket_pub (32) ‖ ticket_issued_at (i64)
          ‖ holder_sig (64)   — by issuer_sign_pub over "moony join ticket v1" ‖ vault_id
                                 ‖ issuer_device ‖ ticket_pub ‖ ticket_issued_at
cert    = body ‖ [ticket, kind 2 only] ‖ issuer_sig (64) ‖ subject_sig (64)
```

| kind | name | issuer | `issuer_sig` made with | Shown as |
|---|---|---|---|---|
| 1 | genesis | itself | its own signing key | "Založilo synchronizaci" |
| 2 | paired | the holder that paired it | the one-time **ticket key** (below) | "Spárováno se zařízením {holder}" |
| 3 | recovery kit | itself | its own signing key | "Připojeno záchrannou sadou" |
| 4 | re-join | its own older ID | the older ID's signing key | "Znovu připojeno (dříve {old name})" |

`subject_sig` is always made by the subject's own signing key (proof of possession), so a record
the device signs later is attributable to it. A certificate is stored base64url in the synced column
`sync_devices.cert`. The vectors of `identity/tests.rs` are the contract between devices.

**The join ticket (pairing protocol 3).** Pairing already has an authenticated, encrypted channel
from the holder to the joiner (slot 2, under the SAS-checked session key, ADR 0026/0027). The holder
cannot learn the joiner's keys first without a new slot (a relay change), so instead it hands the
joiner a **one-time ticket**: a fresh Ed25519 key pair whose public half the holder signs
(`holder_sig`). Slot 2 carries the ticket's secret and the holder's signature; the holder wipes the
secret right after sealing slot 2. The joiner generates its own identity, signs its certificate body
with the ticket secret (`issuer_sig`) and with its own key (`subject_sig`), and wipes the ticket.
Anyone can then verify "the holder vouched for whoever held this ticket", and the joiner's secrets
never left the joiner (the holder generating them was rejected: a holder removed later would know
the joiner's box secret and could open the joiner's share of every later key change).

A ticket is single-use: the first certificate that uses a `ticket_pub` is pinned; a second one is
refused and reported. The ticket is wiped with the received credentials (the same
`RECEIVED_GRACE_SECS` and wiper as the vault keys it travels with), so it never outlives the key it
came with.

### 5.4 The pinned trust store

Each device keeps a Local table `sync_trust` (never synced, never in a snapshot): one entry per
device ID it has verified, with the certificate, the kind, the issuer, a state and the signed
record that justifies the state.

- **First verified certificate wins, forever.** A later, different certificate for the same ID
  (the synced column is write-once, but a forger can win it with an older clock) is ignored and
  logged; the device list shows the pinned one.
- **States only move forward:** `trusted` → `left` → `removed`, or `trusted` → `removed`. `removed`
  is final. Superseded is not a state but a display fact (a re-join certificate names the older ID as
  its issuer).
- **`cut_off_epoch`**: for `left` and `removed`, the first key epoch that device does not have.
  `NULL` means it still knows the current key (a list-only removal or a leave since the last key
  change): the UI says "zná klíč do příští výměny" for those.

**How entries get in** (a pass `trust::refresh` after every merge that touched `sync_devices`, at
unlock and after a key pickup, repeated until nothing new verifies):

| Source | Accepted when |
|---|---|
| Own certificate | always (the device made it) |
| Genesis (the vault creator, at setup) | only its own at setup, or inside a pairing export (below); a genesis certificate arriving later through sync is shown "Neověřené" |
| Pairing export | the joiner receives the holder's whole trust store (certificates and records) in slot 2 over the authenticated channel; it must contain the holder's own certificate and every entry must verify as below; then all of it seeds the joiner's store |
| Paired (kind 2) via sync | the issuer is `trusted` here, the ticket signature and both certificate signatures verify, the ticket is unused |
| Re-join (kind 4) via sync | the issuer (older ID) is `trusted` here and both signatures verify |
| Recovery Kit (kind 3) via sync | both signatures verify (O10); labelled |
| Key-change bundle | the bundle's signer is trusted (§6.3); the remover's own certificate and every recipient's certificate in the bundle are imported as trusted (the signer endorses them), so a device that missed a certificate learns it from the bundle |
| A Kit joiner's first store | trust on first use: every certificate in the restored snapshot whose signatures verify (it has nothing to compare against; §13) |

**Records** (synced write-once columns, verified the same way, then copied into the store):

```
record = "moony device record v1" ‖ type (u8: 1 left, 2 removed) ‖ vault_id (16)
         ‖ subject_device (16) ‖ signer_device (16) ‖ at (i64) ‖ epoch (u32) ‖ sig (64)
```

A `left` record is valid only when signed by the subject itself; a `removed` record only when the
signer is `trusted` here. A verified record naming **this** device that this device did not make
ends sync here (§7.4).

**Recipients of a new key** = the `trusted` entries (this device included), minus those the person
is removing, minus superseded IDs with no change after their successor appeared (O11), each sealed to
the pinned `box_pub`; `left` and `removed` entries never receive one. Every other row is listed in the confirmation with the
reason it gets nothing (§9.2). At most 64 recipients (`validation.syncTooManyDevices`).

### 5.5 Pre-phase-5 vaults (O3)

A vault set up before phase 5 has no certificates. A phase-5 build recognises it by an empty trust
store with sync on: it creates its signing key but no certificate, keeps syncing, lists every row as
"Neověřené", hides "Odebrat" and "Vyměnit klíč", and shows one note in the device card:
"Tuto synchronizaci nastavila starší verze Moony, proto v ní nejde odebírat zařízení. Smažte
synchronizovaná data a nastavte synchronizaci znovu; ostatní zařízení pak připojte." A pairing from
such a holder carries no export and no ticket, so its joiner is in the same state. All of the owner's
current vaults are in this state; setting sync up again also clears the stale "iPhone" row.

## 6. Removing and changing the key

### 6.1 Epochs

The epoch numbers of ADR 0025 (`"MNY1" ‖ epoch ‖ …`, every key kept in `sync_state.vault_keys`) are
used as designed: epoch 0 at setup, epoch n+1 after the n+1-th key change. The relay's rotation
counter is a hint only; epoch numbers come from authenticated bundle headers. A new Local table
`sync_epochs (epoch, rotation_head, adopted_at, by_device)` records, per epoch, the relay position
at which it started (`head` of the rotation; 0 for epoch 0). After the key change to epoch k:

- an op at a relay position above `rotation_head(k)` sealed under an epoch below k, or an op at or
  below `rotation_head(k)` sealed under epoch k or later, stops the pull as `Invalid`
  (problem `invalidChain`): only a misbehaving server produces either;
- an op whose (unauthenticated) header names an epoch above the current one is a **hint** to pick up
  a key (§7.1), never proof.

### 6.2 The bundle (stored opaque by the relay, ≤ 256 KiB)

```
header  = "MNYK" ‖ 0x01 ‖ vault_id (16) ‖ from_epoch (u32) ‖ to_epoch (u32) ‖ eph_pub (32)
entries = count (u16) ‖ count × ( len (u16) ‖ nonce (24) ‖ XChaCha20-Poly1305 ciphertext+tag )
  entry key_i = HKDF-SHA256(salt = vault_id, ikm = X25519(eph_secret, box_pub_i),
                            info = "moony sync rekey seal v1" ‖ eph_pub ‖ box_pub_i ‖ to_epoch)
  entry AAD   = header ‖ index (u16)
  entry plain = 0x01 ‖ new_key (32)                     — a recipient
              | 0x02 ‖ removal record (§5.4)             — a removal notice for a removed device
body    = nonce (24) ‖ XChaCha20-Poly1305 under HKDF-SHA256(new_key, vault_id, "moony sync rekey body v1")
          AAD = header ‖ SHA-256(entries)
  body plain = sig (64) ‖ JSON
  JSON  = {"v":1, "by":"<signer hex>", "at":<unix s>, "reason":"remove"|"rotate",
           "signer":"<signer cert b64>",
           "recipients":[{"device":"<hex>","cert":"<b64>"}],
           "removed":[{"device":"<hex>","record":"<b64>"}],
           "keys":[{"epoch":n,"key":"<b64>","head":h}]}   — every older epoch, with its head
  sig   = Ed25519(signer, "moony rekey v1" ‖ header ‖ SHA-256(entries) ‖ JSON bytes)
```

- **Entries are found by trial decryption** (at most 64 recipients plus notices): the relay learns
  the count, as it already learns the device count, but no device ID.
- **The body opens with the new key only.** Recipients open it after their entry; a Recovery Kit
  holding only the new key opens it directly (that is the back-link of D23, generalised to every
  older epoch); a removed device opens nothing but its notice.
- `keys` carries the older epochs with their rotation heads, so a Kit joiner within 30 days needs
  nothing else (after 30 days the snapshot's Keys record does it, §8.3).
- One ephemeral X25519 pair per bundle; entries are bound to the header and their index, and the
  body to the hash of all entries, so nothing can be cut from one bundle into another.

### 6.3 Opening a bundle (every device)

A bundle is accepted only if: an entry opens with this device's box secret; the body opens with
that key; `vault_id`, `from_epoch` (= this device's current epoch) and `to_epoch` (= from + 1) match;
the signer is `trusted` in the store before this bundle (or its bundled certificate verifies against
the store) and not removed by an earlier bundle; the signature verifies. Otherwise: an entry of type
0x02 that verifies is a removal notice (§7.4); nothing opening, or a failed check, is the problem
`keyNotGiven` or `keyChangeUnverified` (§7.5). On acceptance the device imports the signer's and
recipients' certificates and the removal records into its store (§5.4) and records
`sync_epochs(to_epoch, head)`.

### 6.4 The procedure on the removing device

Under `engine::exclusive` (no round runs), after the password ("Heslo do Moony") and within the
person's consent:

1. **Preconditions.** Sync on, capture not broken, a phase-5 vault (§5.5), this device not removed,
   no key change of its own pending, not removing itself (that is "Vypnout synchronizaci"), and the
   device's epoch equal to the vault's: a round runs first (pull, push, key pickup if needed), so the
   plan is built on the newest list.
2. **Plan and consent.** `sync_removal_plan(devices, mode)` returns the recipients, the excluded rows
   with reasons, the devices added by a device being removed (§9.2), and a consent string: a hash
   of the mode, this epoch, the removed IDs and every recipient's ID and certificate hash. The
   execution rebuilds the plan and refuses a different one with `validation.syncRemovalPlanChanged`
   (the UI reloads and asks again), as replaces do (ADR 0027).
3. **List-only mode** (`listOnly`): sign a `removed` record per removed device with `epoch` = the
   current one, write it into each row (`removal_record`, `removed = 1`; captured, so it syncs), mark
   the store entries `removed` with `cut_off_epoch = NULL`. No relay call. Done.
4. **Key change** (`rotate`): generate the new key; build the bundle (recipients, a notice per
   removed device and per row already removed or left with `cut_off_epoch = NULL`); store
   `sync_state.rotation_pending` (new key, epochs, bundle, removed IDs, the plan) in one transaction;
   forget any pairing session (a holder must not hand out the old key mid-change);
   `POST /v1/vaults/:id/token {token: token(new key), rekey: bundle}` with the current token.
5. **Outcomes:**

   | Answer | What happens |
   |---|---|
   | `200` | **Commit** in one transaction: add the key, `key_epoch` += 1, `sync_epochs` row with the answered head, removal records written to the rows, store entries `removed` (cut off at the new epoch; the same for left and list-removed entries that got a notice), `rotation_pending` cleared. Then re-seal unsent envelopes (§7.2) and run a round. A snapshot upload is due at the first clean round (`snapshot_epoch_due`, like `snapshot_barrier`). |
   | `403 token_read_only` | Lost a race (§10.1): wipe the pending key, pick up the other bundle (§7), answer `validation.syncRotationRaced` (or success "už odebráno" if every device the person removed is now removed). |
   | `409 too_many_rotations` | Wipe pending; `validation.syncTooManyKeyChanges` (32 changes in 30 days). |
   | `400`/`413` | Wipe pending; an internal error (a bug: the bundle is ours). |
   | network, timeout, `5xx`, `408`, `503` | **Ambiguous**: keep `rotation_pending`; the answer to the person is "Dokončí se, až bude server dostupný" (§9.3). |

6. **Resolving a pending change** (at every engine start and at the start of every round, before
   anything else, with no person needed: the consent was given): `vault_info` with the **new** token
   succeeds → it landed: commit (the head comes from `GET /rekey`). `401` with the new token → it did
   not land: send the same `POST` again; `403` then → someone else changed the key: treat as a lost
   race. No round pushes while a change is pending (the status shows `rotationPending`).

### 6.5 Leaving

`sync_turn_off` (password, sync on) and `delete_account` without the vault opt-in (rebased branch
(a)) first sign a `left` record for this device, write it into its row (captured), push it with one
best-effort push of at most 5 s (the Delete Account budget), then do what they do today. Offline or
failed: nothing waits; the row stays active elsewhere until someone removes it. The record does not
change the key (O2). Other devices show the row "Odpojeno {date}" in the folded group, offer only
"Odebrat ze seznamu", and leave it out of the next key change (with a notice entry, harmless: the
device has no box secret any more).

Turning off because this device was removed (§7.4) does not sign a leave record.

### 6.6 Changing the key without removing anyone

`mode = rotate` with no device: the same plan, consent, procedure and Kit step, `reason: "rotate"`.
Offered under the Recovery Kit ("Ztratili jste záchrannou sadu? Vyměnit klíč") and by the note that
appears while list-removed or left devices still know the key ("1 odebrané zařízení zná klíč do
příští výměny. Vyměnit klíč").

## 7. Every other device

### 7.1 Noticing a key change

Three triggers, no extra request per round:
- a push answered `403 token_read_only` (today `SyncError::ReadOnlyToken` → `KeyRotated`);
- a pulled op whose header names an epoch above the current one (today `PullStop::Undecryptable`,
  "never read as a rekey"; now a hint that runs the pickup, after which the op is opened again);
- at engine start, one `vault_info`: `rotation` above `sync_state.rekey_seen`.

### 7.2 Picking the key up

1. **Pull through the rotation point with the old token** (reads are allowed for 30 days). Every op at
   or below the rotation head is processed as today; if the pull stops before the head (a gap, a
   quarantine), the own-envelope scan that already runs behind a newer-schema stop (ADR 0027) runs to
   the rotation head and only acknowledges this device's own envelopes. Without that the pickup waits
   (the round ends unfinished and pushes nothing).
2. `GET /v1/vaults/:id/rekey?after=<rekey_seen>`, all pages; open the bundles in order (§6.3), each
   from the epoch the previous one gave. A device behind by several key changes walks all of them.
3. **Re-seal what was not sent.** In one transaction, in sequence order, every envelope still in
   `sync_unsent` (sealed under an older epoch and, by step 1, not on the server) is opened, its `prev`
   rewritten to the hash of the envelope before it (the first one keeps the hash of the last
   acknowledged envelope), sealed under the new epoch, and stored back with its new hash in
   `sync_unsent` and `sync_own_log`. Same sequence numbers, same rows; the op IDs change. (D23 said
   "same contents"; the chain hash covers the ciphertext, so `prev` must change.) No change is lost
   or applied twice.
4. `rekey_seen` = the newest rotation processed; the round goes on with the new token.

### 7.3 What the person sees on the other devices

A notice, stored as the Local `app_config` key `sync.key_change_notice` (`{by, at, removed: [names]}`)
and carried on `SyncStatus.keyChange`, shown in Settings → Sync (desktop alert, phone block) and as one
line in the top-bar status popover until dismissed:

> **Klíč synchronizace byl vyměněn**
> Zařízení MacBook vyměnilo klíč 16. 11. 2026 a odebralo iPhone. Stará záchranná sada už neotevře
> nová data. Novou si můžete zobrazit tady. [Zobrazit záchrannou sadu] [Zavřít]

No other device is nagged to save the Kit (O5: the remover is); "Zobrazit" opens the existing
password-gated Kit flow, which now shows the current key.

### 7.4 The removed device

- **Verified notice** (an entry of type 0x02 whose record verifies against this device's store), or
  a verified `removed` record naming this device arriving through sync (list-only removal), or a
  verified `left` record for this device that it did not make: the engine stops and sync turns off
  through the existing `setup::turn_off` path (keys, clocks, outbox and binding go; local data stays;
  no leave record). Before that it counts what was never sent (outbox rows and unsent envelopes) and
  writes the Local key `sync.removed_notice` (`{by, at, unsent}`), which survives turning off. Settings
  → Sync then shows, in place of the intro card:

  > **Toto zařízení bylo odebráno ze synchronizace**
  > Odebralo ho zařízení MacBook 16. 11. 2026. Data v tomto zařízení zůstala, synchronizace je tu
  > vypnutá. *(when unsent > 0:)* 3 změny z tohoto zařízení se do synchronizace nedostaly; zůstávají
  > jen tady. [Rozumím] [Připojit znovu]

  "Připojit znovu" opens the existing join choice (pairing or Kit; a join by replace keeps its
  backup `-kept`, so those changes stay recoverable).
- **Nothing opens and nothing verifies:** sync is not turned off (a server alone must never tear
  sync down, the rule of §11.6 for `404`). Problem `keyNotGiven` (§7.5).

### 7.5 New problems

| Problem | Phase | Text (cs) | Action |
|---|---|---|---|
| `keyNotGiven` | error | "Klíč synchronizace byl vyměněn, ale toto zařízení nový klíč nedostalo. Pokud jste ho neodebrali, vypněte tady synchronizaci a připojte zařízení znovu." | "Vypnout synchronizaci" |
| `keyChangeUnverified` | error | "Výměnu klíče nelze ověřit: podepsalo ji zařízení, které toto zařízení nezná. Synchronizace je pozastavená." | none (diagnostics name the signer) |
| `keyRotated` (existing) | waiting | now transient: "Přebírám nový klíč synchronizace…" while a pickup could not finish (offline, the pull did not reach the rotation point) | — |
| `unauthorized` (existing) | error | adds: "Pokud bylo toto zařízení déle než 30 dní offline a klíč se mezitím změnil, připojte ho znovu." | "Vypnout synchronizaci" |

Precedence: `keyChangeUnverified` and `keyNotGiven` rank with `quarantined` (a server cannot mask
them with `relayError`); `serverWentBack` still outranks them.

### 7.6 Documents and photos

- A blob stays sealed under the epoch it was uploaded with (ADR 0030); nothing is re-sealed. Every
  remaining device keeps every epoch's key, so old blobs open; the removed device can still fetch and
  open what it could before (it had those keys) and nothing uploaded after the change.
- An upload interrupted by a key change (`403 token_read_only` at the start or at completion; the
  relay re-checks the token when the body ends) rests until the pickup, then is sealed again from the
  local plaintext under the new epoch: an upload is sealed when it streams, never stored sealed.
- A download of content under an epoch this device does not have yet already reads "on its way"
  (ADR 0030); after the pickup it opens.
- Blob deletion (`DELETE`) needs the current token: a removed device cannot delete anything.

### 7.7 Offline for more than 30 days

The old token is unknown (`401`) and the bundles are gone. The device shows `unauthorized` with the
added sentence above; the way back is "Vypnout synchronizaci", then join (pairing or Kit), whose
replace keeps a `-kept` backup with the changes that never left. Carrying those changes over
automatically is not in phase 5 (§13).

## 8. The Recovery Kit

### 8.1 After a key change, on the remover

The flow's last step shows the new Kit with the password the person just typed (the 5-minute
window of ADR 0027 opens with it), using the existing panel (save, copy, protection from screen
capture):

> **Uložte si novou záchrannou sadu**
> Stará záchranná sada už neotevře nic, co přibude. Uložte si tuto a starou zničte.
> [Uložit sadu…] [Kopírovat kód] ☐ Novou záchrannou sadu mám uloženou [Hotovo]

"Hotovo" needs the tick (or a completed save through the Rust save dialog, which ticks it). Closing
the dialog without it is allowed (the key already changed and cannot be undone); `SyncStatus.kitOutdated`
(`sync_state.kit_epoch_saved` < `key_epoch`) then keeps a card at the top of Settings → Sync and a line
in the status popover: "Klíč synchronizace se změnil. Uložte si novou záchrannou sadu." [Zobrazit
sadu]. Saving or ticking on this device clears it here; other devices never show it (O5).

### 8.2 The Kit page

The Kit page and text gain one line, so two Kits can be told apart: "Sada č. {epoch + 1} · platí od
{date}". The code itself is unchanged (vault ID and the current key).

### 8.3 Joining with a Kit after a key change

ADR 0026 refused a vault whose `rotation` is not 0 (`syncRecoveryKitRotated`). Now:

1. Fetch the bundles (the Kit's token is current, or previous for 30 days). If the newest bundle's
   body opens with the Kit's key, the Kit is current: its epoch is that bundle's `to_epoch`, and the
   bundle's `keys` give every older epoch and head.
2. Else, if `rotation` is 0, the Kit is epoch 0, as today.
3. Else fetch the snapshot: if it opens with the Kit's key, the Kit is current and the snapshot's
   **Keys record** (new: every older epoch with its key and rotation head) gives the rest.
4. Else the Kit is an old one: `validation.syncRecoveryKitRotated`, reworded: "Tato záchranná sada
   je starší: klíč synchronizace se od jejího uložení změnil. Použijte novější sadu nebo zařízení
   připojte spárováním."

A current Kit whose vault has no snapshot under the current epoch and no bundle younger than 30
days (the remover's snapshot upload kept failing) answers
`validation.syncRecoveryKitNeedsSnapshot`; a residual (§13).

### 8.4 Snapshots carry the older keys

D16's "every older vault key epoch" is built now: a snapshot record `Keys` with every epoch this
device has (key and rotation head), inside the sealed stream. A replace restore adds them to the
joiner's keys and `sync_epochs`; a merge restore after `410` adds what is missing. A snapshot whose
Keys record disagrees with a key the device already has for that epoch is refused
(`validation.syncSnapshotInvalid`). Pairing's slot 2 already carries every epoch; protocol 3 adds the
heads.

## 9. Interface

Everything follows `docs/design-system/README.md`: desktop `SettingsCard` / `SettingsRow`, dialogs
560 px with "Zrušit" left and the primary named like the title right, destructive solid only inside
the confirm step; the phone's `EntityRows`, sheets, and rows that wrap (rebased branch (f)). Czech is
binding; the English column is the source for the other eight languages.

### 9.1 The device list

Order: this device first, then active devices by newest change, then a folded group
"Odpojená a odebraná zařízení (n)" (desktop `Collapsible`, phone a row that opens a sheet).

| Row | Badges (desktop) / sub-line (phone) | Hint line | Action |
|---|---|---|---|
| This device | "Toto zařízení" | platform · how it joined · last synced · version | "Přejmenovat" |
| Active, verified | — (plus today's "Změny pozastaveny", "Žádné změny … za N dní") | platform · "Spárováno se zařízením MacBook 3. 11. 2026" / "Připojeno záchrannou sadou 5. 11." / "Založilo synchronizaci" / "Znovu připojeno (dříve iPhone)" · last change · version | "Odebrat…" |
| Active, unverified | "Neověřené" | + "Toto zařízení neumí ověřit, kdo ho přidal. Nový klíč nedostane; připojte ho znovu." | "Odebrat…" (list-only preselected) |
| Superseded | "Nahrazeno" | "Znovu se připojilo jako {new name} {date}" | "Odebrat…" (list-only preselected) |
| Left | "Odpojeno" (folded group) | "Odpojeno 12. 11. 2026" | "Odebrat ze seznamu" |
| Removed, still knows the key | "Odebráno · zná klíč do příští výměny" (folded) | "Odebralo MacBook 16. 11." | "Vyměnit klíč" |
| Removed, cut off | "Odebráno" (folded) | "Odebralo MacBook 16. 11. · nový klíč nemá" | — |

"Last synced" stays derived (ADR 0026): this device's last successful sync, another device's newest
clock. On the phone, other devices' rows become tappable and open a device sheet (name, the hint
facts as label-value rows, state) with "Odebrat zařízení" as a danger text button at the bottom.

In a pre-phase-5 vault (§5.5) the rows show no actions and the card shows the O3 note. The old
`limits.lostDevice` text ("cannot be cut off yet") is replaced by: "Ztracené zařízení odeberte v
seznamu výše: nový klíč nedostane a nic nového nepřečte. Ztracenou záchrannou sadu zneplatníte
výměnou klíče." Every other text that says a device cannot be cut off is reworded (the Kit screens,
`limits.*`; `turnOff.noCutOff` stays true and stays).

### 9.2 "Odebrat zařízení" (desktop dialog, phone sheet of the same steps)

**Step 1 — what to do** (title "Odebrat zařízení {name}?"), two choice cards:

- **Odebrat a vyměnit klíč** *(badge "Doporučeno" for an active verified device)* — "{name} hned
  přestane dostávat nová data a nebude moct nic změnit. Data, která už má, mu zůstanou. Ostatní
  zařízení dostanou nový klíč a vy si uložíte novou záchrannou sadu."
- **Jen odebrat ze seznamu** — "Pro zařízení, které už nemáte, bylo vymazané nebo se znovu
  připojilo. Klíč se nemění: kdyby se zařízení znovu zapnulo, mohlo by číst nová data až do příští
  výměny klíče."

For a left row only the second card exists; for superseded and unverified rows the second is
preselected.

**Step 2 — who gets the key** (only for the key change):
- "Nový klíč dostanou:" each recipient as a row: name (and "Toto zařízení"), platform, how it joined,
  last change. This is the list the person is trusting.
- "Nový klíč nedostanou:" the removed device(s) and every excluded row with its reason ("odpojeno",
  "odebráno", "neověřené", "nahrazeno a od té doby bez změn", "chybí klíč").
- Devices the device being removed added (issuer = it): a note "Tato zařízení přidal(o) {name}:" and a
  tick box per row "Odebrat také" (unticked). Ticking rebuilds the plan.
- "Heslo do Moony" field (focused), then the primary **"Odebrat zařízení"** (destructive) — or
  "Odebrat ze seznamu" in list-only mode, which skips the recipient lists.

**Step 3 — progress** (the dialog body, no busy modal: editing is not blocked, nothing is replaced):
"Připravuji nový klíč" → "Ukládám ho na synchronizační server" → "Hotovo". Then step 4.

**Step 4 — the new Kit** (§8.1).

**Failures:**

| Case | Message | Buttons |
|---|---|---|
| Wrong password | field error (existing key) | — |
| Plan changed meanwhile | "Seznam zařízení se mezitím změnil. Zkontrolujte ho a potvrďte znovu." (back to step 2, reloaded) | — |
| Lost race (O12) | "Mezitím klíč vyměnilo jiné zařízení ({name}). Zkontrolujte seznam a zkuste to znovu." — or, when every chosen device is already removed: "{name} už odebralo jiné zařízení." | "Zkontrolovat znovu" / "Zavřít" |
| Server unreachable, change pending | "Synchronizační server teď není dostupný. Odebrání se dokončí samo, jakmile bude; nemusíte nic dělat." The row shows "Odebírá se…" until then. | "Zavřít" |
| 32 changes in 30 days | "Klíč se za posledních 30 dní měnil příliš často. Zkuste to později." | "Zavřít" |
| Too many devices (> 64) | "Klíč lze předat nejvýš 64 zařízením. Nejdřív odeberte zařízení, která už nepoužíváte, ze seznamu." | "Zavřít" |

### 9.3 Status line and popover

`rotationPending` reads "Dokončuje se výměna klíče…" (phase syncing); `kitOutdated` and
`keyChange` add one line each to the top-bar popover (§7.3, §8.1).

### 9.4 Strings and languages

Every string in all ten locales in the task that introduces it, with each language's plural forms
(the unsent count). "Heslo do Moony" / "Moony password" everywhere (branch (c)). "Synchronizační
server", never "relay". Glossary terms added in `docs/standards/i18n-glossary.md`: klíč
synchronizace (sync key), výměna klíče (key change), odebrat ze seznamu (remove from the list),
odpojeno (left), nahrazeno (superseded), neověřené (unverified).

## 10. Concurrency and failure

1. **Two removals at once.** Both build from epoch n; the relay serialises the two token calls and
   the second gets `403 token_read_only`. Its device picks up the first bundle: if it is a recipient
   it shows the lost race (O12) with the new list; if the first removed it, it is removed (§7.4).
2. **A removal while another device is offline with unsent changes.** It pulls through the rotation
   point with the old token (acknowledging what the server had), picks the key up, re-seals the rest
   under the new epoch and pushes (§7.2). Its changes arrive; none twice.
3. **A removal while another device is mid-pairing.** The remover forgets its own pairing session
   (§6.4 step 4). A joiner paired by a third device with the old key, whose certificate the remover
   had not seen, gets `keyNotGiven` and pairs again.
4. **A crash mid-change.** Before the `POST`: the pending change is resolved at the next start (sent
   then). After the `POST`, before the commit: resolved by the new-token probe and committed. After
   the commit, before the re-seal: the re-seal runs in the first round (it is idempotent: envelopes
   already under the current epoch are left alone). Before the Kit was saved: `kitOutdated` stays.
5. **The relay refuses.** §6.4 table. `402` never applies to the token call (no entitlement needed:
   a person whose access lapsed or was blocked can still cut off a lost device); pushes of the
   re-sealed envelopes wait for entitlement as any push does.
6. **Quota.** The bundle is at most about 50 KiB (64 recipients); the relay's 256 KiB cap and 32
   bundles per 30 days are the only limits (`409` handled). The snapshot due after a change counts
   against the vault's quota like any; if it keeps failing, Kit joins work for 30 days (bundles) and
   then fail with `syncRecoveryKitNeedsSnapshot` (§13).
7. **A forged certificate or record.** Ignored when it does not verify; when it verifies against an
   issuer that is trusted (a device that was stolen, before its removal), it is shown with its issuer,
   and removing that issuer offers to remove what it added (§9.2).
8. **A server that lies about rotations** (fake counter, junk bundle): nothing opens or verifies, so
   the device shows `keyNotGiven` and does not turn sync off. A server could always stop sync;
   it cannot remove a device.
9. **A contested removal (O9).** A stolen, unlocked device removes the honest ones first: they see
   "Odebralo ho zařízení {thief's device}", keep their data, and set sync up again (a new vault;
   their kept backups hold anything unsent). The old vault stays with the thief until the server's
   admin deletes it.

## 11. Wire, storage and versions

### 11.1 Migration 040 `040_sync_device_removal`

Next free number after 039 at the time of writing; renumber at merge if main gained one.

```sql
ALTER TABLE sync_devices ADD COLUMN cert TEXT;             -- synced, write-once
ALTER TABLE sync_devices ADD COLUMN left_record TEXT;      -- synced, write-once
ALTER TABLE sync_devices ADD COLUMN removal_record TEXT;   -- synced, write-once
ALTER TABLE sync_state ADD COLUMN device_sign_key BLOB;    -- Local: Ed25519 seed
ALTER TABLE sync_state ADD COLUMN rotation_pending TEXT;   -- Local: JSON, secret-bearing
ALTER TABLE sync_state ADD COLUMN rekey_seen INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN kit_epoch_saved INTEGER; -- NULL: the setup Kit (epoch 0)
ALTER TABLE sync_state ADD COLUMN snapshot_epoch_due INTEGER NOT NULL DEFAULT 0;
CREATE TABLE sync_trust (device TEXT PRIMARY KEY, cert TEXT NOT NULL, kind INTEGER NOT NULL,
    issuer TEXT NOT NULL, sign_pub BLOB NOT NULL, box_pub BLOB NOT NULL,
    ticket_pub BLOB UNIQUE, state TEXT NOT NULL CHECK (state IN ('trusted','left','removed')),
    state_record TEXT, state_by TEXT, state_at INTEGER, cut_off_epoch INTEGER,
    pinned_at INTEGER NOT NULL);
CREATE TABLE sync_epochs (epoch INTEGER PRIMARY KEY, rotation_head INTEGER NOT NULL,
    adopted_at INTEGER NOT NULL, by_device TEXT);
INSERT INTO sync_epochs (epoch, rotation_head, adopted_at)
    SELECT key_epoch, 0, strftime('%s','now') FROM sync_state WHERE key_epoch IS NOT NULL;
```

- The three `sync_devices` columns are Synced and write-once (registry `write_once`), with the own-ID
  stamp rule extended from `public_key` to `cert` and `left_record`. `removed` (existing, unused until
  now) is set with `removal_record`.
- `sync_trust`, `sync_epochs` and every new `sync_state` column are Local; the export skips them like
  every sync table (§11.4 of the client spec); `turn_off` deletes them with the rest.
- Additive: the barrier stays; the schema number becomes 40, so builds without this migration stop
  pulling at this build's envelopes (`NewerSchema`) and refuse its snapshots. All devices of a vault
  update together; sync is unreleased.
- `docs/architecture/database.md` and `db/schema.snapshot` are updated.

### 11.2 Formats and their versions

| Format | Change | Versioning |
|---|---|---|
| Op envelope `MNY1` | none; the epoch byte is now used | — |
| Changeset JSON | none | — |
| Snapshot record stream | new record `Keys` | schema 40 refuses older readers |
| Device certificate, record | new (§5.3, §5.4) | domain strings `… v1`, vectors |
| Rekey bundle `MNYK` | new (§6.2) | version byte 0x01, vectors |
| Pairing | slot 2 JSON `v: 2` adds `identity: {holder, export: [...], ticket: {secret, holderSig, issuedAt}}` and `head` per key; cap 48 KiB (the relay's slot is 64 KiB) | QR `moony-pair:3:`; a `:2:` code answers "the other device needs an update", `:4:` and above "this device needs an update"; vectors for slot 2 v2. The acknowledgement, the HKDF info and slots 1 and 3 are unchanged. |
| Recovery Kit | one text line (§8.2) | the code is unchanged |

### 11.3 The sync server

**No change.** Everything phase 5 needs exists in the self-hosted server's main (`DESIGN.md`):
`POST /v1/vaults/:id/token {token, rekey}` (current token, no entitlement, one transaction, the
retired token read-only for 30 days, `403 token_read_only` for a second concurrent rotation, `409
too_many_rotations`), `GET /v1/vaults/:id/rekey?after=&limit=` (current or previous token, oldest
first, 30 days), `rotation` in `GET /v1/vaults/:id`, blob reads with a previous token and blob writes
and deletes with the current token only, the token re-checked when an upload's body ends. The app
gains the two calls in `relay/` (`Relay::rotate_token`, `Relay::rekeys`), the HTTP client, and the
fake relay with the same rules (previous tokens read-only, 30-day expiry on a test clock, the
32-bundle cap, serialised rotations).

### 11.4 Code layout

- `services/sync/identity/`: `cert.rs` (bytes, sign, verify), `record.rs`, `trust.rs` (the store,
  `refresh`, recipients, export and import), `mod.rs` (own identity: create, re-join, Kit).
- `services/sync/rekey/`: `bundle.rs` (build, open), `rotate.rs` (plan, consent, pending, commit,
  resolve), `pickup.rs` (triggers, walk, adopt), `reseal.rs`.
- `commands/sync_devices.rs`: `sync_removal_plan`, `sync_remove_devices`, `sync_kit_saved`,
  `sync_dismiss_notice`; `sync_list_devices` returns every row with its state (it filtered
  `removed = 0` before).
- Wire (`shared/schema.ts`, the seven steps): `SyncDevice` gains `state`
  (`active | left | removed | superseded`), `verified`, `joinedVia`
  (`created | paired | recoveryKit | rejoined | unknown`), `addedBy`, `stateAt`, `stateBy`,
  `keepsKey`, `pending` (a removal of it is pending); new `RemovalPlan`, `RemovalOutcome`;
  `SyncStatus` gains `removalSupported`, `rotationPending`, `kitOutdated`, `keyChange`,
  `removedNotice` (readable with sync off), `keptKeyCount`; `SyncProblem` gains `keyNotGiven`,
  `keyChangeUnverified`.

## 12. Security properties after phase 5

After a completed removal with a key change:

- The removed device, an old Recovery Kit and any backup made before the change cannot open any
  envelope, snapshot or blob sealed after it, and cannot push, upload, delete a blob or delete the
  vault (the token they derive is read-only for 30 days, then unknown).
- They keep everything they already had, and the removed device keeps its local data. Phase 5
  cannot take back what was already read; nothing can.
- The new key reached exactly the devices on the confirmation list, each through the box key pinned
  when this device first verified it.
- Nobody, the server, an old Kit or a backup included, can add a recipient of a future key without a
  row appearing in the list that says how it got in ("Spárováno se zařízením X", "Připojeno
  záchrannou sadou", "Znovu připojeno").

What it does not do:

- Before it is removed, a stolen unlocked device (with its password) can do anything a device can:
  add devices, change the key and remove the others first (O9).
- A profile backup made while sync was on holds that device's identity as well as its keys; restored,
  it re-joins as that device's successor (a new, labelled row) or acts as it.
- Changes are still not signed per author (O7): any holder of the current key can still write rows
  that claim another device as their author; only who gets the key is authenticated.
- A Kit joiner trusts what it restores on first use.

## 13. Not in phase 5 (what is still missing)

1. **Author authentication of changes and snapshots** (O7; ADR 0027 H1's second half, M6). Each
   envelope and snapshot signed by its device and verified against the pinned key. Needed before a
   mass-deletion guard can say who deleted. Cost: a signature per envelope and per snapshot, verify on
   every pull, a story for envelopes of re-joined and removed authors.
2. **A mass-deletion guard** (ADR 0027 M6): a changeset that deletes most of a table is applied today.
3. **Re-seeding a vault after a server rollback** (ADR 0027 `serverWentBack`, manual recovery only).
4. **Account continuity** (ADR 0027): deleting the synced data or cancelling a setup forgets the
   account number.
5. **Carrying unsent changes across a re-join after more than 30 days offline** (§7.7): they stay in
   the kept backup and must be entered again.
6. **Upgrading pre-phase-5 vaults in place** (O3).
7. **Verifying an "Neověřené" row** without pairing it again (comparing fingerprints on two screens).
8. **Arbitrating a contested removal** (O9): no owner role on the server.
9. **Hardware-backed identity keys** (ADR 0027 M10): signing and box keys stay in the database.
10. **A Kit join more than 30 days after a key change when no snapshot under the new key exists**
    (§8.3, `syncRecoveryKitNeedsSnapshot`).
11. **Re-sealing old data under the new key**: pointless against the removed device, which already
    had it; old blobs and envelopes stay under their epochs.
12. **Applying a skipped envelope that arrives later** (ADR 0027 residual, unchanged).
13. **Release work** (client spec §17 phase 6): the Moony cloud and payments, Android, the privacy
    policy and README text, the real-profile dry runs, turning the flag on, the owner's checks on
    real hardware still owed from ADRs 0025–0027 and 0030.
14. **The open owner items of ADR 0027 that are not sync design**: the change-password card and the
    setup and recover mutations still keep secrets in mutation state (L16 follow-up).

## 14. Verification

### 14.1 Rust tests (written first, task by task)

- **Formats:** certificate, record and bundle known-answer vectors; any changed byte fails to verify
  or open; an entry cut into another bundle fails; a body under another header fails; a ticket used
  twice is refused; a certificate whose `subject_sig` does not verify is refused.
- **Trust store:** pinning (a second certificate for an ID is ignored and reported); states only move
  forward; a genesis arriving through sync is unverified; a paired certificate from an untrusted
  issuer stays unverified until the issuer is verified (fixpoint); records of untrusted signers are
  ignored; a pairing export that lacks the holder's certificate is refused; a bundle endorses its
  recipients.
- **Rotation state machine** against the fake relay: every row of §6.4's table, a crash at each of the
  four points of §10.4 (simulated by dropping the process state between steps), the probe that tells
  "landed" from "raced", the consent refusing a changed plan, list-only mode writing records only.
- **Pickup:** the three triggers; a walk over two key changes; re-seal rebuilds the chain (other
  devices accept the re-sealed envelopes, and the acknowledged ones are not re-sent); the epoch rule
  stops a misplaced op; removal notice → turn off with `sync.removed_notice`; nothing opens →
  `keyNotGiven`, sync stays on; untrusted signer → `keyChangeUnverified`.
- **Multi-device scenarios** over the fake relay with real engine rounds (`engine/tests/`, next to
  `join_new.rs`; the changeset-level harness in `convergence/` knows no envelopes or epochs, so it is
  not where these go): A removes C while B is offline with unsent
  changes, then B returns; two removals at once; a removal racing a pairing join; a list-only removal
  followed by a key change from another device; and a property test of random edits, offline
  periods, key changes and removals over four devices: all remaining devices converge, no envelope is
  lost or applied twice, and no device ever holds a key of an epoch after its removal.
- **Kit:** join after one and after two key changes (bundles), after 30 days (snapshot Keys record),
  an old Kit refused, `syncRecoveryKitNeedsSnapshot`.
- **Blobs:** an old-epoch blob opens after a change on a remaining device; an upload refused with
  `token_read_only` is sealed again under the new epoch after the pickup.

### 14.2 Against a real local sync server

`services/sync/relay_e2e_rekey.rs`, ignored, run with the existing recipe:

```bash
cd ../Moony-relay && npm ci && npm run build
DATA_DIR=$(mktemp -d) PORT=8795 SETUP_CODE=moony-e2e-setup-code node dist/main.js &
cd ../Moony-tauri-phase5/src-tauri
MOONY_RELAY_E2E=http://127.0.0.1:8795 cargo test --lib relay_e2e -- --ignored --test-threads=1
```

It covers: a rotation with a real bundle; the old token's pull, rekey fetch and vault read succeed
and its push, blob upload, blob delete and vault delete get `403 token_read_only`; two concurrent
rotations (exactly one `200`); a three-device removal end to end (C cannot open the new ops, B picks
up and re-seals); a Kit join after a rotation; a blob upload whose token rotates mid-body.

### 14.3 Live checks (the controller, before merge)

Two desktop dev instances on their own identifiers and demo profiles (ports 1424 and 1425, never
1420–1423, never the owner's real profile) and the iPhone simulator with a debug build carrying the
flag (ADR 0026's `.cargo/config.toml` recipe, deleted afterwards), against a local sync server with
its own `DATA_DIR`:

1. Set up on desktop A, pair desktop B by typed code (branch (d)) and the phone by QR; check the
   device rows say how each joined and B shows on A within the fast poll (branch (e)).
2. Remove the phone from A with a key change: the confirmation lists A and B; the phone shows "Toto
   zařízení bylo odebráno…", sync off, data intact; A shows the Kit step; B shows the key-change
   notice; edits on A and B still sync; the phone's old token can no longer push.
3. Take B offline, edit on B, remove nothing but change the key on A ("Vyměnit klíč"), bring B back:
   its edits arrive on A.
4. Turn sync off on B: A shows B under "Odpojená zařízení"; remove it from the list.
5. Join a fresh phone profile with the **old** Kit (refused) and the new Kit (joins).
6. Kill A during "Ukládám ho na synchronizační server" (stop the server first, then quit): restart,
   start the server: the change completes by itself.
7. Languages: English, Czech, German, Japanese for steps 1–4 on the desktop and the phone; the phone
   at 360 pt wide with the largest text size (rows wrap, nothing overflows); one pass in dark mode.

## As built (phase 5)

Decisions made while building, in the spirit of the spec; each supersedes the text above it.

**Tasks 1–3 (identities, migration 040, trust store).**

1. **Record shapes are checked at decode.** A `left` record must name its subject as signer; a
   `removed` record must not (removing itself is "Vypnout synchronizaci", §6.4). A paired or
   re-join certificate whose issuer device is the subject itself is refused, as is a genesis or
   Kit certificate whose issuer fields differ from the subject's.
2. **Every certificate and record is checked against this device's vault ID** before it is
   pinned or applied; a certificate in a `sync_devices` row must also name that row's ID.
3. **An issuer vouches when it is pinned with the signing key the certificate names and is not
   `removed`**, so a device that **left** still vouches for what it paired or re-joined before
   (§5.4's table says "`trusted`"). Reason: pairing a new phone from an old one and then retiring
   the old one is common; with "`trusted`" only, any device that hears of both at once would leave
   the new phone unverified for good. A device that left signs nothing afterwards (its signing key
   goes with its `sync_state` row). A `removed` record still needs a `trusted` signer, as written.
4. **Order inside one `trust::refresh` pass:** every record is judged against the store as it was
   before the pass (so two records that arrive together, "B removed C" and "A removed B", both
   apply whatever the row order), records apply before certificates (a certificate whose issuer is
   removed in the same pull is not pinned), and the pass repeats until nothing changes. Two new
   certificates that share one join ticket in the same pass: the lower device ID is pinned, the
   other is reported as `TicketReused`.
5. **`RefreshReport.about_me`** holds every verified removal record naming this device and every
   verified `left` record naming it: this device signs its own `left` record only on its way out,
   after which its store is gone, so one it meets while syncing was made by another holder of its
   key. The store's own entry is never moved; the caller ends sync (§7.4).
6. **Pairing export JSON** (`trust::TrustExport`): `{"entries":[{"cert":"<b64>","state":
   "trusted"|"left"|"removed","record":"<b64>"?,"cutOffEpoch":n?}]}`, unknown fields refused, at
   most 256 entries. `import_export` seeds an **empty** store only (the join clears or starts
   empty); the holder must be in it as `trusted`; a `trusted` entry carries no record and no
   cut-off; a `left` record must verify with its subject's key, a `removed` one with another
   entry's.
7. **`trust::import_bundle`** never replaces a pinned certificate (the pin wins, logged), skips a
   recipient whose join ticket is already pinned to another ID (logged), and fails as a whole when
   a certificate does not verify or a removal record's signer is unknown.
8. **Signatures** (small API differences from the plan): `import_export(conn, export, holder,
   now)` takes the pin time; `recipients(conn, removing)` takes no time (O11 compares the old ID's
   newest `sync_clock` second with its successor's `issued_at`); `mark_left(conn, record)` and
   `mark_removed(conn, record, cut_off_epoch)` take the subject from the record and verify it
   (`mark_removed` needs a `trusted` signer). `recipients` lists rows without a pinned certificate
   as `Unverified` when they have a public key and `NoKey` when they have none; a pinned entry is
   always sealed to its pinned `box_pub`, whatever the row's `public_key` says.
9. **Merge:** `removal_record` keeps the older value whoever stamped it; `cert` and `left_record`
   follow `public_key`'s own-ID stamp rule (migration 040, `merge/columns.rs`).
10. **`validation.syncTooManyDevices` got its text in all ten locales with Task 3**, not Task 16:
    the Rust gate `every_sync_validation_key_has_an_english_translation` fails as soon as a key
    appears in Rust code. Czech and English are §9.2's; Task 16 finds it already there.

**Task 4 (this device's identity).**

11. **`identity::create_own(conn, issuer, now)`** takes the kind from the issuer:
    `Issuer::{Genesis, RecoveryKit, Ticket {..}, Predecessor {..}}` (borrowed secrets, so a join
    can keep its ticket for "Try again"). It signs with a fresh seed, stores it and pins the
    certificate in one savepoint; a device whose own certificate is already pinned gets that one
    back (a resumed setup signs nothing new). `identity::create_uncertified` is the seed without a
    certificate (§5.5).
12. **A re-join certifies only when the old ID's own certificate is pinned here**
    (`identity::predecessor`): a pre-phase-5 profile that got a seed at unlock has no pinned
    certificate, and an ID nobody could verify vouches for nobody. Its successor gets a seed and
    no certificate.
13. **A repair and a re-sync are re-joins too.** Both replace the profile under a new device ID;
    the old ID vouches for the new one with a re-join certificate, as at a clone, and the trust
    store stays. Without this a supported device would lose removal after a re-sync.
14. **A join starts its store over.** `join_new_device` takes a `JoinIdentity` (`RecoveryKit`,
    `Unvouched` for a holder that sent no identity; Task 5 adds the paired one); the replace
    clears `sync_trust` before it seeds anything. A Kit joiner then runs `trust::tofu_from_rows`
    after recording its own row.
15. **"At unlock" is the engine's start** (the runtime loop's `start`, after the copy check, holding off
    every round): an enabled profile without a seed gets one (`identity::ensure_seed`); a
    failure is logged and sync goes on.
16. The `sync_epochs` row for epoch 0 of a new setup or join is not written here (Task 8).

**Task 5 (pairing protocol 3).**

17. **Slot 2 v2 JSON:** `{"v":2,"relayUrl","accountNumber","vaultId","keys":[{"epoch","key",
    "head"?}],"identity":null | {"holder":"<hex>","export":{"entries":[…]},"ticket":{"secret",
    "holderSig","issuedAt"}}}`. `export` is `trust::TrustExport` as it serialises (item 6), not a
    bare array; the ticket's public key is not sent (it is the secret's). A key whose head the
    holder does not know travels without `head` (the holder writes epoch 0's as 0 and every
    epoch `sync_epochs` records); `identity` may be `null` or absent. Only `v: 2` opens.
18. **A store that does not fit:** the 48 KiB cap holds about 70 entries; a holder whose payload
    would exceed it refuses to seal with `validation.syncTooManyDevices` (its text already says
    to take unused devices off the list).
19. **Who reads what:** `pairing::holder::holder_payload` builds the holder's payload (holding
    off every round in `sync_pairing_confirm`), with a fresh ticket per call; the command drops
    it right after `confirm_with`, which wipes the ticket's secret. The joiner's session keeps
    the whole payload (`received`, `received_identity`, `received_heads`) and wipes it with the
    credentials. `confirm(…, Option<&Credentials>, …)` survives only as a test shorthand
    (identity `null`).
20. **A paired join that does not verify is refused** with `validation.syncPairingFailed`
    inside the replace's savepoint (nothing replaced, the backup `-unused`): an export that
    `import_export` refuses, or a ticket the holder's pinned key did not sign.
21. The received heads are not stored yet: Task 8 writes them to `sync_epochs` at the join.

**Task 6 (the bundle's bytes).**

22. **Spelled out where §6.2 was loose:** an entry's `len` counts its ciphertext with the tag
    (the nonce is fixed-length); the body key is `HKDF-SHA256(salt = vault_id, ikm = new_key,
    info = "moony sync rekey body v1")`; the signed JSON is exactly the body's JSON bytes; a
    `keys` item whose head the signer does not know has no `head`.
23. **Entry limits:** at most 64 recipient entries (§5.4) and, separately, at most 64 notice
    entries, so a full vault can still remove a device; the 256 KiB cap applies to the whole
    bundle. A low-order box key refuses the build; on open it reads as `Nothing`.
24. **Decode checks of the body:** `by` is the signer certificate's device; every listed
    certificate and record names the device it is listed under and this vault; older keys are
    32 bytes, below `to_epoch`, each epoch once. `BundleBody::verify` checks only the bundle's
    signature; trust is `import_bundle`'s.
25. **Opening:** the first entry that opens decides. A key entry whose body then fails to open
    or read is an error (the bundle was tampered with), not `Nothing`. A notice is bound to the
    header and its index only, not to the body; its record is signed and the caller verifies it
    against its store (§7.4). `build` takes its random source as
    `&mut dyn FnMut(&mut [u8]) -> Result<()>` (`bundle::os_fill` in production);
    `Opened::Recipient` carries the body boxed.
