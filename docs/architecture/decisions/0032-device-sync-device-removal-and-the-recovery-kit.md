# 0032. Device sync: removing a device, and the Recovery Kit as a paper device

Date: 2026-11-17

## Status

Accepted. Amended by [ADR 0034](0034-device-sync-second-audit-fixes.md) (the signed head binds the range below it; a new threat row; R1, R4 and
O-2 restated; a retired token reads less). Records phase 5 of device sync as built on `feat/sync-device-removal`: the base spec
`docs/specs/2026-11-16-device-sync-phase-5-removal-design.md` ("the phase-5 spec") and its revision
`docs/specs/2026-11-16-device-sync-phase-5-revision-ux.md` ("the revision"), whose decisions D1–D4
the owner took on 2026-11-16. Where they differ, the revision wins, and its "As built (revision)"
list (R-1 to R-117) supersedes the text above it; the phase-5 spec's own "As built" items 1–69 stay
true except where the revision says otherwise. Builds on [ADR 0024](0024-device-sync-capture-clocks-merge.md),
[ADR 0025](0025-device-sync-relay-client.md), [ADR 0026](0026-device-sync-pairing.md),
[ADR 0027](0027-device-sync-audit-fixes.md), [ADR 0029](0029-self-hosted-sync-server-in-the-app.md)
and [ADR 0030](0030-device-sync-documents-and-photos.md). It resolves H1 of the security audit
(ADR 0027: "no revocation") and replaces the Recovery Kit of ADR 0025 and ADR 0027. The sync server
side (Kit routes, migration 7) is the Moony-relay repository (`DESIGN.md` there is authoritative;
branch `feat/recovery-kit`, not merged into the relay's main). Sync is still hidden behind the
runtime flag (`feature::RELEASED` is `false`); everything here lands before the flag is turned on,
and development vaults made before it are not upgraded (the owner sets sync up again).

The code is in `src-tauri/src/services/sync/`: `identity/` (certificates, the pinned trust store
`trust.rs`, the Kit's derivations `kit.rs`), `rekey/` (`rotate.rs`, `bundle.rs`, `envelope.rs`,
`pickup.rs`, `head.rs`, `kit.rs`), `recovery.rs` (Kit code v2), `join.rs`, `setup.rs`, `devices.rs`,
`engine/`, and `commands/sync_devices.rs`; the UI is under `src/components/settings/sync/` and
`src/mobile/components/sync/`. Migrations 040 and 041 are in `docs/architecture/database.md`.

## Context

Until phase 5 a device that was lost, stolen, sold or retired kept the vault key for good, and the
app said so (audit H1, ADR 0027). The list of devices only grew. The first phase-5 design (the
phase-5 spec) removed a device with a key change, but offered a list-only removal, a new Recovery
Kit after every key change, and a confirmation step for devices that a Kit had vouched for. The
owner's review of the result (`docs/specs/2026-11-16-device-sync-ux-review.md`) found it too hard to
use: three kinds of removal, a Kit that has to be re-printed whenever anyone is removed, "klíč
synchronizace" and "výměna klíče" in the interface, devices that dropped out of sync because the
phone they were paired from had left, and endless "Synchronizuji…". The revision answers it with
four decisions and a security review of its own text.

## Decision

**D1: the Recovery Kit is a paper device with its own key.** One printed page: a QR, a 95-character
code (Crockford base32 in 19 groups of five, the account number inside it) and the sync server's
address. The code is `vault_id ‖ kit_seed ‖ account` (55 bytes) plus a 35-bit checksum (format v2;
the 80-character v1 code reads `syncRecoveryCodeInvalid`). Everything the Kit needs comes from
`kit_seed` through HKDF-SHA256: an X25519 box key, an Ed25519 signing key, a Kit token that reads
the envelope from the server (`GET /v1/vaults/:id/kit`, never a vault token) and the Kit's ID. The
seed is never stored: it lives in a `Zeroizing` draft for at most ten minutes while the Kit is shown
(`KIT_DRAFT`), and only derived public keys and the certificate reach the database. So no device,
no removed device and no profile backup can open the envelope or show the Kit again. The Kit
survives every removal and every key change; it is replaced only when lost, seen by someone, or
kept on a device that is being removed ("Vytvořit novou sadu"). "Ověřit sadu" (no password, no
network) replaces viewing it.

The Kit is a recipient of every key change, but not through the `MNYK` bundle (bundles expire after
30 days, which is what the Kit must outlive). The remover seals an **envelope** to the Kit's box key
(`rekey/envelope.rs`: every key epoch it holds, verified heads only, the sealer's `confirmed` set,
signed by the sealer) and sends it in the same `POST /token` call as the rotation, in one
transaction. The bundle (version 0x02) names the Kit's certificate so every receiver pins it. The
Kit is in the trust store as certificate kind 5; a device that joins with it carries a kind-3
certificate signed by the Kit's key, so it can be verified like a pairing. At most one Kit is
`trusted` (`ConflictKind::SecondKit`); a replacement is pinned only through the key-change bundle
that names it or by the device that makes it (R-24, R-58).

Joining with the Kit after any number of key changes (`join::recovery_credentials`): decode,
`GET /kit` with the derived token, open the envelope, take every key, restore the snapshot, anchor
the Kit's own row (it must be named by the envelope's confirmed set, R-60), sign the joiner's
certificate with the Kit's key, and trust on first use only what the Kit's own issuer chain names.
The envelope's confirmed set (and any genesis it names) is taken only when its signer speaks on that
store: pinned with that certificate, `trusted`, not superseded, not left out for its issuer, the Kit's
creator and a remover of its own issuer (that issuer not itself left out, R-112) aside
(`trust::kit_join_speaker`, R-106); any other pinned signer opens the Kit but vouches for nothing
(generalising R-78). A replaced Kit is pinned on the joiner `removed` by its row's removal record only
when that record's signer is pinned with its certificate and speaks at the record's time, or joined
with that very Kit (R-105, tightened by R-112). The newest epoch is never left unchecked (a recorded head, the snapshot's own
epoch as a floor, or an unverified-epoch entry that holds old-epoch ops until the signer's signed
head record verifies). Then the **push hold**: the joiner pulls but pushes nothing until the person
answers "Data jsou zpět" (R-45).

