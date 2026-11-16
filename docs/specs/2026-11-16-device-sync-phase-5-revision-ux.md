# Device sync, phase 5 revision: the Recovery Kit as a paper device, one removal, a plain device list — design

Date: 2026-11-16. Status: the owner's decisions D1–D4 of 2026-11-16 (taken on the recommendations of
`../Moony-tauri/docs/specs/2026-11-16-device-sync-ux-review.md`, "the review"), worked out for building.
Sync stays behind the runtime flag (`feature::RELEASED = false`); everything here lands **before** the
flag is turned on. Old development vaults are not upgraded: the owner sets sync up again.

This revises `2026-11-16-device-sync-phase-5-removal-design.md` ("the phase-5 spec") and its plan.
Where they differ, this document wins; the phase-5 spec's "As built" items 1–69 stay true except where
§1.3 below supersedes them. Czech texts are binding, English is the source for the other eight locales.

## 0. Security verdict on D1–D4 (read first)

None of D1–D4 is unsound. Three of them trade a property away; each trade is stated here so it is
accepted knowingly, and each is repeated in §8.

1. **D1 weakens one case that today's design covers: a removed device colluding with the sync server
   can impersonate the vault to a later Kit join.** Sealing a key to the Kit needs only the Kit's
   public key, and every device that was ever in the vault since the Kit was made knows it. A removed
   device plus a dishonest server can therefore hand a person who joins with the Kit a forged envelope
   and a stale copy of the data, and then read what that person types afterwards. Today's Kit (it holds
   the current vault key) is immune once a key change happened. This cannot be fixed without giving
   the Kit something new at every key change, which is exactly what D1 removes. It needs a dishonest
   server operator (for a self-hosted server, usually the person themselves) *and* a stolen removed
   device *and* a Kit join; accepted as residual R1 (§8).
2. **D1: a copied Kit is cut off only by "Vytvořit novou sadu"**, never by removing a device (the review's
   §3.5 point 1). Partly compensated by making Kit use visible (§2.8).
3. **D3 trusts a vouch made before the issuer left.** For an issuer that was *removed* this is airtight
   (removal always changes the key, §3.2). For an issuer that *left*, a copy of its signing key made
   before it left (a profile backup made while sync was on) can still add a device that keeps getting
   keys. The As built 34 tick box was the only guard, and it only worked when the person unticked the
   right row; it cost a genuine phone otherwise. Accepted as residual R2, with visibility (§3.3).
4. D2 and D4 are sound. D2's costs are availability ones: the 32-changes-per-30-days cap now counts
   every removal, and a device offline for more than 30 days re-joins after any removal.

## 1. Decisions and what they supersede

### 1.1 The decisions

- **D1.** The Recovery Kit is a **paper device** with its own key. It survives every removal and every
  key change; it is replaced only when lost, or when it was kept on a device that is being removed
  ("Vytvořit novou sadu"). It is shown once and never again; "Ověřit sadu" replaces viewing. A relay
  change (long-lived Kit read access) and a new Kit format; no released users, no upgrade of old vaults.
- **D2.** Removal always changes the key. One button, "Odebrat zařízení"; no "Jen odebrat ze seznamu";
  the words "klíč synchronizace" and "výměna klíče" leave the UI.
- **D3.** A device vouched for by a trusted device is confirmed automatically; a phone paired from an
  old phone never drops out when the old one leaves or is removed.