Where the Kit is kept decides whether a removal replaces it (`sync_devices.kit_storage`, migration
041, merged by strength so sync never lowers it, R-54): `paper` never, `copied` and `file` when the
creator is removed, `cloud` for any removal. Kit use is visible: the server records the minute of the
last Kit read (`kit.lastRead`), the status carries `kitUsed` when a read has no join on this device
that explains it, and a Kit join gives every other device a strong notice.

**D2: removal always changes the key.** One button, "Odebrat zařízení". `RemovalMode` and the
list-only path are gone; "klíč synchronizace" and "výměna klíče" leave the UI (the glossary now
forbids them). A removal that makes no sense on its own (nobody removed, no new Kit) is refused.
The plan lists who keeps syncing, who also drops out and why, and every descendant of the removed
devices (devices a removed device paired, what those paired, and Kit-joined devices when the Kit is
being replaced), with "Odebrat také" ticks that start unticked; the consent hash covers all of it.
Costs, accepted: the 32-changes-per-30-days cap now counts every removal, and a device offline for
more than 30 days re-joins after any removal.

**D3: a device vouched for by a trusted device is confirmed automatically.** `trust::recipients`
includes every pinned `trusted` entry except a closed list of reasons (being removed, `Left`,
`Removed`, `SupersededQuiet`, `Unverified`, `NoKey`) and four issuer-based exclusions:
`IssuerRemoved` (transitive: a whole chain hanging off a removed device drops, however deep; a
`confirmed` entry stops it), `LateVouch` (below), `IssuerUnknown` (R-23) and, for a left or
superseded issuer, the log-order check. `confirmed` now means "named as a recipient by a verified
key change, or kept by a person on this device"; the person-signed confirmation records and
`kit_unconfirmed` are withdrawn. A device left out for its issuer gets no key from this device and
speaks for nothing on it (R-22). "Je moje" (`sync_keep_device`, no password) keeps a "Zkontrolujte"
row.

The R2 mitigation is a log-order rule. A paired certificate whose issuer has since left counts only
when its holder announced the pairing in its own chain before leaving: the holder queues an envelope
with the new changeset field `"tickets"` (the ticket public key) at the confirm, which always reaches
the server before a later `left` record of the same device, and every receiver records where in the
relay's log it first saw each certificate, `left` record and announcement (`sync_trust_seen`,
`sync_tickets`, both Local). Equal positions are late, an unknown position is in time, and a pairing
export carries the holder's positions so a joiner judges as its holder does. A superseded device ID
speaks only until this device pinned its successor, by this device's own clock (R-25).

**D4: the desktop accepts a pairing code too.** The join choice has three options: "Mám telefon se
synchronizací" (this computer shows a QR), "Mám jiný počítač se synchronizací" (a typed or pasted
pairing code) and "Nemám u sebe žádné zařízení" (the Kit); the holder's dialog gets "Zkopírovat kód".
The holder finishes visibly: the dialog watches the device list and ends with "{name} je připojené".
The Kit is for emergencies.

**Security verdict (§0 of the revision).** None of D1–D4 is unsound. Three of them trade a property
away, and each trade is a residual below: D1 costs freshness at a Kit join (R1) and cuts off a copied
Kit only by a new Kit; D3 trusts a vouch made before the issuer left (R2); D2 and D4 cost
availability only. What a dishonest server can still do around a key change (R4, R5) was true
before the revision and is now stated for the owner.

**Threats (§2.10 of the revision).**

| Attacker | Can | Cannot | Detected by |
|---|---|---|---|
| Stolen or copied Kit (with the server address) | Read everything, past and future, until "Vytvořit novou sadu"; join as a device | Survive "Vytvořit novou sadu": the new key and envelope are sealed to keys it lacks, its token answers `410` | `kitUsed`, the Kit-join notice on every device |
| Stolen Kit plus dishonest server | The above, and the server can hide the read | Survive a new Kit: the cut-off is cryptographic | The Kit-join notice only |
| Server alone | Deny, delay; replay an older genuine envelope with matching older data to a Kit joiner (R1) | Read anything (it never sees a key, the Kit's public key or the seed); read what the joiner writes | The push hold and "Data jsou zpět" |
| Removed device (old keys, signing key) | Nothing new: no Kit token unless it kept the Kit file or code (which is why the Kit is then replaced) | Read the envelope; write (its token is read-only, then unknown); add a device that gets keys | none needed |
| Removed device plus dishonest server | R1; R4: fill not-yet-pulled slots with its own later changes for a lagging device | Read data written by devices that stayed | "Data jsou zpět"; `keyChangeOverrun` in part |
| A device that left, or a superseded ID, from a backup with its signing key | Until the next key change, everything (O2); after it, nothing unless a person keeps a device it adds | Add a device that silently gets keys: a certificate it backdates appears after its `left` record, so it is "Zkontrolujte" and excluded | "Zkontrolujte" row, new-device notice |

**The sync server (Moony-relay).** Schema migration 7 adds `kits` (one per vault: the SHA-256 of the
Kit token, the opaque envelope, `since`, `updated_at`, `last_read`) and `kits_retired` (the newest 256
hashes). `PUT /v1/vaults/:id/kit` sets a Kit up; `GET` reads it with the Kit token only (`410
kit_replaced` for a replaced one, `401` for anything else, a vault token included); `POST /token`
gains `kit: {envelope, token?}` and is required on a vault that has a Kit (`400 kit_required`);
vault info gains `kit: {since, lastRead}`; `/v1/server` lists the feature `kit` and the app refuses
setup on a server without it. Failed Kit reads are limited per client and per vault, but a successful
read never waits on them, so nobody without the Kit can lock the owner out of recovery. The admin shows per profile when the Kit was made and last used.

**Smaller decisions.** `left` records stay (O2: leaving does not change the key); a device that
leaves sends a best-effort signed `left` record and the row moves to "Dřívější zařízení" by itself,
listed for 30 days. The v1 Kit, `kit_unconfirmed` and person-signed confirmations are gone (the
column and record type stay in the format, unused). Three device states: active, "Zkontrolujte"
(`unverified`, `copy`, `issuerRemoved`, `lateVouch`) and folded "Dřívější zařízení". Every status
says what happened, that local data is fine and offers one button; a round longer than 60 s says so
("Zkusit znovu"). A removed device sees a notice at unlock, in Settings and in "Stav dat", and keeps
its data.

## Consequences

- A person who loses every device gets the data back with one sheet of paper and the sync server's
  address, however many removals happened since it was printed.
- Removal is one button, one dialog, one meaning. Nobody has to know what a key change is.
- The Kit's seed is never in the database, logs or IPC outside `RecoveryKit`; the final review checks
  it (Task 33).
- A copied Kit cannot be cut off by removing a device, only by a new Kit; the app makes its use
  visible but cannot stop it.
- The wire grows: bundle v2, the envelope, the changeset field `"tickets"`, certificate kind 5 and a
  re-meant kind 3, the commands of the revision's §6.5, migration 041 (`kit_storage`, `sync_trust_seen`,
  `sync_tickets`). The Kit join and the removal need the relay's `kit` feature.
- Sync server operators must run a relay with migration 7 and the Kit routes before the flag is turned
  on.

**Residuals for the owner** (§8 of the revision, plus two found while building). The owner accepted
R1–R5 and the copied-Kit trade on 2026-11-16.