- **D4.** The desktop accepts a pairing code too (`sync_pairing_scan` exists); the Kit is for emergencies.
- Adopted from the review as well: three device states (active, "Zkontrolujte", folded "Dřívější
  zařízení"); every state says what happened, that local data is fine, and offers one button; no
  endless "Synchronizuji…"; the other recommendations, except the two named in §1.4.

### 1.2 Phase-5 owner decisions

| # | Fate | Now |
|---|---|---|
| O1 (list-only removal offered) | **Superseded by D2** | Every removal rotates. `ListOnly` leaves the API (§4). |
| O2 (leaving does not rotate) | Kept | The row moves to "Dřívější zařízení" by itself; the next key change cuts it off. |
| O3 (pre-phase-5 vaults not upgraded) | Kept, widened | Also phase-5 vaults made before this revision: their v1 Kit is refused (§2.9). |
| O4 (unverified rows never get a key) | Kept | Shown as "Zkontrolujte" (§6). |
| O5 (new Kit after every key change) | **Superseded by D1** | A new Kit only when the Kit is replaced (§2.7). |
| O6 (verified notice turns sync off) | Kept | The notice is also shown at unlock and in "Stav dat" (§6.4). |
| O7 (no per-change signatures) | Kept | — |
| O8 ("Vyměnit klíč" without removal) | **Superseded by D1/D2** | The only rotation that removes no device is "Vytvořit novou sadu". |
| O9 (first key change wins) | Kept | — |
| O10 (Kit joiner trusted, labelled) | **Superseded by D1** | A Kit joiner's certificate is signed by the Kit's key: verifiable like a pairing. |
| O11 (superseded rows) | Kept in substance | A quiet superseded row is hidden; one that still writes is "{name} (kopie)", "Zkontrolujte". |
| O12 (no retry after a lost race) | Kept | — |

### 1.3 Phase-5 text and As built items superseded

| Item | Fate |
|---|---|
| Spec §5.3 kind 3 ("recovery kit", self-issued) | Re-meant: issuer = the Kit (§2.3). New kind 5 = the Kit itself. |
| Spec §5.4 "A Kit joiner's first store" | Kept (trust on first use) plus a Kit anchor check (§2.6). |
| Spec §6.4 step 3 (list-only), §6.6, §8 (whole), §9 (whole), §11.3 ("server: no change"), §13 item 10 | Replaced by §2, §4, §6 and the relay change. §13 item 10 (`syncRecoveryKitNeedsSnapshot`) disappears. |
| As built 14, 43 (Kit join identity, Kit heads) | Amended: `JoinIdentity::RecoveryKit(KitJoin)` carries the envelope's heads (§2.6). |
| As built 29 (`kit_unconfirmed`) | **Withdrawn.** The column stays in migration 040, always 0. |
| As built 33 (person-signed `confirmed` records, `sync_confirm_device`) | **Withdrawn.** Record type 3 and `confirm_record` stay in the format, unused; the command is not built. |
| As built 34 (`IssuerGone` opt-in ticks) | **Replaced** by the rule of §3.1 (`IssuerRemoved`, no tick). |
| As built 53, 54, 56, 57, 60 (removal API with mode, `opted_in`, `confirming`; list-only checks; pending JSON; commit confirms the ticked) | Amended in §4.2 and §7.1. |
| As built 50 ("a signer … not `kit_unconfirmed`") | The clause goes; everything else stands. |
| Plan Tasks 11–24 | Replaced by Tasks 11–32 (§7.3). |

### 1.4 Review recommendations not adopted

- **Default device name from the OS host name** (review §4.1): needs a new dependency (`gethostname` or
  `tauri-plugin-os`) and a cargo audit for one default; the name field is already there. Later.
- **The joining phone shows a code for a holder phone to scan** (review §4.2): phone to phone already
  works the other way round (the holder phone shows, the new phone scans). Nothing to build.

## 2. The Recovery Kit as a paper device (D1)

### 2.1 What the person has

One printed page (or file) with: the header "Záchranná sada Moony · vytvořena 3. 11. 2026 · MacBook",
a QR, a 95-character code in 19 groups of five, the sync server's address in plain text, and the line
"Kdo má tuto stránku, přečte vaše data. K obnově potřebujete i svůj synchronizační server: {adresa}."
The account number is inside the code: one thing to copy, not two.

### 2.2 The code (format v2)

```
payload  = vault_id (16) ‖ kit_seed (32) ‖ account (7, big-endian integer < 10^16)   = 55 bytes = 440 bits
code     = Crockford base32 of payload (88 chars) ‖ checksum (7 chars = 35 bits of
           SHA-256("moony recovery code v2" ‖ payload))                               = 95 chars
QR       = "moony-recover:2:" ‖ code [ ":" ‖ base64url(server address) ]   (address omitted for the default server)
```

`recovery.rs` keeps its alphabet, separators, I/L/O folding and redacting types; only lengths, the label
and the account field change. A v1 code (80 characters) reads `validation.syncRecoveryCodeInvalid`
(only development vaults ever had one). `kit_seed` comes from `keys::os_random`.

Everything the Kit needs is derived from `kit_seed` with HKDF-SHA256, `salt = vault_id`:

| Value | `info` | Use |
|---|---|---|
| `kit_box_secret` (X25519) | `"moony kit box v1"` | opens the Kit envelope |
| `kit_sign_seed` (Ed25519) | `"moony kit sign v1"` | signs the Kit's own certificate (proof of possession) and the certificate of a device that joins with it |
| `kit_token` (43 base64url chars) | `"moony kit token v1"` | reads the envelope from the server (`GET /kit`); never a vault token |
| `kit_id` (16 bytes) | — | `SHA-256("moony kit id v1" ‖ vault_id ‖ kit_sign_pub)[0..16]`: the Kit's ID in `sync_devices` and `sync_trust` |

**The seed is never stored.** It lives in a `Zeroizing` buffer in a process-global draft
(`KIT_DRAFT`, owned by the profile like `KIT_COPY_WINDOW` today, at most 10 minutes, wiped at lock, exit,
profile switch and when the flow ends). Only the derived public keys, the certificate and (inside a
pending key change) the `kit_token` and envelope ever reach the database. Consequence, and the point of
D1: no device, and so no removed device and no profile backup, can open the envelope or show the Kit.

### 2.3 The Kit in the trust store

- **Kind 5, "kit"** (new): subject `kit_id` with `kit_sign_pub` and `kit_box_pub`; issuer = the device
  that created it (`issuer_sig` with that device's signing key, no ticket); `subject_sig` with
  `kit_sign_seed` (the creator holds the seed while it creates the Kit). Pinned when its issuer vouches
  (`trust::vouch`, unchanged). **At most one Kit is `trusted`**: a second Kit certificate is pinned only
  in the same pass or bundle that removes the first; otherwise it is refused (`ConflictKind::SecondKit`).
- **Kind 3, "joined with the Kit"** (re-meant): issuer = `kit_id`, `issuer_sign_pub = kit_sign_pub`,
  `issuer_sig` by `kit_sign_seed`; no ticket. Pinned when the Kit vouches (pinned, the same key,
  `speaks_at` the certificate's time). The byte layout is unchanged; `verify_signatures` refuses a kind 3
  whose issuer is its subject. Domain string stays `moony device cert v1` (layout unchanged, no released
  data, migration 041 bumps the schema so an older build stops at this build's envelopes); the kind-3
  vectors are regenerated and kind-5 vectors added.
- **The Kit's row** is a `sync_devices` row: `id = kit_id`, `public_key = kit_box_pub`,
  `platform = 'recoveryKit'`, `name = ''`, `joined_at` = creation time, `cert` = kind 5, and the new
  synced column `kit_storage` (§2.7). It is written by the creator (captured) and never listed as a
  device (`devices::list`, `impact`, counts and "last synced" skip `platform = 'recoveryKit'`).
- **Replacing the Kit** ("Vytvořit novou sadu") signs a `removed` record for the old `kit_id`, written
  into its row, and pins the new one; both travel in the key-change bundle (§2.5).

### 2.4 The envelope (the Kit's copy of every key)

```
header = "MNYE" ‖ 0x01 ‖ vault_id (16) ‖ kit_id (16) ‖ epoch (u32, the newest key it holds) ‖ eph_pub (32)
key    = HKDF-SHA256(salt = vault_id, ikm = X25519(eph_secret, kit_box_pub),
                     info = "moony kit envelope v1" ‖ eph_pub ‖ kit_box_pub ‖ epoch)
body   = nonce (24) ‖ XChaCha20-Poly1305(key, AAD = header, plain)
plain  = sig (64) ‖ JSON {"v":1, "by":"<hex>", "at":<unix s>, "signer":"<cert b64>",
                          "keys":[{"epoch":n, "key":"<b64url>", "head":h?}]}   — ascending, last = header epoch
sig    = Ed25519(signer, "moony kit envelope v1" ‖ header ‖ JSON bytes)
```

Built in `rekey/envelope.rs` like the bundle's entries (`bundle::os_fill`, wiped buffers, at most
256 KiB, vectors from fixed seeds). `keys` carries every epoch the sealer holds; `head` only for
**verified** heads (As built 63); the newest epoch never has one, because the envelope is sealed before the
server answers the rotation's head (§2.6 says how the joiner gets it).

**Who seals it, when:** whoever makes a key change (the remover), every time, in the same
`POST /token` call (§2.5). The server never holds an envelope for an older key than the current token's.

### 2.5 Every key change seals to the Kit

The Kit is a recipient of every key change, but not through an entry in the `MNYK` bundle: bundles are
fetched with device tokens and expire after 30 days, which is what the Kit must survive. Instead:

- The bundle (version byte **0x02**, JSON `v: 2`, vectors regenerated) gains
  `"kit": {"device":"<kit id>", "cert":"<kind-5 cert b64>", "replaces":{"device":"<old kit id>","record":"<removal record b64>"}?}`,
  present whenever the vault has a Kit. Receivers pin the Kit certificate (endorsed by the signer, like
  recipients), mark it `confirmed`, and apply `replaces.record` with the bundle's other removals. So a
  device that later makes a key change always seals to the right Kit: it must be at the vault's epoch to
  rotate (As built 55), so it has adopted the bundle that named the Kit.
- The relay call carries the envelope: `POST /v1/vaults/:id/token {token, rekey, kit: {envelope, token?}}`,
  `token` only when the Kit is replaced. The relay stores token, bundle and envelope in one transaction.
- The pending change (As built 57) gains `"kit": {"envelope":"<b64url>", "token":"<43>"?, "cert":"<b64>"?,
  "replaces":"<record b64>"?}`; `resolve_pending` re-sends it unchanged. The Kit seed is not in it.
- `trust::recipients` returns the Kit separately (`Recipients.kit: Option<TrustEntry>`); it never takes a
  bundle entry and does not count toward the 64.

### 2.6 Joining with the Kit after any number of key changes

`join::recovery_credentials` is rewritten (it refused any `rotation != 0`):

1. Decode the code (v2), derive `kit_token`, `kit_box_secret`, `kit_sign_seed`, `kit_id`.
2. `account_status(account)` as today (`syncRecoveryAccountInvalid`).
3. `GET /v1/vaults/:id/kit` with `Authorization: Bearer <kit_token>`:
   `401` / `404 vault_not_found` → `validation.syncRecoveryVaultNotFound`;
   `410 kit_replaced {replacedAt}` → `validation.syncRecoveryKitReplaced|{"at":<unix s>}` (replaces
   `syncRecoveryKitRotated`).
4. Open the envelope: header `vault_id` and `kit_id` must be the derived ones; the signature must verify
   with the bundled signer certificate (self-consistent; nothing pinned yet); keys ascending, 32 bytes,
   the last one = the header epoch. Any failure → `validation.syncRecoveryKitUnreadable` (new).
5. Credentials = all keys (`VaultKeys::from_parts`); `vault_info` with the derived token. Then the join
   as today (snapshot, replace, pull), with `JoinIdentity::RecoveryKit(KitJoin {kit_sign_seed, kit_id,
   heads})`: the envelope's heads go into `sync_epochs`; the newest epoch's head is taken provisionally
   from `GET /rekey` (`head::note_unverified`, verified by the remover's signed head record like on any
   device) or from the restored snapshot's `Keys` record when it names it (verified there). When neither
   has it (no bundle younger than 30 days and no snapshot under the newest epoch; `snapshot_epoch_due`
   makes that rare), the epoch stays unchecked as As built 45 allows, logged once.
6. **Kit anchor:** the restored rows must contain the Kit's row with a kind-5 certificate whose
   `sign_pub` is the derived one and no removal record that verifies; otherwise
   `syncRecoveryKitUnreadable`, nothing replaced (inside the replace's savepoint). The joiner pins it,
   signs its own kind-3 certificate with `kit_sign_seed` (`Issuer::Kit`), then `tofu_from_rows` as today.
7. The joiner writes the Local notice `sync.after_kit_join = {"devices": [ids]}`: every other active
   device (§6.5 "Data jsou zpět").

**What a Kit joiner trusts:** the Kit (derived from the paper) and, on first use, every certificate in
the restored data that verifies (phase-5 §5.4 TOFU, unchanged). It does not trust the envelope's signer
beyond the signature binding the envelope's bytes.

**The review's note on phase-5 §8.3 step 3** (an old Kit marked valid while an old snapshot is still on
the server): resolved by construction. Nothing is probed against snapshots; the server answers a
replaced Kit's token with `410`, and the envelope always holds the current key, so a snapshot of any
epoch opens.

### 2.7 When the Kit is replaced (and the `kit_storage` column)

The Kit is replaced only (a) when the person asks ("Vytvořit novou sadu…", lost or seen by someone), or
(b) when it may be on a device being removed. (b) needs to know where it is kept, so migration 041 adds
the synced, last-writer-wins column `sync_devices.kit_storage` on the Kit's row, written by the device
that shows the Kit (the creator; the Kit cannot be shown anywhere else):

| Value | Set by | Replaced when removing… |
|---|---|---|
| `paper` | "Vytisknout" (`sync_print_recovery_kit`) | never |
| `copied` | "Zkopírovat kód" (`sync_copy_recovery_code`) | the creator |
| `file` | "Uložit jako soubor…" to a local folder | the creator |
| `cloud` | the same, when `kit_file::looks_cloud_synced` says the folder syncs | **any** device (a stolen phone signed into the same cloud reads it) |
| NULL | nothing done (the flow was closed) | — (the Kit row says "neuložená", §6.3) |

The strongest value wins (`cloud` > `file` > `copied` > `paper`). During setup the value sits in the draft
and is written with the Kit's row at `finish_setup`. A screenshot or a photo of the screen is not
detected (residual R3).

**Replacing = one key change** that removes the old Kit and adds a new one (`replaces_kit` in the plan,
§4.2), combined with the removal when (b) applies. Order: the key change commits first, the new Kit is
shown after (so a closed dialog leaves "neuložená", never a printed page that never became valid). A
change left pending (server unreachable) shows the Kit at once with "Sada začne platit, až se odebrání
dokončí."; one that then fails writes `sync.removal_not_done` with `kitReplaced: true` ("Nová záchranná
sada nezačala platit; vytvořte si ji znovu").

Devices that joined with the old Kit keep syncing: at plan time the old Kit is still `trusted`, so they
are ordinary recipients and the bundle names them (§3.1). The new-Kit dialog lists them with
"Odebrat také" ticks (unticked), as removal lists devices a removed device added.

### 2.8 "Ověřit sadu" and seeing the Kit used

- **"Ověřit sadu"** (`sync_check_recovery_kit(code)`, no password, no network): decode; `vault_id` not
  this vault's → `otherSync`; derive `kit_sign_pub` and `kit_id`; the pinned kind-5 entry with that ID and
  key is `trusted` → `valid`; `removed` → `replaced {at, currentCreatedAt}`; none → `unknown`. Secrets are
  wiped before it answers. The sync server sees nothing.
- **Kit reads are visible**: the server records the minute of the last successful `GET /kit` and reports
  it in vault info (`kit.lastRead`). The Kit row shows "Naposledy použita {date}". When `lastRead` is
  newer than the Local `sync.kit_read_seen`, more than an hour old, and no kind-3 certificate issued since
  `lastRead − 1 h` is pinned, the status carries `kitUsed` (§6.3). A dishonest server can hide a read;
  an honest one cannot be fooled by the reader.
- **Kit joins are visible**: a newly pinned kind-3 entry gives every other device the strong notice of §6.3.

### 2.9 The relay change (Moony-relay)

Schema migration 7 (append to `MIGRATIONS` in `src/db.ts`):

```sql
CREATE TABLE kits (
  vault_id TEXT PRIMARY KEY REFERENCES vaults (vault_id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL,      -- SHA-256 of the Kit token
  envelope BLOB NOT NULL,        -- opaque, 1 B .. 256 KiB
  since INTEGER NOT NULL,        -- when this Kit token was set
  updated_at INTEGER NOT NULL,   -- last envelope replacement
  last_read INTEGER              -- minute of the last GET /kit with this token
);
CREATE TABLE kits_retired (
  vault_id TEXT NOT NULL REFERENCES vaults (vault_id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL,
  retired_at INTEGER NOT NULL,
  PRIMARY KEY (vault_id, token_hash)
);
```

| Method | Path | Rules |
|---|---|---|
| `PUT` | `/v1/vaults/:id/kit` | Current vault token (access `owner`). Body `{"token":"<43>","envelope":"<base64>"}`. No Kit yet → `201`; the same token hash → envelope replaced, `200` (a retried setup); another token → `409 kit_exists`. `400 invalid_kit_token` (not 43 base64url, or equal to a current/retired vault or Kit token hash), `400 invalid_kit`, `413 kit_too_large`. Judged before the body is read, like `/token`. |
| `GET` | `/v1/vaults/:id/kit` | `Authorization: Bearer <kit token>`, checked **only** against `kits.token_hash`, never against vault tokens (and a vault token never reads here). → `200 {"envelope","since","updatedAt"}`, sets `last_read` (minute). A retired Kit token → `410 {"error":"kit_replaced","replacedAt":t}`. Otherwise `401 unauthorized`; `404 vault_not_found`. Entitlement: the read rule (30 days after `paidUntil`). Does not touch `last_activity`. |
| `POST` | `/v1/vaults/:id/token` | Gains `"kit": {"envelope", "token"?}`. A vault with a Kit and no `kit` → `400 kit_required`. With `token`: the old hash moves to `kits_retired`, `since` = now (a new Kit; also how a vault without a Kit gets one by rotation). Envelope, token, bundle and the vault token change in **one** transaction. `MAX_ROTATE_BODY_BYTES` grows to hold two 256 KiB payloads in base64. |
| `GET` | `/v1/vaults/:id` | Gains `"kit": {"since","lastRead"} | null`. |
| `GET` | `/v1/server` | `features: ["blobs","kit"]` (protocol stays 2). The app refuses setup on a server without `kit` (`validation.syncServerNeedsUpdate`). |

Limits: a new limiter `kitRead` (10 per minute per client key) and `kitVault` (30 per hour per vault)
before the token check of `GET /kit`; the usual `vaultIp` applies. Retired Kit hashes: the newest 32 per
vault are kept (housekeeping). Deleting a vault or a person cascades. Backup: in the database copy already.
DESIGN.md: the routes, the storage line, Limits ("Kit envelope 256 KiB", "Kit reads"), and *What the
server knows*: "per vault, the SHA-256 of the Kit token and of replaced Kit tokens, the Kit envelope
(ciphertext it cannot open), when the Kit was set, last replaced and last read".

**Admin:** `GET /admin/api/people/:id` vaults gain `kitSince`, `kitLastRead`; Person detail shows per
profile "Záchranná sada: vytvořena {date} · naposledy použita {date | nikdy}" / "Recovery kit: created
{date} · last used {date | never}" (cs and en strings in `admin-ui/src/i18n`). While there, the review's
wording point: the admin's "profil" gets the subtitle "synchronizovaná data jednoho profilu Moony".

### 2.10 Threats

| Attacker | Can | Cannot | Detected by |
|---|---|---|---|
| **Stolen or copied Kit** (with the server address) | Read everything, past and future, until "Vytvořit novou sadu"; join as a device | Survive "Vytvořit novou sadu": the new key and the new envelope are sealed to keys it lacks, its token gets `410` | `kitUsed` (read without a join), the Kit-join notice on every device |
| **Stolen Kit + dishonest server** | The above, and the server can hide the read | Survive a new Kit: the cut-off is cryptographic, not the server's | The Kit-join notice only |
| **Server alone** | Deny, delay, serve an older envelope (older keys only: the joiner then cannot read current data or gets `410`/`401` paths) | Read anything: it never sees a key, `kit_box_pub` or the seed | Joins fail visibly |
| **Removed device** (with its old keys and signing key) | Nothing new: it has no Kit token (the seed was never stored) unless it kept the Kit file or code, which is why §2.7 replaces the Kit then | Read the envelope; write anything (its token is read-only, then unknown) | — |
| **Removed device + dishonest server** | Forge an envelope for the public `kit_box_pub` and serve a stale fork to someone joining with the Kit, then read what that person writes (R1). The fork-slot residual of As built 65(b) also stands | Read data written by devices that stayed | "Data jsou zpět" names the newest change's date; nothing more |
| **Device that left, from a profile backup with its signing key** | Until the next key change: everything (O2); also add a backdated device that survives later key changes (R2) | After a key change without such a device: nothing | New-device notice (§3.3) |

**Rotation head interaction:** the envelope carries only verified heads, never the newest (unknown when
sealed); the joiner verifies the newest through the signed head record like any device (§2.6 step 5).
The signed head, the provisional heads and pickup-before-pull of the running fix round are unchanged.

## 3. Automatic confirmation (D3)

### 3.1 The rule

`trust::recipients(conn, removing) -> Recipients {included, excluded, kit}` includes every pinned
`trusted` device entry, this device included, **except**:

| Reason | When |
|---|---|
| `Removing` | the person removes it now |
| `Left`, `Removed` | its state (as today) |
| `SupersededQuiet` | O11, as today |
| `IssuerRemoved` (new) | its issuer (paired, re-join or Kit-joined) is `removed` here **and** it is not `confirmed` |
| `Unverified`, `NoKey` | a row without a pinned certificate (as today) |

`confirmed` (the migration-040 column) now means "named as a recipient by a verified key change": set by
the remover's own commit for every recipient and the Kit, and by `import_bundle` for the signer, every
recipient and the Kit. No person, no tick, no record. `KitUnconfirmed` and `IssuerGone` are gone; an
issuer that **left** vouches for what it issued before it left (`vouch`, As built 27, unchanged) and
that device is an ordinary recipient.

### 3.2 Why it is safe

A certificate becomes a recipient on device D only if its signatures verify, its issuer is pinned on D
with the key the certificate names, and that issuer `speaks_at` the certificate's (and its ticket's)
time. Producing such a certificate after the fact needs the issuer's signing key **and** a way to put a
row into the vault. Case by case:

- **Issuer trusted** (it, or its unlocked thief, is a member): it can add devices anyway (O9, phase-5
  §12). Nothing changes.
- **Issuer removed:** D2 makes every removal a key change, so the issuer's key-holder lost the current
  vault key at that moment and cannot write rows. Its only path is the fork residual of As built 65(b): a
  dishonest server serving a lagging device old-epoch ops in pre-rotation slots. Such a certificate was
  not named by the bundle that cut its issuer off, so it is `IssuerRemoved` and gets nothing. A genuine
  device the remover did know was a plain recipient (its issuer was still `trusted` at plan time) and
  is `confirmed` everywhere through the bundle. A genuine device the remover did not know yet (paired a
  minute before) never got the new key in either design; it shows "Připojit znovu".
- **Issuer left:** the honest device deleted its signing key when it left. Its key survives only in a
  copy made earlier (a profile backup made while sync was on, phase-5 §12), and that copy also holds the
  current vault key until the next key change, so it can write a backdated certificate (R2). The As
  built 34 tick guarded this only when the person unticked exactly the forged row and kept every genuine
  one; in the common case (a new phone paired from an old one that was then turned off) it cost the
  genuine phone. The same recognition is available without the cost: §3.3.
- **Issuer = the Kit:** a Kit holder can add devices by design (D1); visible, cut off by a new Kit,
  after which its later joiners are `IssuerRemoved` unless the new-Kit bundle named them.

### 3.3 What the person still sees

- Each device's details: "Přidáno {date} ze zařízení {name}" / "Připojeno záchrannou sadou {date}".
- **A new-device notice on every other device** when an entry is pinned outside a join
  (`trust::refresh` appends to the Local `sync.new_devices`, at most 20, kinds 2 and 3; re-joins say
  nothing): "Přibylo zařízení {name}, přidané ze zařízení {by} {date}." When the issuer had already
  left or been removed on this device before the entry arrived, the stronger text with two buttons
  (§6.3). Kit joins always get the strong notice.
- The removal dialog's "Synchronizovat dál budou" list (§4.3): the people the new key goes to.

## 4. Always-rotate removal (D2)

### 4.1 What happens to which row

| Row | In the list | Removal | At the next key change |
|---|---|---|---|
| Active device | active | "Odebrat zařízení" → key change | — |
| Left (signed record) | "Dřívější zařízení", no action | — | gets a notice entry and is cut off (`cut_off_all_pending`, unchanged) |
| Superseded, quiet | hidden (the successor's details say "Znovu připojeno {date}") | — | left out (O11) |
| Superseded, still writing | "{name} (kopie)", Zkontrolujte | "Odebrat" → key change | — |
| `IssuerRemoved`, unverified | Zkontrolujte | "Odebrat" → key change | left out |
| Removed | "Dřívější zařízení", no action | — | — |

"Dřívější zařízení" rows are listed for 30 days after their state time (`devices::list` leaves older
ones out); the rows and the trust entries stay in the database.

### 4.2 The API

- `RemovalMode` and the list-only path are deleted (`rotate.rs`, its tests; `sync_removal_plan` takes no
  mode). Removal records naming this device that arrive through sync are still handled (Task 19): a
  commit writes them into rows, so they still sync.
- `rotate::plan(conn, removing, also_remove, new_kit) -> Plan {epoch, removing, recipients, excluded,
  added_by_removing, kit: Option<TrustEntry>, replaces_kit, consent}`. `replaces_kit` = `new_kit`, or the
  §2.7 table says so for a device in `removing`. `added_by_removing` includes devices joined with the
  Kit when `new_kit`.
- `consent` = hex SHA-256 over the domain string, the epoch, the removed IDs, each recipient's ID and
  certificate hash, the current Kit's certificate hash (or none) and `replaces_kit`.
- `remove(relay, db, Removal {removing, consent, new_kit}, data_dir, forget_pairing, now)` →
  `Outcome::{Changed {epoch, removed, kit: Option<NewKit>}, Pending {kit: Option<NewKit>},
  AlreadyRemoved {by}, Raced {by}, RemovedHere}`. `NewKit` holds the `KitSecret` (redacting, wiped) for
  the UI to show once; the command turns it into a `RecoveryKit` and drops it.
- Preconditions stay (As built 54) minus the list-only one; `syncRemovalPlanChanged` keeps its meaning.

### 4.3 The dialog (one screen)

Title, what happens, who stays, devices the removed one added (ticks, unticked), the "in your hand"
hint, the Kit line when `replaces_kit`, the password, "Zrušit" · "Odebrat zařízení". Then "Odebírám
{name}…" in the dialog body, then a toast, or the Kit step when `replaces_kit`. Texts in §6.2. The
excluded list is shown only for rows that are **active or Zkontrolujte** and still drop out
("Synchronizovat se přestanou i:"), which under §3.1 is only `IssuerRemoved` and unverified rows.

### 4.4 The 32-per-30-days cap and the 64-device cap

`409 too_many_rotations` → `validation.syncTooManyKeyChanges`, reworded: "Za posledních 30 dní se seznam
zařízení změnil příliš mnohokrát. Zkuste to znovu za několik dní." / "The device list changed too many
times in the last 30 days. Try again in a few days." No counter is shown in advance: 32 removals or new
Kits in 30 days is far above human use, and the server stays the judge. `syncTooManyDevices` (64): "Moony
synchronizuje nejvýš 64 zařízení. Nejdřív odeberte ta, která už nepoužíváte." / "Moony syncs at most 64
devices. First remove the ones you no longer use."

## 5. Pairing code on the desktop (D4)

- The desktop join choice (`JoinChoice.tsx`) gets three options in this order: "Mám telefon se
  synchronizací" (today's: this computer shows a QR, the phone scans), **"Mám jiný počítač se
  synchronizací"** (new: a code field, `TypedCodeField`, then `deviceSyncApi.pairingScan(code, {password,
  deviceName})`; this computer then shows the six digits and the holder types them, exactly the phone
  joiner's path), and "Nemám u sebe žádné zařízení" (the Kit).
- The desktop holder's `AddDeviceDialog` gets "Zkopírovat kód" under the text code (the six digits typed
  on the holder protect the exchange, ADR 0026; a code sent through a messenger is safe for that reason).
- **The holder finishes visibly** (desktop and phone): after `sent`, the dialog watches the device list
  (the fast poll of `fix/sync-live-test`) for a device row that was not there when pairing started and
  ends with "{name} je připojené" / "{name} is connected" · "Hotovo". After 2 minutes without it:
  "Dokončete připojení na druhém zařízení. Až se připojí, objeví se v seznamu." Frontend only.
- ADR 0026's "typed code is a developer tool" note is amended: release builds accept a typed code on
  every platform.

## 6. What the person sees

Rule for every state: what happened (one sentence), that the data on this device are fine (where it is
in doubt), and one button; where there is none, say that Moony keeps trying.

### 6.1 The device list

| Place | Czech | English |
|---|---|---|
| Card description | Tato zařízení mají přístup k vašim datům. | These devices can access your data. |
| Active row line | aktivní {when} | active {when} |
| Inactive badge, hint | Neaktivní {n} dní · Už ho nepoužíváte? Odeberte ho. (plurals) | Inactive for {n} days · Not using it any more? Remove it. |
| Pending removal | Odebírá se… | Removing… |
| Details: created / paired / Kit / re-join | První zařízení · Přidáno {date} ze zařízení {name} · Připojeno záchrannou sadou {date} · Znovu připojeno {date} | First device · Added on {date} from {name} · Joined with the recovery kit on {date} · Reconnected on {date} |
| "Zkontrolujte" badge | Zkontrolujte | Check this |
| … unverified | Tohle zařízení se nepodařilo ověřit. Až příště odeberete nějaké zařízení, přestane se synchronizovat. Pokud je vaše, připojte ho znovu: na něm vypněte synchronizaci a zvolte Připojit. | This device couldn't be verified. The next time you remove a device, it will stop syncing. If it's yours, connect it again: turn sync off on it and choose Connect. |
| … copy (name "{name} (kopie)") | Toto zařízení se znovu připojilo, ale jeho dřívější kopie pořád zapisuje změny. Pokud ji nepoznáváte, odeberte ji. | This device connected again, but its earlier copy still writes changes. If you don't recognise it, remove it. |
| … issuer removed | Přidalo ho zařízení, které jste odebrali, a nová data už nedostává. Pokud je vaše, připojte ho znovu. | It was added by a device you removed and no longer gets new data. If it's yours, connect it again. |
| Folded group | Dřívější zařízení ({n}) | Former devices ({n}) |
| Former row | odpojeno {date} · odebráno {date} zařízením {name} | left on {date} · removed on {date} by {name} |
| Under the list | Ztracené, prodané nebo vymazané zařízení odeberte. Nic nového už neuvidí. | Remove a lost, sold or wiped device. It won't see anything new. |
| Pre-phase-5 vault (O3) | Tuto synchronizaci nastavila starší verze Moony, proto v ní nejde odebírat zařízení. Smažte synchronizovaná data, nastavte synchronizaci znovu a připojte ostatní zařízení. | An older version of Moony set up this sync, so devices can't be removed from it. Delete the synced data, set sync up again and connect your other devices. |

Desktop: a row expands for its details and the "Odebrat…" button; phone: other devices' rows open the
device sheet (details as label-value rows, "Odebrat zařízení" as a danger text button). The
"Ztratili jste zařízení…" collapsible goes.

### 6.2 Removal and the new Kit

| Place | Czech | English |
|---|---|---|
| Title | Odebrat zařízení {name}? | Remove {name}? |
| Body | {name} přestane dostávat nová data a nebude moct nic měnit. Co už v něm je, tam zůstane; chrání to heslo do Moony. | {name} will stop getting new data and won't be able to change anything. What's already on it stays there, protected by its Moony password. |
| Who stays | Synchronizovat dál budou: · {name} (toto zařízení) | These will keep syncing: · {name} (this device) |
| Who drops out (only if any) | Synchronizovat se přestanou i: | These will also stop syncing: |
| Added by the removed one | Tato zařízení jste přidali ze zařízení {name}. Odeberte i ta, která nepoznáváte. ☐ {device} · přidáno {date} | You added these from {name}. Remove any you don't recognise too. ☐ {device} · added {date} |
| In hand | Máte {name} u sebe? Stačí na něm synchronizaci vypnout. | Have {name} with you? Just turn sync off on it. |
| Kit line (`replaces_kit`) | Na zařízení {name} jste si uložili záchrannou sadu, proto přestane platit. Hned potom si vytvoříte novou. | Your recovery kit was saved on {name}, so it will stop working. You'll make a new one right after. |
| Kit line (`cloud`) | Záchrannou sadu máte ve složce, která se synchronizuje s cloudem, proto přestane platit. Hned potom si vytvoříte novou. | Your recovery kit is in a folder that syncs to the cloud, so it will stop working. You'll make a new one right after. |
| Field, buttons | Heslo do Moony · Zrušit · Odebrat zařízení | Moony password · Cancel · Remove device |
| Progress / done toast | Odebírám {name}… / Zařízení {name} je odebrané | Removing {name}… / {name} removed |
| Server unreachable | Synchronizační server teď není dostupný. Odebrání se dokončí samo, jakmile bude. | The sync server isn't reachable right now. The removal will finish by itself once it is. |
| Race, already removed | {name} mezitím odebralo zařízení {by}. | {by} removed {name} in the meantime. |
| Race, other change (`syncRotationRaced`) | Seznam zařízení mezitím změnilo zařízení {name}. Zkontrolujte ho a zkuste to znovu. | {name} changed the device list in the meantime. Check it and try again. |
| Plan changed | Seznam zařízení se mezitím změnil. Zkontrolujte ho a potvrďte znovu. | The device list changed in the meantime. Check it and confirm again. |
| Kit step title / body | Vaše nová záchranná sada · Stará sada přestala platit. Vytiskněte si tuto a uložte ji mimo toto zařízení. Ukážeme ji jen teď. | Your new recovery kit · The old kit no longer works. Print this one and keep it away from this device. We show it only now. |
| Kit step, pending | Sada začne platit, až se odebrání dokončí. | The kit starts working once the removal finishes. |
| Kit step buttons | Vytisknout · Uložit jako soubor… · Zkopírovat kód → Sadu mám, hotovo (phone: Uložit nebo sdílet… · Zkopírovat kód) | Print · Save as file… · Copy code → I have the kit, done (phone: Save or share… · Copy code) |
| New Kit dialog | Vytvořit novou záchrannou sadu? · Stará sada přestane platit. Udělejte to, když jste ji ztratili nebo si nejste jistí, kdo ji viděl. · Heslo do Moony · Vytvořit novou sadu | Make a new recovery kit? · The old kit will stop working. Do this if you lost it or aren't sure who has seen it. · Moony password · Make a new kit |
| New Kit, Kit-joined devices | Tato zařízení se připojila starou sadou. Odeberte ta, která nepoznáváte. ☐ … | These devices joined with the old kit. Remove any you don't recognise. ☐ … |

### 6.3 The Kit in Settings and setup

| Place | Czech | English |
|---|---|---|
| Kit row | **Záchranná sada** · Vytvořena {date} na zařízení {name}. Platí, dokud si nevytvoříte novou. · Ověřit sadu · Vytvořit novou… | **Recovery kit** · Created on {date} on {name}. Valid until you make a new one. · Check kit · Make a new one… |
| Kit row, read | Naposledy použita {date} | Last used {date} |
| Kit row, unsaved (`storage` NULL) | Záchrannou sadu jste si neuložili a znovu ji ukázat nejde. · Vytvořit novou sadu | You didn't save your recovery kit, and it can't be shown again. · Make a new kit |
| No Kit (`recoveryKit` null) | Synchronizace nemá záchrannou sadu. Bez ní data nedostanete zpět, když přijdete o všechna zařízení. · Vytvořit sadu | Sync has no recovery kit. Without one you can't get your data back if you lose every device. · Make a kit |
| Check dialog | Ověřit záchrannou sadu · Naskenujte nebo opište kód ze sady. Nic se nikam neodesílá. | Check your recovery kit · Scan or type the code from the kit. Nothing is sent anywhere. |
| Check results | Tato sada platí. · Tato sada už neplatí. Platná je ta z {date}. · Tato sada patří k jiné synchronizaci. · Tuto sadu Moony nezná; možná se nikdy nedokončila. | This kit is valid. · This kit no longer works. The valid one is from {date}. · This kit belongs to a different sync. · Moony doesn't know this kit; it may never have been finished. |
| `kitUsed` | Záchranná sada byla použita {date}, ale žádné zařízení se jí nepřipojilo. Pokud jste to nebyli vy, má ji někdo další: vytvořte si novou. · Vytvořit novou sadu · Vím o tom | Your recovery kit was used on {date}, but no device joined with it. If it wasn't you, someone else has it: make a new one. · Make a new kit · I know about it |
| Kit join (other devices) | **Zařízení se připojilo záchrannou sadou** · {name} se připojilo {date}. Pokud jste to nebyli vy, má vaši sadu někdo další. · To jsem já · Odebrat a vytvořit novou sadu | **A device joined with your recovery kit** · {name} joined on {date}. If it wasn't you, someone else has your kit. · That was me · Remove it and make a new kit |
| New device | Přibylo zařízení {name}, přidané ze zařízení {by} {date}. · Zavřít | {name} was added from {by} on {date}. · Close |
| New device, issuer already gone | Přibylo zařízení {name}. Přidalo ho zařízení {by} {date}, které už se nesynchronizuje. Pokud {name} nepoznáváte, odeberte ho. · Je moje · Odebrat | {name} was added. {by} added it on {date} and no longer syncs. If you don't recognise {name}, remove it. · It's mine · Remove |
| Setup, Kit step | **Záchranná sada** · Kdybyste přišli o všechna zařízení najednou, vrátí vám data jen tato sada. Vytiskněte ji a uložte mimo tento počítač, třeba k dokladům nebo do správce hesel. · Vytisknout · Uložit jako soubor… · Zkopírovat kód · Sadu ukážeme jen teď. Když ji ztratíte, vytvoříte si v Nastavení novou. · Není to obnovovací klíč tohoto zařízení; ten obnoví jen zapomenuté heslo. → Sadu mám, pokračovat | **Recovery kit** · If you lose every device at once, only this kit gets your data back. Print it and keep it away from this computer, with your documents or in a password manager. · Print · Save as file… · Copy code · We show the kit only now. If you lose it, make a new one in Settings. · It isn't this device's recovery key; that only resets a forgotten password. → I have the kit, continue |
| Setup, done screen | **Synchronizace je zapnutá** · Nahrávám první kopii dat. Můžete pracovat dál. · Přidat druhé zařízení · Později | **Sync is on** · Uploading the first copy of your data. You can keep working. · Add another device · Later |
| Server address change | Novou adresu si připište na záchrannou sadu. | Write the new address on your recovery kit. |

The setup's "Uložil(a) jsem si…" tick goes: after print, save or copy the primary becomes "Sadu mám,
pokračovat". "Vytisknout" (desktop) is `sync_print_recovery_kit`, which calls the window's
`print()` (Tauri `WebviewWindow::print`) while a print stylesheet shows only the Kit sheet; nothing is
written to disk by Moony. If the live check shows it does not print in the built app, "Uložit jako
soubor…" becomes the primary and the button is hidden (the decision is recorded in As built).

### 6.4 Status, other devices, the removed device

| Field (`SyncStatus`) | Czech | English | Button |
|---|---|---|---|
| `removalPending` (was `rotationPending`), phase syncing | Dokončuji odebrání zařízení… | Finishing removing a device… | — |
| problem `keyRotated` | Synchronizuji… (the ordinary text) | Syncing… | — |
| `syncingSince` > 60 s, not `firstUpload` | Synchronizace trvá déle než obvykle. | Sync is taking longer than usual. | Zkusit znovu (`sync_now`) |
| `firstUpload` | Nahrávám první kopii dat. Můžete pracovat dál. | Uploading the first copy of your data. You can keep working. | — |
| `removalNotice` (Local `sync.key_change_notice`) | Zařízení {by} odebralo {names} ({date}). / Zařízení {by} vytvořilo novou záchrannou sadu ({date}); stará už neplatí. | {by} removed {names} ({date}). / {by} made a new recovery kit ({date}); the old one no longer works. | Zavřít |
| `removalNotDone` | Odebrání zařízení {devices} se nedokončilo: seznam zařízení mezitím změnilo zařízení {by}. Zkontrolujte ho a zkuste to znovu. / {devices} mezitím odebralo zařízení {by}. / (by null) Synchronizační server odebrání odmítl. Zkuste to znovu. / (kit) Nová záchranná sada nezačala platit; vytvořte si ji znovu. | Removing {devices} didn't finish: {by} changed the device list in the meantime. Check it and try again. / {by} removed {devices} in the meantime. / The sync server refused the removal. Try again. / The new recovery kit didn't start working; make it again. | Zavřít |
| problems `keyNotGiven`, `keyChangeUnverified`, `unauthorized` (one UI state) | **Toto zařízení je potřeba připojit znovu** · Bylo dlouho bez spojení, nebo se mezitím změnilo, kdo má k datům přístup. Data v tomto zařízení jsou v pořádku, jen se nesynchronizují. (+ `rejoinBy`: Změnu provedlo zařízení {by}, které toto zařízení nezná.) | **This device needs to connect again** · It was offline for a long time, or who can access the data changed meanwhile. The data on this device are fine; they just don't sync. (+ {by}, which this device doesn't know, made the change.) | Připojit znovu |
| `keyChangeOverrun` | Synchronizační server poslal tomuto zařízení změny, které po odebrání zařízení {date} poslat neměl, a Moony je použila. Pokud jste zařízení odebrali kvůli krádeži, zkontrolujte změny z posledních dní. | The sync server sent this device changes it shouldn't have after a device was removed on {date}, and Moony applied them. If you removed that device because it was stolen, check the changes of the last few days. | Zavřít |
| `removedNotice` (readable with sync off) | **Toto zařízení bylo odebráno ze synchronizace** · Odebralo ho zařízení {by} {date}. Vaše data tady zůstala, jen se už nesynchronizují. (+ {n} změn z tohoto zařízení se do synchronizace nedostalo; zůstávají jen tady. — plurals) | **This device was removed from sync** · {by} removed it on {date}. Your data stayed here; they just don't sync any more. (+ {n} changes from this device didn't make it into sync; they stay only here.) | Rozumím · Připojit znovu |
| `afterKitJoin` (the Kit joiner only) | **Data jsou zpět** · Poslední změna je z {date}. V seznamu zůstala vaše dřívější zařízení: {names}. Pokud jste o ně přišli, odeberte je, ať nic nového neuvidí. | **Your data are back** · The newest change is from {date}. Your earlier devices are still on the list: {names}. If you lost them, remove them so they see nothing new. | Odebrat je · Mám je |

"Připojit znovu" = password → turn sync off (local data stay) → the join choice; before replacing, the
replace notice adds "Změny, které odsud neodešly, zůstanou v záloze." (the `-kept` backup). The removed
notice shows in Settings → Sync, **once at unlock** (desktop dialog, phone sheet) and as a line in the
top-bar "Stav dat" popover ("Synchronizace je vypnutá: toto zařízení bylo odebráno.") until "Rozumím".
`newDevices`, `kitUsed` and `removalNotice` add one line each to the popover. Other phase-5 texts and
the three worst of today per the review §4.9 table ("Čeká se na změnu z jiného zařízení", `captureBroken`,
`invalidChain`) are reworded in the same task. The turn-off dialog: review §4.6 texts; `turnOff.noCutOff`
goes; "Synchronizovat toto zařízení znovu" moves under the problems card.

Glossary (`docs/standards/i18n-glossary.md`): add odebrat zařízení / remove device, dřívější zařízení /
former devices, připojit znovu / connect again, zkontrolujte / check this, záchranná sada (lower case) /
recovery kit, ověřit sadu / check kit; **forbidden in UI**: klíč synchronizace, výměna klíče, odříznout,
synchronizační účet, číslo účtu (own server).

### 6.5 Wire (`shared/schema.ts`, the seven steps)

- `SyncDevice` gains `status: 'active' | 'check' | 'former'`, `checkReason: 'unverified' | 'copy' |
  'issuerRemoved' | null`, `formerReason: 'left' | 'removed' | null`, `joinedVia: 'created' | 'paired' |
  'recoveryKit' | 'rejoined' | 'unknown'`, `addedBy: string | null`, `stateAt: number | null`,
  `stateBy: string | null`, `removalPending: boolean`, `inactiveDays: number | null`.
- `SyncStatus` gains `removalSupported`, `removalPending`, `removalNotice`, `removalNotDone`,
  `keyChangeOverrun`, `removedNotice`, `rejoinBy`, `newDevices: {id, name, by, at, viaKit, issuerGone}[]`,
  `recoveryKit: {createdAt, createdBy, storage, lastReadAt} | null`, `kitUsed: number | null`,
  `afterKitJoin: {names: string[], newestChangeAt: number | null} | null`, `syncingSince: number | null`,
  `firstUpload: boolean`. `SyncProblem` gains `keyNotGiven`, `keyChangeUnverified`.
- `RecoveryKit`: `accountNumber` goes; gains `createdAt`, `createdBy`; `recoveryCode` is 19 groups.
- New: `RemovalPlan {removing, staying, droppingOut: {device, reason}[], addedByRemoving, replacesKit:
  'no' | 'creatorRemoved' | 'cloud' | 'asked', consent}`, `RemovalOutcome {kind: 'done' | 'pending' |
  'alreadyRemoved', by: string | null, kit: RecoveryKit | null}`, `KitCheck {result: 'valid' | 'replaced'
  | 'otherSync' | 'unknown', replacedAt, currentCreatedAt}`.
- Commands: `sync_removal_plan(deviceIds, alsoRemove, newKit)`, `sync_remove_devices(password, deviceIds,
  alsoRemove, newKit, consent)`, `sync_check_recovery_kit(code)`, `sync_print_recovery_kit()`,
  `sync_dismiss_notice(kind)` with kinds `removal | removed | removalNotDone | keyChangeOverrun |
  newDevices | kitUsed | afterKitJoin`. **Deleted:** `sync_show_recovery_kit` (and `setup::recovery_kit`).
  Changed: `sync_prepare_recovery_kit` makes the draft; `sync_finish_setup(deviceName)` loses `kitSaved`
  (refuses with `validation.syncRecoveryKitNotConfirmed` while the draft has no storage value).
- New validation keys: `syncRecoveryKitReplaced` (`{at}`), `syncRecoveryKitUnreadable`. Reworded:
  `syncRotationRaced`, `syncTooManyKeyChanges`, `syncTooManyDevices`, `syncRecoveryVaultNotFound`
  ("K této sadě server žádná data nemá. Zkontrolujte kód a adresu serveru." / "The server has no data for
  this kit. Check the code and the server address."). Never added: `syncRecoveryKitNeedsSnapshot`.

## 7. Impact on the plan

### 7.1 Code of Tasks 1–10 that changes (after the running fix round lands; nothing here touches its files before)

| File | Change |
|---|---|
| `identity/cert.rs`, `identity/tests.rs` | `CertKind::Kit = 5`; kind 3 issuer = the Kit; `issue_kit(creator, fields, kit_seed)`, `issue_kit_join(kit_seed, kit_id, fields, subject)`; `verify_signatures` per kind; kind-3 vectors regenerated, kind-5 added. |
| new `identity/kit.rs` | `KitSecret` (seed, redacting), the four derivations of §2.2, `kit_id`. |
| `identity/mod.rs` | `Issuer::RecoveryKit` → `Issuer::Kit {seed, kit}`; `write_once_value_valid` knows kind 5. |
| `identity/trust.rs`, `trust_tests.rs` | `judge_kit` → kind 5 via `vouch` + one trusted Kit (`ConflictKind::SecondKit`); kind 3 via `vouch`; `kit_unconfirmed` never set (column stays); `recipients(conn, removing)` returns `kit`, drops `opted_in`, `KitUnconfirmed`, `IssuerGone`, adds `IssuerRemoved`; `import_bundle` takes the bundle's `kit`; `confirm` deleted; `export` drops `kitUnconfirmed`; `tofu_from_rows` keeps; `refresh` writes `sync.new_devices`. Tests of As built 29/33/34 rewritten as §3 cases. |
| `pairing/holder.rs`, `pairing/tests.rs` | The export carries kind 5 entries; vectors of slot 2 v2 regenerated only if they include `kitUnconfirmed`. |
| `rekey/bundle.rs`, `bundle_tests.rs` | Version 0x02, JSON `v: 2`, `kit` field (`BundleInput.kit`, `BundleBody.kit`); vectors. |
| new `rekey/envelope.rs` (+ tests) | §2.4. |
| `relay/{mod,http,fake}.rs` (+ tests) | `rotate_token(vault, new_token, bundle, kit: Option<&KitUpdate>)`, `put_kit`, `get_kit(vault_id, kit_token)`, `VaultInfo.kit`, feature `kit`, errors `KitReplaced {at}`, `KitExists`; fake mirrors §2.9. |
| `rekey/rotate.rs`, `engine/tests/rekey_rotate.rs` | §4.2: no `RemovalMode`, `Plan`/`Removal`/`Outcome` as there; envelope built and sent; pending JSON gains `kit`; commit marks every recipient and the Kit `confirmed` (replaces `trust::confirm(ticked)`), writes the new Kit row and the old Kit's removal record; consent covers the Kit. |
| `rekey/pickup.rs`, `engine/tests/rekey_pickup.rs` | The signer check loses `kit_unconfirmed`; `import_bundle` with `kit`; Local `sync.key_change_unverified = {by}` on `Unverified`. |
| `rekey/head.rs` | None (the envelope reads `head::verified_epochs`). |
| `recovery.rs` | Code v2 (§2.2). |
| `join.rs` | `recovery_credentials` per §2.6; `JoinIdentity::RecoveryKit(KitJoin)`. |
| `setup.rs`, `commands/sync.rs`, `commands/sync_kit.rs`, `kit_file.rs` | Kit draft, registration at `finish_setup`, storage values, print, the deleted re-show; the Kit page header line. |
| `devices.rs`, `impact.rs` | Skip the Kit row; statuses of §6.5. |

### 7.2 Migration, schema, wire

- **Migration 041 `041_sync_recovery_kit`** (main ends at 039, this branch has 040, no other branch has
  04x): `ALTER TABLE sync_devices ADD COLUMN kit_storage TEXT;`. Registry: Synced, last-writer-wins, not
  write-once; schema number 41 (the barrier stays). `sync_trust.kit_unconfirmed` and
  `sync_state.kit_epoch_saved` stay unused (documented in `database.md`).
- Relay schema 7 (§2.9).
- Wire: §6.5, one mixed commit (Task 23).

### 7.3 Revised tasks (replace plan Tasks 11–24)

Global constraints of the plan stay. Relay tasks work in `/Users/filipkral/Programování/FinanceApp/Moony-relay`
on a branch `feat/recovery-kit` from its main (never push; the owner merges); gates `npm run lint &&
npm run typecheck && npm test` (and `npm --prefix admin-ui test` for Task 12).

**Task 11 [opus] Relay: Kit storage and routes (§2.9).** Files: `src/db.ts` (migration 7), new
`src/kits.ts`, `src/vaults.ts` (rotation with `kit`, info `kit`), `src/sync-api.ts` (routes), `src/limits.ts`,
`src/ratelimit.ts`, `src/housekeeping.ts`, `src/app.ts` (`FEATURES`), `DESIGN.md`; new `test/kit-api.test.ts`,
`test/vault-api.test.ts`. Tests first: setup `PUT` (201, idempotent 200, `409 kit_exists`); `GET` with the
Kit token (200, `last_read` minute), with a vault token (`401`), with a retired Kit token (`410` +
`replacedAt`); rotation without `kit` on a vault with a Kit (`400 kit_required`), with `kit.envelope`
(replaced atomically; a failed rotation leaves the old envelope), with `kit.token` (old token retired);
a Kit token equal to a vault token refused; limits (`kitRead`, `kitVault`, 256 KiB); cascade on vault and
person delete; info's `kit`; `features`. Done: all green, DESIGN.md updated.

**Task 12 [sonnet] Relay admin: the Kit per profile (§2.9).** Files: `src/admin/api.ts`,
`admin-ui/src/screens/PersonDetail.tsx`, `admin-ui/src/i18n/{cs,en}.ts`, `test/admin-people.test.ts`.
Done: `kitSince`, `kitLastRead` in the person view; the line shows "nikdy" / "never"; the profile subtitle.

**Task 13 [opus] Kit formats: code v2, derivations, kind 5 and kind 3, envelope (§2.2–§2.4).** Files:
`recovery.rs`, new `identity/kit.rs`, `identity/cert.rs`, `identity/mod.rs`, new `rekey/envelope.rs`, tests
and vectors. Tests first: code v2 round trip, typo and v1 refusal, QR v2; derivation vectors; kind 5 and
the new kind 3 sign/verify, flipped bytes; envelope vectors, a stranger cannot open, a changed header,
signature or key order fails, 256 KiB cap. Done: gates green, vectors committed.

**Task 14 [opus] Trust store revision and migration 041 (§2.3, §3).** Files: `db/migrations.rs`,
`db/schema.snapshot`, `registry/tables.rs`, `registry/keys.rs`, `merge/columns.rs`, `identity/trust.rs`,
`trust_tests.rs`, `docs/architecture/database.md`. Tests first: the Kit pinned via its creator, one
trusted Kit, `SecondKit`; kind 3 via the Kit, refused after the Kit's removal unless dated before;
`recipients`: a phone paired from a device that left is included (the As built 34 trap, both arrival
orders); a device whose issuer was removed is `IssuerRemoved` unless a bundle named it; the Kit returned
apart, never counted; `import_bundle` confirms recipients and the Kit and applies `replaces`; new-device
notices written for kinds 2 and 3 outside a join, not for this device or kind 4; migration 041 test.
Done: no code reads or sets `kit_unconfirmed`; `trust::confirm` gone.

**Task 15 [sonnet] Relay client and fake: the Kit (§2.9).** Files: `relay/{mod,http,fake}.rs`,
`relay/fake/tests.rs`, `relay/http/tests.rs`, every `impl Relay for`. Tests: the HTTP shapes and error
mapping; the fake's atomic rotation with the envelope, `kit_required`, `410` with `replacedAt`, info's
`kit`. Done: gates green.

**Task 16 [opus] Removal with the Kit, always rotating (§2.5, §2.7, §4).** Files: `rekey/rotate.rs`,
`rekey/bundle.rs`, `rekey/pickup.rs`, `engine/tests/rekey_rotate.rs`, `rekey_pickup.rs`, `rekey_fixture.rs`.
Tests first: the plan's `replaces_kit` per the §2.7 table; consent changes with the Kit; a removal sends
the envelope sealed to the current Kit (opened with its secret: every key, verified heads only); a new-Kit
change retires the old token on the fake, the old Kit reads `410`, the new one opens; pending keeps and
re-sends the Kit part; commit confirms all recipients and writes the Kit rows; a pickup pins the bundle's
Kit; list-only tests deleted. Done: gates green; the running fix round's tests still pass.

**Task 17 [sonnet] Setup and the Kit lifecycle (§2.2, §2.7, §2.8).** Files: `setup.rs`, `commands/sync.rs`,
`commands/sync_kit.rs`, `kit_file.rs`, `engine/status.rs`. Tests: the draft is never in the database
(scan `sync_state` and `sync_devices` after setup for the seed bytes); `finish_setup` writes the Kit row
and calls `put_kit` (idempotent on retry); storage values and their order; `check_kit` results; a server
without `kit` refuses setup; `kitUsed` rule (an hour's grace, explained by a Kit join). Done: gates green.

**Task 18 [opus] Joining with the Kit (§2.6).** Files: `join.rs`, `engine/tests/join_new.rs`. Tests first:
join after 0, 1 and 3 key changes; after 31 days (bundles gone; heads from the snapshot's `Keys` record
or unchecked); a replaced Kit (`410` → `syncRecoveryKitReplaced`); an unknown one (`401`); a tampered
envelope or a missing/removed Kit row (`syncRecoveryKitUnreadable`, nothing replaced); the joiner's
kind-3 certificate pins on another device; `sync.after_kit_join` written. Done: gates green.

**Task 19 [sonnet] Leaving and records about this device** = plan Task 11 without the list-only parts
(files and tests as written there).

**Task 20 [sonnet] Documents and photos across a key change** = plan Task 13, unchanged.

**Task 21 [opus] Multi-device scenarios** = plan Task 14, amended: no list-only cases; add the D3 trap
(O pairs N, O turns sync off, D offline throughout, then D removes X: N keeps syncing); a removed issuer's
late certificate stays out; a new Kit while a Kit-joined device is offline (it picks up and stays); a Kit
join after the property test's key changes. Property: also "the Kit opens the newest envelope with every
key".

**Task 22 [sonnet] Against a real sync server** = plan Task 15, plus the Kit routes and a Kit join after a
rotation, run against Task 11's relay branch (`npm run build` there first).

**Task 23 [sonnet] Commands, status and the wire (§6.5)** — the one mixed commit, as plan Task 16, with
§6.5's fields, commands and keys (all ten locales for the validation keys).

**Task 24 [sonnet] Hooks and helpers.** Files: `src/hooks/use-device-sync*.ts`, `src/utils/sync-devices.ts`,
`sync-removal.ts`, `sync-state.ts` (+ tests). Tests: grouping (active, check, former; former older than
30 days already absent), `checkReason` and `joinedVia` keys, `replacesKit` → Kit line key, the three
problems → one rejoin state, `syncingSince` → "longer than usual" after 60 s, popover lines.

**Task 25 [sonnet] Desktop device list and removal dialog (§4.3, §6.1, §6.2).** Files:
`SyncDevicesCard.tsx`, new `DeviceRow.tsx`, `RemoveDeviceDialog.tsx`, locales ×10.

**Task 26 [sonnet] Desktop Kit: setup step, done screen, Kit row, check, new Kit (§6.3).** Files:
`SyncSetupDialog.tsx`, `RecoveryKitPanel.tsx`, delete `ShowRecoveryKitFlow.tsx`, new `KitRow.tsx`,
`CheckKitDialog.tsx`, `NewKitDialog.tsx`, print stylesheet, locales ×10.

**Task 27 [sonnet] Desktop pairing-code entry and the holder's finish (§5).** Files: `join/JoinChoice.tsx`,
`join/PairingJoinFlow.tsx`, `AddDeviceDialog.tsx`, `src/mobile/components/sync/PairingSheet.tsx` (holder
finish), locales ×10.

**Task 28 [sonnet] Desktop notices and status (§6.4).** Files: `SyncSection.tsx`, `SyncProblemAlert.tsx`,
`SyncStatusCard.tsx`, `TurnOffSyncDialog.tsx`, `SyncDangerCard.tsx`, the top-bar `SyncStatusSection.tsx`,
an unlock-time removed dialog, new `NoticeList.tsx`, locales ×10.

**Task 29 [sonnet] Phone list, device sheet, removal sheet** = plan Task 21 with §6.1–§6.2.

**Task 30 [sonnet] Phone Kit and notices** = plan Task 22 plus the Kit row, check, new Kit and setup step
(`SyncKitSheet.tsx`, `KitBody.tsx`, `SetupSheet.tsx`), the removed sheet at unlock, locales ×10.

**Task 31 [sonnet] Docs.** ADR 0032 (phase 5 with this revision: D1–D4, §0, §2.10, residuals), ADR 0026
(typed codes everywhere, Kit v2), ADR 0027 status line, `decisions/README.md`, the phase-5 spec's As built
pointer to this document, `overview.md`, glossary (§6.4), relay `DESIGN.md` check.

**Task 32 [opus] Live checks and the final review** = plan Task 24, with: two desktops paired **by typed
code**; the phone by QR; remove the phone (no Kit step when the Kit was printed); remove the creator of a
saved Kit (Kit step follows); "Ověřit sadu" with the old and the new page; a Kit join on a fresh profile
after two removals; the removed phone's notice at unlock; the "longer than usual" line (stop the server
mid-round). Languages en, cs, de, ja on desktop and phone; the phone at **320 pt** width with the largest
text size (rows wrap); one pass in dark mode; the relay admin's Kit line. Then the final review against the
plan's Review Focus plus: the Kit seed never in the database, logs or IPC outside `RecoveryKit`; no UI
string contains "klíč synchronizace" or "výměna klíče" (`grep` in `src/i18n/locales/cs`).

## 8. Residuals and open questions

1. **R1, removed device + dishonest server vs a Kit join** (§0.1, §2.10). Accepted by D1; the "Data jsou
   zpět" card names the newest change's date. Owner: confirm acceptance.
2. **R2, a left device's leaked signing key** can add a device that survives key changes (§3.2). Visible
   through the new-device notice. Owner: confirm acceptance, or ask that leaving also rotates when another
   device is online (not proposed: the leaving device would generate and so know the new key).
3. **R3, Kit storage is what the app saw.** Screenshots, photos and pasting the copied code into a synced
   note are invisible; `copied` is treated like a file on the creator only.
4. **Printing from the built app** (`WebviewWindow::print`) is unverified on macOS, Windows and Linux;
   Task 32 decides between "Vytisknout" and "Uložit jako soubor…" as the primary.
5. **Open: should a Kit read without a join (`kitUsed`) also be surfaced by the relay admin?** Not
   designed; the person sees it in the app.