- **R1 — An old copy served to a Kit join (D1).** The Kit cannot check that the server's copy is
  current. A dishonest server (with a removed device, to read the result) can hand someone joining
  with the Kit a stale copy from before a removal. It bites only if the person, at "Data jsou zpět",
  keeps a device on the list that they had removed earlier ("Mám je"); otherwise the joiner's first key
  change leaves it out before anything is sent. The earlier design did not have this.
- **R2 — A device added from a backup of a device that left (D3).** A profile backup made while sync
  was on holds that device's signing key. On an honest server a device it adds after the original left
  shows as "Zkontrolujte" and gets nothing; it bites only if the person answers "Je moje" for a device
  they do not own. On a dishonest server it falls under R4.
- **R3 — Where the Kit is kept is what the app saw.** A screenshot, a photo of the screen, a code
  pasted into a synced note or a PDF saved to iCloud from the print dialog is not known to Moony, so
  removing that device does not replace the Kit. It bites if such a copy sits on a stolen device; the
  remedy is "Vytvořit novou sadu".
- **R4 — Slots before a key change (phase-5 spec As built 65 (b), O7).** The signed head fixes where a
  key change happened, not what fills the positions before it. A dishonest server working with a
  removed device can give a device that was behind the removed device's later changes as if they were
  older ones; Moony applies them and says so afterwards only in part (`keyChangeOverrun`). Only signing
  every change per device (not in phase 5) closes it. It bites only with a dishonest server operator,
  a stolen removed device and a device that was offline over the removal.
- **R5 — Waiting for the remover (phase-5 spec As built 65 (c)).** A device that was behind waits for
  the remover's signed confirmation before it accepts older changes past its place. If the remover
  never syncs again (lost right after removing), that device stays at "Synchronizace čeká na zařízení
  {by}"; the way out is "Připojit znovu" (turn off, join again; unsent changes stay in the backup).
- **A copied Kit is cut off only by a new Kit (D1),** made visible by `kitUsed` and the Kit-join notice.
- **A current-key holder can upload a snapshot that omits a `left` record (R-28, R-66).** A copy of a
  device that left (before the next key change it still holds the vault key) can upload a snapshot
  without that device's `left` record; a device that later joins or catches up from it sees the leaver
  as `trusted`. Log positions cannot catch an omission; it is snapshot integrity, which phase 5 does not
  authenticate. The next key change closes it for devices that take part. On a Kit joiner it reaches
  further (security review of the Kit join, F-2, As built R-112): the anchored Kit's creator speaks
  there even when it is left out for its issuer (R-105's chains need it, a Kit joiner that made the
  next Kit), so a backup of a Kit-making device whose `left` record is omitted from the snapshot can
  confirm devices of its choosing through the Kit's envelope; "Mám je" at the push hold would keep
  them. Accepted by the lead on 2026-11-17 and reported to the owner (not yet confirmed by him); the test
  `accepted_residual_an_omitted_left_record_lets_the_kits_creator_confirm` pins it.
- **"Je moje" only helps before the next key change (R-77a).** A device a key change left out cannot be
  let in later: the pickup stops at the first bundle that gives it nothing, so after the next change the
  way in is joining again. The "Zkontrolujte" row says so ("Potvrďte ho před příštím odebráním zařízení;
  potom by se muselo připojit znovu.").
- Smaller, accepted: on a device that gets a certificate and its issuer's `left` record from one
  restored snapshot, a genuine device paired by one that has since left is "Zkontrolujte" until a key
  change names it or the person keeps it (R-23); a Kit joiner removes a device before the joiner's own
  row went out, so that device reads `keyChangeUnverified` rather than "removed" (R-46); a Kit's
  envelope signer that left, is superseded, is a late vouch or whose issuer another device removed
  still opens a Kit join, but its confirmed set vouches for nothing (R-75, R-78, R-106), so such a
  device and what only it confirmed show "Zkontrolujte" on the joiner; "Je moje" keeps a late vouch or
  an unknown issuer, never a device whose issuer was removed (R-107). That makes envelope substitution
  stickier: a current-key holder that re-seals the envelope with a key that opens it but does not speak
  (a device that left, say) confirms nothing, and honest devices whose issuer was removed (Kit joiners
  of a replaced Kit, devices kept through a removal) are "Zkontrolujte" on the joiner and cannot be
  kept: the joiner's first key change cuts them off and they join again. A denial of service only,
  needing the current key (R-112); printing from the built app is unverified until Task 33 (the primary button may become
  "Uložit jako soubor…").
