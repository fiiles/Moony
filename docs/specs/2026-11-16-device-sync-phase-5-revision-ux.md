# Device sync, phase 5 revision: the Recovery Kit as a paper device, one removal, a plain device list — design

Date: 2026-11-16. Status: the owner's decisions D1–D4 of 2026-11-16 (taken on the recommendations of
`../Moony-tauri/docs/specs/2026-11-16-device-sync-ux-review.md`, "the review"), worked out for building,
with the security review of this document folded in (§9 lists what it changed). Sync stays behind the
runtime flag (`feature::RELEASED = false`); everything here lands **before** the flag is turned on. Old
development vaults are not upgraded: the owner sets sync up again.

This revises `2026-11-16-device-sync-phase-5-removal-design.md` ("the phase-5 spec") and its plan.
Where they differ, this document wins; the phase-5 spec's "As built" items 1–69 stay true except where
§1.3 below supersedes them. Czech texts are binding, English is the source for the other eight locales.

## 0. Security verdict on D1–D4 (read first)

None of D1–D4 is unsound. Three of them trade a property away; each trade is stated here so it is
accepted knowingly, and each is repeated as a residual in §8.

1. **D1: the Kit cannot tell a fresh envelope from an old one (R1).** The Kit learns nothing at a key
   change, so a person joining with it cannot check that what the server hands over is current. A
   dishonest server can replay a genuine envelope from before a removal (or, with a removed device's
   help, forge one for the Kit's public key) and serve the matching stale data; a removed device
   colluding with it would then read whatever the person types next. Today's Kit (it holds the current
   key) is immune once a key change happened. Not fixable without giving the Kit something new at every
   key change, which is what D1 removes. **Mitigation adopted (§2.6):** a Kit joiner sends nothing until
   the person answers "Data jsou zpět"; in such a stale copy the removed device is still on the list, and
   "Odebrat je" makes a key change that leaves it out. It bites only if the person keeps a device there
   that they had removed.
2. **D1: a copied Kit is cut off only by "Vytvořit novou sadu"**, never by removing a device. Inherent;
   compensated by making Kit use visible (§2.8).
3. **D3 trusts a vouch made before the issuer left (R2).** For an issuer that was *removed* this is
   airtight (removal always changes the key, §3.2). For an issuer that *left*, or an older ID that was
   superseded, a copy of its signing key made earlier (a profile backup made while sync was on) could
   add a device that keeps getting keys. **Mitigation adopted (§3.1):** such a certificate counts only
   when the issuer announced its pairing in the log before it left; otherwise the device is
   "Zkontrolujte" and gets nothing until a person keeps it.
4. D2 and D4 are sound. D2's costs are availability ones: the 32-changes-per-30-days cap now counts
   every removal, and a device offline for more than 30 days re-joins after any removal.
5. Unchanged by this revision but now stated for the owner: what a dishonest server can still do around
   a key change (As built 65 (b) and (c), R4 and R5 in §8).

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
| O7 (no per-change signatures) | Kept | The cost is R4 (§8). |
| O8 ("Vyměnit klíč" without removal) | **Superseded by D1/D2** | The only rotation that removes no device is "Vytvořit novou sadu". |
| O9 (first key change wins) | Kept | — |
| O10 (Kit joiner trusted, labelled) | **Superseded by D1** | A Kit joiner's certificate is signed by the Kit's key: verifiable like a pairing. |
| O11 (superseded rows) | Kept in substance | A quiet superseded row is hidden; one that still writes is "{name} (kopie)", "Zkontrolujte"; a superseded ID vouches only for what it issued before its successor (§3.1). |
| O12 (no retry after a lost race) | Kept | — |

### 1.3 Phase-5 text and As built items superseded

| Item | Fate |
|---|---|
| Spec §5.3 kind 3 ("recovery kit", self-issued) | Re-meant: issuer = the Kit (§2.3). New kind 5 = the Kit itself. |
| Spec §5.4 "A Kit joiner's first store" | Kept (trust on first use) plus the Kit anchor, the envelope's confirmed set and the push hold (§2.6). |
| Spec §6.4 step 3 (list-only), §6.6, §8 (whole), §9 (whole), §11.3 ("server: no change"), §13 item 10 | Replaced by §2, §4, §6 and the relay change. §13 item 10 (`syncRecoveryKitNeedsSnapshot`) disappears. |
| As built 14, 43 (Kit join identity, Kit heads) | Amended: `JoinIdentity::RecoveryKit(KitJoin)` carries the envelope's heads and confirmed set (§2.6). |
| As built 27 (who vouches) | Amended: a superseded ID speaks only until this device pinned its successor (R-25; not the successor's `issued_at`); a vouch by a left or superseded issuer also needs the §3.1 log-order check; a device left out for its issuer speaks for nothing (R-22). |
| As built 29 (`kit_unconfirmed`) | **Withdrawn.** The column stays in migration 040, always 0. |
| As built 33 (person-signed `confirmed` records, `sync_confirm_device`) | **Withdrawn.** Record type 3 and `confirm_record` stay in the format, unused; the command is not built. |
| As built 34 (`IssuerGone` opt-in ticks) | **Replaced** by §3.1 (`IssuerRemoved`, transitive; `LateVouch`). |
| As built 45 (an epoch with no head is not checked) | Amended for Kit joins: never unchecked (§2.6 step 5). Elsewhere unchanged. |
| As built 53, 54, 56, 57, 60 (removal API with mode, `opted_in`, `confirming`; list-only checks; pending JSON; commit confirms the ticked) | Amended in §4.2 and §7.1. |
| As built 50 ("a signer … not `kit_unconfirmed`") | The clause goes; everything else stands. |
| As built 65 residuals (b), (c) | Unchanged; restated for the owner as R4, R5. |
| Plan Tasks 11–24 | Replaced by Tasks 11–33 (§7.3). |

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
  (`trust::vouch`) and its creator may speak (§3.1), and only as the vault's **first** Kit: while no Kit
  was ever pinned here and it is the only Kit certificate in the pass. **At most one Kit is `trusted`**:
  a replacement Kit is pinned only through the verified key-change bundle that names it (`kit`, with the
  old Kit's removal, §2.5) or by the device that creates it; a refresh never pins one, and never chooses
  between Kits by ID (`ConflictKind::SecondKit`; As built R-24).
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
                          "keys":[{"epoch":n, "key":"<b64url>", "head":h?}],
                          "confirmed":[{"device":"<hex>", "cert":"<hex SHA-256 of the certificate>"}]}
sig    = Ed25519(signer, "moony kit envelope v1" ‖ header ‖ JSON bytes)
```

Built in `rekey/envelope.rs` like the bundle's entries (`bundle::os_fill`, wiped buffers, at most
256 KiB, vectors from fixed seeds). `keys` carries every epoch the sealer holds, ascending, the last one
the header's epoch; `head` only for **verified** heads (As built 63), so never for the newest epoch (the
envelope is sealed before the server answers the rotation's head; §2.6 step 5 covers it). `confirmed`
is the sealer's `confirmed` set after the key change commits (every recipient of this change plus those
confirmed before), so a Kit joiner learns what a bundle would have told it (§2.6 step 6).

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
- A change that makes a new Kit also removes every other `trusted` Kit (one silenced for its creator, or
  a second one): their removal records travel in the bundle's `removed` list (As built R-55). A device
  with no Kit it can seal to makes a new one at its next removal when the server holds one
  (`ReplacesKit::Missing`, R-57). A receiver pins a bundle's new Kit only when its creator is pinned there
  with the key the certificate names and is the signer or `trusted` (R-58).

### 2.6 Joining with the Kit after any number of key changes

`join::recovery_credentials` is rewritten (it refused any `rotation != 0`):

1. Decode the code (v2), derive `kit_token`, `kit_box_secret`, `kit_sign_seed`, `kit_id`.
2. `account_status(account)` as today (`syncRecoveryAccountInvalid`).
3. `GET /v1/vaults/:id/kit` with `Authorization: Bearer <kit_token>`:
   `401` / `404 vault_not_found` → `validation.syncRecoveryVaultNotFound`, reworded to cover a long
   replaced Kit (§2.9); `410 kit_replaced {replacedAt}` → `validation.syncRecoveryKitReplaced|{"at":<unix s>}`
   (replaces `syncRecoveryKitRotated`).
4. Open the envelope: header `vault_id` and `kit_id` must be the derived ones; the signature must verify
   with the bundled signer certificate; keys ascending, 32 bytes, the last one = the header epoch E. Any
   failure → `validation.syncRecoveryKitUnreadable` (new). (The signer is checked against the restored
   store in step 6.)
5. Credentials = all keys (`VaultKeys::from_parts`); `vault_info` with the derived token. Then the join
   as today (snapshot, replace, pull), with `JoinIdentity::RecoveryKit(KitJoin {kit_sign_seed, kit_id,
   heads, confirmed, signer})`. The envelope's heads go into `sync_epochs`. **The newest epoch E is never
   left unchecked** (As built 45's "no row" case does not apply to a Kit join):
   - E's head is recorded **verified** when the restored snapshot's `Keys` record names it (a snapshot
     only hands on verified heads, As built 63);
   - else, when the restored snapshot is **sealed under E**, it was taken after E began: an op past the
     snapshot's position sealed under an epoch below E is refused as `Invalid` (`invalidChain`), exactly
     the epoch rule with the snapshot's position standing in for the head; the head itself is learnt from
     the signed head record when the pull meets it;
   - else (the snapshot is older than E) E goes into `sync.epochs_unverified` with `after` = the
     snapshot's position, the server's head from `GET /rekey` when a bundle is younger than 30 days
     (otherwise none), and `by` = the envelope's signer, which is E's remover (an envelope is only
     replaced at a key change, by the device making it). `head::awaits_head` then holds every old-epoch
     op past `after` until the remover's signed head record is found and verified, exactly as on any
     device (As built 63); with no server head, the record's own head is taken once its signature
     verifies.
6. **Kit anchor and the store.** Inside the replace's savepoint: the restored rows must contain the Kit's
   row with a kind-5 certificate whose `sign_pub` is the derived one, **that the envelope's confirmed set
   names with that certificate's hash** (As built R-60), and no removal record that verifies; the joiner
   pins it, signs its own kind-3 certificate with `kit_sign_seed` (`Issuer::Kit`), then
   `tofu_from_rows` as today. Then **the envelope's signer must be pinned** with the bundled certificate
   and `speak_at` the envelope's `at`; then every `confirmed` item whose device is pinned with a
   certificate of that hash is marked `confirmed` (others are ignored). Any failure →
   `syncRecoveryKitUnreadable`, nothing replaced.
7. **Push hold.** The joiner writes the Local `sync.after_kit_join = {"devices": [ids], "newestChangeAt"}`
   (every other active device; the newest change's time) and, **while that key exists, its rounds pull
   but push nothing** (no changes, not even its own device row). The status shows "Data jsou zpět"
   (§6.4): "Odebrat je" opens the removal of the listed devices (a key change, after which the hold is
   lifted); "Mám je" lifts the hold. With no other active device the key is not written. Why: in a stale
   copy served by a dishonest server (R1) the removed device is still listed as active, so "Odebrat je"
   leaves it out of the new key before the person's first change goes anywhere.

**What a Kit joiner trusts:** the Kit (derived from the paper), on first use every certificate in the
restored data that verifies (phase-5 §5.4 TOFU) with a genesis only from the Kit's own issuer chain, and
the envelope's confirmed set (with any genesis it names) **only when its signer speaks on that store**
(As built R-106): pinned with the certificate the envelope bundles, `trusted`, not superseded and not left
out for its issuer (`trust::kit_join_speaker`). Two signers left out for their issuer still speak: the
Kit's creator (the Kit's certificate, which carries the Kit's own proof of possession, names it as
issuer), and a signer whose issuer is `removed` by the signer's own removal record (that record counted
only while the signer was a speaker). Any other signer opens the Kit but vouches for nothing: **a signer
that has `left`** (As built R-78: a left device's `left` record epoch is self-asserted and a backup of that
device holds the signing key and the vault key), a superseded ID, a late vouch, a device whose issuer
another device removed or whose issuer is unknown. A signer not pinned at all, or `removed`, makes the Kit
unreadable. **The envelope's signer check is only TOFU-strength against a holder of the current vault
key** (it can write rows and a snapshot, so a signer "pinned in the restored store" is whatever that
snapshot says); **the root of trust of a Kit join is the Kit-signed envelope's confirmed set**, which
binds the Kit's own certificate (step 6), **and the push hold** (step 7). Freshness it cannot check (R1);
the push hold is the answer.

**The review's note on phase-5 §8.3 step 3** (an old Kit marked valid while an old snapshot is still on
the server): resolved by construction. Nothing is probed against snapshots; the server answers a
replaced Kit's token with `410`, and the envelope always holds the current key, so a snapshot of any
epoch opens.

### 2.7 When the Kit is replaced (and the `kit_storage` column)

The Kit is replaced only (a) when the person asks ("Vytvořit novou sadu…", lost or seen by someone), or
(b) when it may be on a device being removed. (b) needs to know where it is kept, so migration 041 adds
the synced column `sync_devices.kit_storage` on the Kit's row (merged by strength, never lowered by
sync: As built R-54), written by the device
that shows the Kit (the creator; the Kit cannot be shown anywhere else):

| Value | Set by | Replaced when removing… |
|---|---|---|
| `paper` | "Vytisknout" (`sync_print_recovery_kit`) | never |
| `copied` | "Zkopírovat kód" (`sync_copy_recovery_code`) | the creator |
| `file` | "Uložit jako soubor…" to a local folder | the creator |
| `cloud` | the same, when `kit_file::looks_cloud_synced` says the folder syncs | **any** device (a stolen phone signed into the same cloud reads it) |
| NULL | nothing done (the flow was closed) | — (the Kit row says "neuložená", §6.3) |

The strongest value wins (`cloud` > `file` > `copied` > `paper`). During setup the value sits in the draft
and is written with the Kit's row at `finish_setup`. **The clipboard** already gets the concealed type
and the clipboard-history exclusion and is cleared after 60 s (macOS, Windows; iOS `.localOnly` with a
60 s expiry, phase 2); this revision adds `org.nspasteboard.TransientType` on macOS so clipboard tools
that honour it never keep the code. Not detected: a screenshot, a photo of the screen, a pasted code in a
synced note, and a PDF printed or saved into iCloud from the print dialog (R3).

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
  newer than the Local `sync.kit_read_seen`, more than an hour old, and no kind-3 entry was **pinned on
  this device** (`sync_trust.pinned_at`, this device's clock; never the certificate's own `issued_at`,
  which a Kit holder chooses) since `lastRead − 1 h`, the status carries `kitUsed` (§6.3). A dishonest
  server can hide a read; an honest one cannot be fooled by the reader.
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

**Limits.** A new limiter `kitRead` (10 per minute per client key) before the token check of
`GET /kit`; the usual `vaultIp` applies. A per-vault limit counts **only failed** Kit token attempts
(`401`/`410`, 30 per hour per vault, `kitVaultFailures`): a successful read with the right token never
waits on it, so nobody without the Kit (a removed device included) can lock the owner out of recovery.
**Retired Kit hashes** are kept, the newest 256 per vault (32 bytes each; at the 32-changes cap that is
eight months of nothing but new Kits). A Kit older than that reads `401`; the app's text for `401` on a
Kit join therefore covers it: "Tuto sadu server nezná: buď ji mezitím nahradila novější, nebo patří k
jiné synchronizaci či jinému serveru. Zkontrolujte kód a adresu serveru." / "The server doesn't know this
kit: a newer kit may have replaced it, or it belongs to another sync or server. Check the code and the
server address." (Cosmetic otherwise.) Deleting a vault or a person cascades. Backup: in the database
copy already. DESIGN.md: the routes, the storage line, Limits ("Kit envelope 256 KiB", "Kit reads",
"failed Kit attempts"), and *What the server knows*: "per vault, the SHA-256 of the Kit token and of up
to 256 replaced Kit tokens, the Kit envelope (ciphertext it cannot open), when the Kit was set, last
replaced and last read".

**Admin:** `GET /admin/api/people/:id` vaults gain `kitSince`, `kitLastRead`; Person detail shows per
profile "Záchranná sada: vytvořena {date} · naposledy použita {date | nikdy}" / "Recovery kit: created
{date} · last used {date | never}" (cs and en strings in `admin-ui/src/i18n`). While there, the review's
wording point: the admin's "profil" gets the subtitle "synchronizovaná data jednoho profilu Moony".

### 2.10 Threats

| Attacker | Can | Cannot | Detected by |
|---|---|---|---|
| **Stolen or copied Kit** (with the server address) | Read everything, past and future, until "Vytvořit novou sadu"; join as a device | Survive "Vytvořit novou sadu": the new key and the new envelope are sealed to keys it lacks, its token gets `410` | `kitUsed` (read without a join, judged on this device's clock), the Kit-join notice on every device |
| **Stolen Kit + dishonest server** | The above, and the server can hide the read | Survive a new Kit: the cut-off is cryptographic, not the server's | The Kit-join notice only |
| **Server alone** | Deny, delay; replay an older genuine envelope with the matching older data to a Kit joiner (R1) | Read anything: it never sees a key, `kit_box_pub` or the seed; read what the joiner writes (it has no key of its own) | The push hold and "Data jsou zpět" (date of the newest change, the device list) |
| **Removed device** (with its old keys and signing key) | Nothing new: it has no Kit token (the seed was never stored) unless it kept the Kit file or code, which is why §2.7 replaces the Kit then | Read the envelope; write anything (its token is read-only, then unknown); add a device that gets keys (§3.1, transitive) | — |
| **Removed device + dishonest server** | R1: serve a Kit joiner a stale copy (replayed or forged envelope); with the push hold it reads the joiner's changes only if the person keeps that device ("Mám je"). R4: fill not-yet-pulled slots before a key change with its own later changes for a lagging device | Read data written by devices that stayed | "Data jsou zpět"; `keyChangeOverrun` in part |
| **Device that left, or a superseded ID, from a profile backup with its signing key** | Until the next key change: everything (O2); after it: nothing, unless a person keeps a device it adds | Add a device that silently gets keys: a certificate it backdates appears in the log after its `left` record (or its successor), so it is "Zkontrolujte" and excluded (§3.1) | "Zkontrolujte" row, new-device notice |

**Rotation head interaction:** the envelope carries only verified heads, never the newest (unknown when
sealed); a Kit joiner checks the newest epoch as §2.6 step 5 says, never leaving it unchecked. The signed
head, the provisional heads and pickup-before-pull of the running fix round are unchanged.

## 3. Automatic confirmation (D3)

### 3.1 The rule

`trust::recipients(conn, removing) -> Recipients {included, excluded, kit}` includes every pinned
`trusted` device entry, this device included, **except**:

| Reason | When |
|---|---|
| `Removing` | the person removes it now |
| `Left`, `Removed` | its state (as today) |
| `SupersededQuiet` | O11, as today |
| `IssuerRemoved` (new, **transitive**) | it is not `confirmed` and its issuer (paired, re-join or Kit-joined) is `removed` here **or is itself `IssuerRemoved`**; computed to a fixpoint (`trust::issuer_removed_set`, replacing `issuer_gone` in `recipients`), so a whole chain hanging off a removed device drops, however deep. A `confirmed` entry stops the chain. |
| `LateVouch` (new) | it is not `confirmed`, its issuer is `left` or superseded, and it was not seen in time (below); or its issuer is itself `LateVouch` (As built R-6) |
| `IssuerUnknown` (As built R-23) | it is not `confirmed` and its issuer is not pinned here (a certificate a Kit joiner trusted on first use whose issuer has no row), or its issuer is itself `IssuerUnknown` |
| `Unverified`, `NoKey` | a row without a pinned certificate (as today); a row that only calls itself `recoveryKit` without a Kit certificate that verifies is such a row (R-26) |

**Who may speak** (As built R-22). An entry left out for its issuer (`IssuerRemoved`, `LateVouch`,
`IssuerUnknown`) gets no key from this device and speaks for nothing on it: its key change is not taken
(`keyChangeUnverified`; `trust::speaker`), a bundle it signs is refused before anything is pinned or
confirmed, and its removal records count nowhere (`removal_counts`). Kept ("Je moje") or named by a key
change, it is `confirmed` and speaks again. It still vouches: what it pairs is pinned (so it can show as
"Zkontrolujte" and be kept) and left out with it by the transitive rule; a Kit it creates is not pinned.

`confirmed` (the migration-040 column) means "named as a recipient by a verified key change, or kept by a
person on this device": set by the remover's own commit for every recipient and the Kit, by
`import_bundle` for the signer, every recipient and the Kit, by a Kit join from the envelope's confirmed
set (§2.6), and locally by "Je moje" on a "Zkontrolujte" row (§6.1); the next key change this device
makes then names it, which confirms it everywhere. No records, no tick boxes.

**Seen in time** (the R2 mitigation; Local data from migration 041):

- *Where things were first seen.* The pull notes, for each applied `sync_devices` change that brings a
  `cert` or `left_record` value, the relay position of its op in the Local table
  `sync_trust_seen (device, what ∈ {cert, left}, pos)` (first one wins), when the row now holds that value
  and it verifies (As built R-27). Rows restored from a snapshot get the snapshot's position. A pairing
  export carries the holder's positions (`certSeen`, `leftSeen`, `ticketSeen`), which replace the
  joiner's own for the exported devices, so a joiner leaves out for their issuer what its holder does
  (R-23). Entries that came from a bundle, this device or a Kit join's envelope need nothing: they are
  `confirmed`.
- *Pairings announced by the holder.* `pairing::holder::confirm` queues, under the holder's current key,
  an envelope with no rows and the changeset's new optional field `"tickets": ["<ticket_pub b64url>"]`
  (the shape of the signed head record, As built 62: a reader before this build ignores it); it leaves
  through `sync_unsent` in the holder's own chain, so it always reaches the server **before** a later
  `left` record of the same device. Receivers record it in the Local table
  `sync_tickets (ticket_pub PRIMARY KEY, issuer, pos)`.
- A paired certificate whose issuer is `left` is **in time** when its ticket was announced, or the
  certificate itself was seen, at a position before the issuer's `left` record was seen; a re-join or
  Kit-free certificate likewise by its own first-seen position. For a superseded issuer the end is the
  position where its successor's certificate (another than the one judged) was first seen. **Equal
  positions are late** (As built R-23): only one restored snapshot gives them, and it cannot tell which
  came first (a joiner, by pairing or by Kit, and a device catching up from a snapshot alike). An
  unknown position on either side is in time (that side came by a vetted path).
- **A superseded ID speaks only until its successor**: `speaks_at` bounds a `trusted` entry that has a
  pinned re-join successor at the time **this device pinned** its first successor (`pinned_at`, this
  device's clock), as it bounds a left entry at `state_at`; never at the successor's own `issued_at`,
  which whoever made the certificate chose (As built R-25). O11's "wrote since its successor" counts from
  the earlier of the two.

The D3 trap still passes on a device that pulls the ops: O pairs N, O's announcement goes out before O's
`left` record, so N is in time on a device D that was offline throughout, whatever order N's row arrives
in. A device that gets O's `left` record and N's certificate from one restored snapshot (a Kit joiner, or
a device catching up past compacted ops) cannot see the announcement: N is "Zkontrolujte" there until the
next key change names it or the person keeps it (accepted UX cost, R-23). A pairing joiner takes its
holder's positions, so it judges N as its holder does.

### 3.2 Why it is safe

A certificate becomes a recipient on device D only if its signatures verify, its issuer is pinned on D
with the key the certificate names, and that issuer `speaks_at` the certificate's (and its ticket's)
time. Producing such a certificate after the fact needs the issuer's signing key **and** a way to put a
row into the vault. Case by case:

- **Issuer trusted** (it, or its unlocked thief, is a member): it can add devices anyway (O9, phase-5
  §12). Nothing changes.
- **Issuer removed:** D2 makes every removal a key change, so the issuer's key-holder lost the current
  vault key at that moment and cannot write rows. Its only path is the fork residual of As built 65(b)
  (R4): a dishonest server serving a lagging device old-epoch ops in pre-rotation slots. Such a
  certificate was not named by the bundle that cut its issuer off, so it is `IssuerRemoved` and gets
  nothing, **and so does everything it vouches for**, because the exclusion is transitive: a forged B
  that pairs a C cannot launder C into the recipients. A genuine device the remover did know was a plain
  recipient (its issuer was still `trusted` at plan time) and is `confirmed` everywhere through the
  bundle, which stops the chain. A genuine device the remover did not know yet (paired a minute before)
  never got the new key in either design; it shows "Připojit znovu".
- **Issuer left or superseded:** the honest device deleted its signing key when it left (a superseded ID
  lives on only as a copy). The key survives only in a copy made earlier (a profile backup made while
  sync was on, phase-5 §12), and that copy also holds the current vault key until the next key change,
  so it can write a backdated certificate. On an honest server it can only append to the log, after the
  issuer's `left` record (or successor) and without an earlier announcement of its ticket, so the
  certificate is `LateVouch`: listed as "Zkontrolujte", excluded, kept only if a person says "Je moje".
  On a dishonest server, forking slots that a lagging device has not read yet, it is R4.
- **Issuer = the Kit:** a Kit holder can add devices by design (D1); visible, cut off by a new Kit,
  after which its later joiners (and what they pair) are `IssuerRemoved` unless the new-Kit bundle
  named them.

### 3.3 What the person still sees

- Each device's details: "Přidáno {date} ze zařízení {name}" / "Připojeno záchrannou sadou {date}".
- **A new-device notice on every other device** when an entry is pinned outside a join
  (`trust::refresh` appends to the Local `sync.new_devices`, at most 20, kinds 2 and 3; re-joins say
  nothing): "Přibylo zařízení {name}, přidané ze zařízení {by} {date}." A `LateVouch` entry gets the
  stronger text with "Je moje" · "Odebrat" (§6.3); Kit joins always get the strong notice.
- The removal dialog's "Synchronizovat dál budou" list (§4.3): the people the new key goes to.

## 4. Always-rotate removal (D2)

### 4.1 What happens to which row

| Row | In the list | Removal | At the next key change |
|---|---|---|---|
| Active device | active | "Odebrat zařízení" → key change | — |
| Left (signed record) | "Dřívější zařízení", no action | — | gets a notice entry and is cut off (`cut_off_all_pending`, unchanged) |
| Superseded, quiet | hidden (the successor's details say "Znovu připojeno {date}") | — | left out (O11) |
| Superseded, still writing | "{name} (kopie)", Zkontrolujte | "Odebrat" → key change | — |
| `IssuerRemoved`, `Unverified` | Zkontrolujte | "Odebrat" → key change | left out |
| `LateVouch` | Zkontrolujte | "Je moje" (kept, then included) or "Odebrat" → key change | left out until kept |
| Removed | "Dřívější zařízení", no action | — | — |

"Dřívější zařízení" rows are listed for 30 days after their state time (`devices::list` leaves older
ones out); the rows and the trust entries stay in the database.

### 4.2 The API

- `RemovalMode` and the list-only path are deleted (`rotate.rs`, its tests; `sync_removal_plan` takes no
  mode). Removal records naming this device that arrive through sync are still handled (Task 20): a
  commit writes them into rows, so they still sync.
- `rotate::plan(conn, removing, also_remove, new_kit) -> Plan {epoch, removing, recipients, excluded,
  added_by_removing, kit: Option<TrustEntry>, replaces_kit, consent}`. `replaces_kit` = `new_kit`, or the
  §2.7 table says so for a device in `removing`. **`added_by_removing` holds every descendant** of the
  devices being removed through issuer links, at any depth (a device a removed device paired, what that
  one paired, …), Kit-joined devices included when the path runs through a Kit that is being replaced
  (`new_kit`, or `replaces_kit` from §2.7). A Kit that stays is not a removed issuer, so its joiners are
  not listed.
- `consent` = hex SHA-256 over the domain string, the epoch, the removed IDs, each recipient's ID and
  certificate hash, the current Kit's certificate hash (or none) and `replaces_kit`.
- `remove(relay, db, Removal {removing, consent, new_kit}, data_dir, forget_pairing, now)` →
  `Outcome::{Changed {epoch, removed, kit: Option<NewKit>}, Pending {kit: Option<NewKit>},
  AlreadyRemoved {by}, Raced {by}, RemovedHere}`. `NewKit` holds the `KitSecret` (redacting, wiped) for
  the UI to show once; the command turns it into a `RecoveryKit` and drops it.
- Preconditions stay (As built 54) minus the list-only one; `syncRemovalPlanChanged` keeps its meaning.

### 4.3 The dialog (one screen)

Title, what happens, who stays, devices the removed one added (all descendants, ticks, unticked), the
"in your hand" hint, the Kit line when `replaces_kit`, the password, "Zrušit" · "Odebrat zařízení". Then
"Odebírám {name}…" in the dialog body, then a toast, or the Kit step when `replaces_kit`. Texts in §6.2.
The excluded list is shown only for rows that are **active or Zkontrolujte** and still drop out
("Synchronizovat se přestanou i:"): `IssuerRemoved`, `LateVouch` (with "Je moje" inline, which keeps it
and reloads the plan) and unverified rows.

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
| … late vouch | Přidalo ho zařízení {by}, které se už nesynchronizuje, a Moony nemůže ověřit, že to bylo dřív, než se odpojilo. Až příště odeberete nějaké zařízení, přestane se synchronizovat, pokud ho nepotvrdíte. · Je moje · Odebrat | {by} added it, but {by} no longer syncs and Moony can't confirm this happened before it left. The next time you remove a device, it will stop syncing unless you keep it. · It's mine · Remove |
| Folded group | Dřívější zařízení ({n}) | Former devices ({n}) |
| Former row | odpojeno {date} · odebráno {date} zařízením {name} | left on {date} · removed on {date} by {name} |
| Under the list | Ztracené, prodané nebo vymazané zařízení odeberte. Nic nového už neuvidí. | Remove a lost, sold or wiped device. It won't see anything new. |
| Pre-phase-5 vault (O3) | Tuto synchronizaci nastavila starší verze Moony, proto v ní nejde odebírat zařízení. Smažte synchronizovaná data, nastavte synchronizaci znovu a připojte ostatní zařízení. | An older version of Moony set up this sync, so devices can't be removed from it. Delete the synced data, set sync up again and connect your other devices. |

Desktop: a row expands for its details and the "Odebrat…" button; phone: other devices' rows open the
device sheet (details as label-value rows, "Odebrat zařízení" as a danger text button). The
"Ztratili jste zařízení…" collapsible goes. "Je moje" needs no password (it gives nothing now; the next
removal, which needs one, is where it takes effect) and sets `confirmed` locally (`sync_keep_device`).

### 6.2 Removal and the new Kit

| Place | Czech | English |
|---|---|---|
| Title | Odebrat zařízení {name}? | Remove {name}? |
| Body | {name} přestane dostávat nová data a nebude moct nic měnit. Co už v něm je, tam zůstane; chrání to heslo do Moony. | {name} will stop getting new data and won't be able to change anything. What's already on it stays there, protected by its Moony password. |
| Who stays | Synchronizovat dál budou: · {name} (toto zařízení) | These will keep syncing: · {name} (this device) |
| Who drops out (only if any) | Synchronizovat se přestanou i: | These will also stop syncing: |
| Added by the removed one | Tato zařízení jste přidali ze zařízení {name} (nebo z nich). Odeberte i ta, která nepoznáváte. ☐ {device} · přidáno {date} | You added these from {name} (or from them). Remove any you don't recognise too. ☐ {device} · added {date} |
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
| New device, late vouch | Přibylo zařízení {name}. Přidalo ho zařízení {by}, které už se nesynchronizuje. Pokud {name} nepoznáváte, odeberte ho. · Je moje · Odebrat | {name} was added. {by} added it and no longer syncs. If you don't recognise {name}, remove it. · It's mine · Remove |
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
| problem `keyRotated` for more than a day (R5: a head that never verifies) | **Synchronizace čeká na zařízení {by}** · Toto zařízení čeká na potvrzení od zařízení {by}, které naposledy měnilo seznam zařízení. Data tady jsou v pořádku. Pokud {by} už nepoužíváte, připojte toto zařízení znovu. | **Sync is waiting for {by}** · This device is waiting for a confirmation from {by}, which last changed the device list. The data here are fine. If you no longer use {by}, connect this device again. | Připojit znovu |
| `keyChangeOverrun` | Synchronizační server poslal tomuto zařízení změny, které po odebrání zařízení {date} poslat neměl, a Moony je použila. Pokud jste zařízení odebrali kvůli krádeži, zkontrolujte změny z posledních dní. | The sync server sent this device changes it shouldn't have after a device was removed on {date}, and Moony applied them. If you removed that device because it was stolen, check the changes of the last few days. | Zavřít |
| `removedNotice` (readable with sync off) | **Toto zařízení bylo odebráno ze synchronizace** · Odebralo ho zařízení {by} {date}. Vaše data tady zůstala, jen se už nesynchronizují. (+ {n} změn z tohoto zařízení se do synchronizace nedostalo; zůstávají jen tady. — plurals) | **This device was removed from sync** · {by} removed it on {date}. Your data stayed here; they just don't sync any more. (+ {n} changes from this device didn't make it into sync; they stay only here.) | Rozumím · Připojit znovu |
| `afterKitJoin` (the Kit joiner only; holds pushes) | **Data jsou zpět** · Poslední změna je z {date}. V seznamu zůstala vaše dřívější zařízení: {names}. Pokud jste o ně přišli, nebo pokud mezi nimi je zařízení, které jste už dřív odebrali, odeberte je. Dokud se nerozhodnete, nic odsud neodešleme. | **Your data are back** · The newest change is from {date}. Your earlier devices are still on the list: {names}. If you lost them, or if one of them is a device you had already removed, remove them. Until you decide, nothing is sent from here. | Odebrat je · Mám je |

"Připojit znovu" = password → turn sync off (local data stay) → the join choice; before replacing, the
replace notice adds "Změny, které odsud neodešly, zůstanou v záloze." (the `-kept` backup). The removed
notice shows in Settings → Sync, **once at unlock** (desktop dialog, phone sheet) and as a line in the
top-bar "Stav dat" popover ("Synchronizace je vypnutá: toto zařízení bylo odebráno.") until "Rozumím".
`newDevices`, `kitUsed`, `removalNotice` and `afterKitJoin` add one line each to the popover. Other
phase-5 texts and the three worst of today per the review §4.9 table ("Čeká se na změnu z jiného
zařízení", `captureBroken`, `invalidChain`) are reworded in the same task. The turn-off dialog: review
§4.6 texts; `turnOff.noCutOff` goes; "Synchronizovat toto zařízení znovu" moves under the problems card.

Glossary (`docs/standards/i18n-glossary.md`): add odebrat zařízení / remove device, dřívější zařízení /
former devices, připojit znovu / connect again, zkontrolujte / check this, záchranná sada (lower case) /
recovery kit, ověřit sadu / check kit; **forbidden in UI**: klíč synchronizace, výměna klíče, odříznout,
synchronizační účet, číslo účtu (own server).

### 6.5 Wire (`shared/schema.ts`, the seven steps)

- `SyncDevice` gains `status: 'active' | 'check' | 'former'`, `checkReason: 'unverified' | 'copy' |
  'issuerRemoved' | 'lateVouch' | null`, `formerReason: 'left' | 'removed' | null`, `joinedVia: 'created' |
  'paired' | 'recoveryKit' | 'rejoined' | 'unknown'`, `addedBy: string | null`, `stateAt: number | null`,
  `stateBy: string | null`, `removalPending: boolean`, `inactiveDays: number | null`.
- `SyncStatus` gains `removalSupported`, `removalPending`, `removalNotice`, `removalNotDone`,
  `keyChangeOverrun`, `removedNotice`, `rejoinBy`, `waitingForRemover: {by, since} | null` (R5 text),
  `newDevices: {id, name, by, at, viaKit, lateVouch}[]`, `recoveryKit: {createdAt, createdBy, storage,
  lastReadAt} | null`, `kitUsed: number | null`, `afterKitJoin: {names: string[], newestChangeAt: number |
  null} | null`, `syncingSince: number | null`, `firstUpload: boolean`. `SyncProblem` gains `keyNotGiven`,
  `keyChangeUnverified`.
- `RecoveryKit`: `accountNumber` goes; gains `createdAt`, `createdBy`; `recoveryCode` is 19 groups.
- New: `RemovalPlan {removing, staying, droppingOut: {device, reason}[], addedByRemoving, replacesKit:
  'no' | 'creatorRemoved' | 'cloud' | 'asked', consent}`, `RemovalOutcome {kind: 'done' | 'pending' |
  'alreadyRemoved', by: string | null, kit: RecoveryKit | null}`, `KitCheck {result: 'valid' | 'replaced'
  | 'otherSync' | 'unknown', replacedAt, currentCreatedAt}`.
- Commands: `sync_removal_plan(deviceIds, alsoRemove, newKit)`, `sync_remove_devices(password, deviceIds,
  alsoRemove, newKit, consent)`, `sync_check_recovery_kit(code)`, `sync_print_recovery_kit()`,
  `sync_keep_device(deviceId)` ("Je moje"), `sync_kit_join_decided(removeThem: boolean)` (lifts the push
  hold; with `true` the UI opens the removal of the listed devices first and calls it after the removal),
  `sync_dismiss_notice(kind)` with kinds `removal | removed | removalNotDone | keyChangeOverrun |
  newDevices | kitUsed`. **Deleted:** `sync_show_recovery_kit` (and `setup::recovery_kit`). Changed:
  `sync_prepare_recovery_kit` makes the draft; `sync_finish_setup(deviceName)` loses `kitSaved` (refuses
  with `validation.syncRecoveryKitNotConfirmed` while the draft has no storage value).
- New validation keys: `syncRecoveryKitReplaced` (`{at}`), `syncRecoveryKitUnreadable`. Reworded:
  `syncRotationRaced`, `syncTooManyKeyChanges`, `syncTooManyDevices`, `syncRecoveryVaultNotFound` (the
  `401` text of §2.9). Never added: `syncRecoveryKitNeedsSnapshot`.

## 7. Impact on the plan

### 7.1 Code of Tasks 1–10 that changes (after the running fix round lands; nothing here touches its files before)

| File | Change |
|---|---|
| `identity/cert.rs`, `identity/tests.rs` | `CertKind::Kit = 5`; kind 3 issuer = the Kit; `issue_kit(creator, fields, kit_seed)`, `issue_kit_join(kit_seed, kit_id, fields, subject)`; `verify_signatures` per kind; kind-3 vectors regenerated, kind-5 added. |
| new `identity/kit.rs` | `KitSecret` (seed, redacting), the four derivations of §2.2, `kit_id`. |
| `identity/mod.rs` | `Issuer::RecoveryKit` → `Issuer::Kit {seed, kit}`; `write_once_value_valid` knows kind 5. |
| `identity/trust.rs`, `trust_tests.rs` | `judge_kit` → kind 5 via `vouch` + one trusted Kit (`ConflictKind::SecondKit`); kind 3 via `vouch`; `kit_unconfirmed` never set (column stays); **`speaks_at` bounds a superseded `trusted` entry at its successor's `issued_at`** (the successor map moves out of `recipients`); `recipients(conn, removing)` returns `kit`, drops `opted_in`, `KitUnconfirmed`, `IssuerGone` (the one-level `issuer_gone` closure, around line 1188), adds **transitive** `IssuerRemoved` (`issuer_removed_set`, fixpoint) and `LateVouch`; `keep(conn, device)` sets `confirmed`; `import_bundle` takes the bundle's `kit`; `apply_confirmed_set(conn, items)` for a Kit join; `confirm` deleted; `export` drops `kitUnconfirmed`; `refresh` writes `sync.new_devices`. Tests of As built 29/33/34 rewritten as §3 cases. |
| `changeset.rs` | Optional `"tickets"` field next to `"rekey"` (As built 62's pattern); unknown to older readers. |
| `pairing/holder.rs`, `pairing/tests.rs` | `confirm` queues the ticket announcement (§3.1) in the same hold that reads the payload; the export carries kind 5 entries; slot-2 vectors regenerated only if they include `kitUnconfirmed`. |
| `engine/pull.rs` | Records `sync_trust_seen` and `sync_tickets` positions for applied changes; the Kit-join epoch rule for E (§2.6 step 5: the snapshot position as the bound when the snapshot is under E). |
| `engine/mod.rs` / `engine/push.rs` | No push while `sync.after_kit_join` exists (pull only). |
| `rekey/head.rs` | `note_unverified` accepts no server head (the signed record's head is taken); `verify_heads` then uses the record's head. The envelope reads `head::verified_epochs`. |
| `rekey/bundle.rs`, `bundle_tests.rs` | Version 0x02, JSON `v: 2`, `kit` field (`BundleInput.kit`, `BundleBody.kit`); vectors. |
| new `rekey/envelope.rs` (+ tests) | §2.4, with `confirmed`. |
| `relay/{mod,http,fake}.rs` (+ tests) | `rotate_token(vault, new_token, bundle, kit: Option<&KitUpdate>)`, `put_kit`, `get_kit(vault_id, kit_token)`, `VaultInfo.kit`, feature `kit`, errors `KitReplaced {at}`, `KitExists`; fake mirrors §2.9. |
| `rekey/rotate.rs`, `engine/tests/rekey_rotate.rs` | §4.2: no `RemovalMode`, `Plan`/`Removal`/`Outcome` as there, `added_by_removing` all descendants; envelope built (with the confirmed set) and sent; pending JSON gains `kit`; commit marks every recipient and the Kit `confirmed` (replaces `trust::confirm(ticked)`), writes the new Kit row and the old Kit's removal record; consent covers the Kit. |
| `rekey/pickup.rs`, `engine/tests/rekey_pickup.rs` | The signer check loses `kit_unconfirmed`; `import_bundle` with `kit`; Local `sync.key_change_unverified = {by}` on `Unverified`. |
| `recovery.rs` | Code v2 (§2.2). |
| `join.rs` | `recovery_credentials` per §2.6; `JoinIdentity::RecoveryKit(KitJoin)`; the push-hold key. |
| `setup.rs`, `commands/sync.rs`, `commands/sync_kit.rs`, `kit_file.rs` | Kit draft, registration at `finish_setup`, storage values, print, the deleted re-show; the Kit page header line. |
| `src-tauri/plugins/moony-keychain/macos/…` | `org.nspasteboard.TransientType` next to the concealed type. |
| `devices.rs`, `impact.rs`, `engine/status.rs` | Skip the Kit row; statuses of §6.5; `waitingForRemover` from `head::unverified_epochs` older than a day. |

### 7.2 Migration, schema, wire

- **Migration 041 `041_sync_recovery_kit`** (main ends at 039, this branch has 040, no other branch has
  04x):
  ```sql
  ALTER TABLE sync_devices ADD COLUMN kit_storage TEXT;               -- synced, strongest wins (R-54), only on the Kit's row
  CREATE TABLE sync_trust_seen (device TEXT NOT NULL,
      what TEXT NOT NULL CHECK (what IN ('cert','left')), pos INTEGER NOT NULL,
      PRIMARY KEY (device, what));                                    -- Local
  CREATE TABLE sync_tickets (ticket_pub BLOB PRIMARY KEY, issuer TEXT NOT NULL,
      pos INTEGER NOT NULL);                                          -- Local
  ```
  Registry: `kit_storage` Synced, merged by strength (R-54), not write-once; both tables Local, skipped by the
  export, deleted by `turn_off` and a join's replace (kept by a repair or re-sync, like `sync_trust`).
  Schema number 41 (the barrier stays). `sync_trust.kit_unconfirmed` and `sync_state.kit_epoch_saved`
  stay unused (documented in `database.md`).
- Relay schema 7 (§2.9).
- Wire: §6.5, one mixed commit (Task 24).

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
a Kit token equal to a vault token refused; `kitRead`; **the per-vault limit counts only failures: 100
wrong-token attempts then the right token still reads**; the 257th retired hash drops the oldest; 256 KiB;
cascade on vault and person delete; info's `kit`; `features`. Done: all green, DESIGN.md updated.

**Task 12 [sonnet] Relay admin: the Kit per profile (§2.9).** Files: `src/admin/api.ts`,
`admin-ui/src/screens/PersonDetail.tsx`, `admin-ui/src/i18n/{cs,en}.ts`, `test/admin-people.test.ts`.
Done: `kitSince`, `kitLastRead` in the person view; the line shows "nikdy" / "never"; the profile subtitle.

**Task 13 [opus] Kit formats: code v2, derivations, kind 5 and kind 3, envelope (§2.2–§2.4).** Files:
`recovery.rs`, new `identity/kit.rs`, `identity/cert.rs`, `identity/mod.rs`, new `rekey/envelope.rs`, tests
and vectors. Tests first: code v2 round trip, typo and v1 refusal, QR v2; derivation vectors; kind 5 and
the new kind 3 sign/verify, flipped bytes; envelope vectors (with a `confirmed` set), a stranger cannot
open, a changed header, signature, key order or confirmed item fails, 256 KiB cap. Done: gates green,
vectors committed.

**Task 14 [opus] Trust store revision and migration 041 (§2.3, §3).** Files: `db/migrations.rs`,
`db/schema.snapshot`, `registry/tables.rs`, `registry/keys.rs`, `merge/columns.rs`, `identity/trust.rs`,
`trust_tests.rs`, `docs/architecture/database.md`. Tests first: the Kit pinned via its creator, one
trusted Kit, `SecondKit`; kind 3 via the Kit, refused after the Kit's removal unless dated before;
**transitive `IssuerRemoved`, both chain cases**: (a) A removed, B paired by A and not named by the
bundle, C paired by B → B and C excluded; (b) A removed, B as in (a), C paired by B, D re-joined from C
→ all three excluded, while a `confirmed` B stops the chain (C and D included); (c) a replaced Kit, a
device it joined that the bundle did not name, and a device that one paired → both excluded; **superseded `speaks_at`**: a certificate issued by an older ID after its successor's
`issued_at` is not pinned; `LateVouch`: a cert of a left issuer seen after its `left` record without an
announcement is excluded, with an earlier announcement included (the D3 trap, both arrival orders), equal
positions included, `keep` includes it; a superseded issuer likewise against its successor's position; the
Kit returned apart, never counted; `import_bundle` confirms recipients and the Kit and applies
`replaces`; `apply_confirmed_set` marks only matching certificate hashes; new-device notices written for
kinds 2 and 3 outside a join, not for this device or kind 4; migration 041 test. Done: no code reads or
sets `kit_unconfirmed`; `trust::confirm` and `issuer_gone` gone.

**Task 15 [opus] Where entries were first seen; pairing announcements (§3.1).** Files: `changeset.rs`,
`pairing/holder.rs`, `pairing/tests.rs`, `engine/pull.rs`, `engine/tests/pull.rs`, new
`engine/tests/trust_order.rs`. Tests first: a holder's confirm queues one empty envelope with `tickets`
before anything else it sends; it reaches the server before a later `left` record of the same device
(`leave_best_effort` after a pairing); the pull records first-seen positions for `cert` and `left_record`
values and ticket announcements, keeps the first, records the snapshot's position for restored rows;
an older reader ignores `tickets`. Done: gates green.

**Task 16 [sonnet] Relay client and fake: the Kit (§2.9).** Files: `relay/{mod,http,fake}.rs`,
`relay/fake/tests.rs`, `relay/http/tests.rs`, every `impl Relay for`. Tests: the HTTP shapes and error
mapping; the fake's atomic rotation with the envelope, `kit_required`, `410` with `replacedAt`, info's
`kit`, failures-only limit. Done: gates green.

**Task 17 [opus] Removal with the Kit, always rotating (§2.5, §2.7, §4).** Files: `rekey/rotate.rs`,
`rekey/bundle.rs`, `rekey/pickup.rs`, `engine/tests/rekey_rotate.rs`, `rekey_pickup.rs`, `rekey_fixture.rs`.
Tests first: the plan's `replaces_kit` per the §2.7 table; consent changes with the Kit; **`added_by_removing`
lists grandchildren and Kit-joined descendants when the Kit is replaced, not when it stays**; a removal
sends the envelope sealed to the current Kit (opened with its secret: every key, verified heads only, the
confirmed set equal to the store's after commit); a new-Kit change retires the old token on the fake, the
old Kit reads `410`, the new one opens; pending keeps and re-sends the Kit part; commit confirms all
recipients and writes the Kit rows; a pickup pins the bundle's Kit; list-only tests deleted. Done: gates
green; the running fix round's tests still pass.

**Task 18 [sonnet] Setup and the Kit lifecycle (§2.2, §2.7, §2.8).** Files: `setup.rs`, `commands/sync.rs`,
`commands/sync_kit.rs`, `kit_file.rs`, `engine/status.rs`, the macOS keychain plugin (TransientType). Tests:
the draft is never in the database (scan `sync_state` and `sync_devices` after setup for the seed bytes);
`finish_setup` writes the Kit row and calls `put_kit` (idempotent on retry); storage values and their
order; `check_kit` results; a server without `kit` refuses setup; `kitUsed` uses `pinned_at`: a Kit join
whose certificate claims a time after the read but was pinned before it does not explain the read, and an
hour's grace holds. Done: gates green.

**Task 19 [opus] Joining with the Kit (§2.6).** Files: `join.rs`, `rekey/head.rs`, `engine/pull.rs`,
`engine/mod.rs`, `engine/tests/join_new.rs`, new `engine/tests/kit_join.rs`. Tests first: join after 0, 1
and 3 key changes; after 31 days with (i) the snapshot's `Keys` naming E, (ii) a snapshot under E without
it (an old-epoch op injected past the snapshot is `Invalid`), (iii) a snapshot older than E (old-epoch ops
past it wait until the envelope signer's head record verifies, then apply; a record by another signer
refutes); a replaced Kit (`410`); an unknown one (`401`, the reworded text); a tampered envelope, a
missing or removed Kit row, an envelope signer not in the restored store or not speaking at `at`
(`syncRecoveryKitUnreadable`, nothing replaced); the confirmed set keeps a genuine device whose issuer was
removed (bundles expired) as a recipient; the joiner's kind-3 certificate pins on another device;
**the push hold**: nothing pushed (not even the own row) until `sync_kit_join_decided`, pulls go on; in a
stale copy where a removed device is listed, "Odebrat je" makes a key change that leaves it out. Done:
gates green.

**Task 20 [sonnet] Leaving and records about this device** = plan Task 11 without the list-only parts
(files and tests as written there), plus: a device that pairs and then leaves sends the ticket
announcement first (shared fixture with Task 15).

**Task 21 [sonnet] Documents and photos across a key change** = plan Task 13, unchanged.

**Task 22 [opus] Multi-device scenarios** = plan Task 14, amended: no list-only cases; the D3 trap (O pairs
N, O turns sync off, D offline throughout, then D removes X: N keeps syncing, both arrival orders); a
backdated certificate from a left device's copy appended later stays out until kept; the transitive
chain from a removed issuer through a forked slot (fake relay serving a lagging device) stays out; a new
Kit while a Kit-joined device is offline (it picks up and stays); a Kit join after the property test's
key changes. Property: also "the Kit opens the newest envelope with every key" and "no excluded chain
member ever holds a key of an epoch after its issuer's removal".

**Task 23 [sonnet] Against a real sync server** = plan Task 15, plus the Kit routes, a Kit join after a
rotation and the failures-only limit, run against Task 11's relay branch (`npm run build` there first).

**Task 24 [sonnet] Commands, status and the wire (§6.5)** — the one mixed commit, as plan Task 16, with
§6.5's fields, commands (`sync_keep_device`, `sync_kit_join_decided` included) and keys (all ten locales
for the validation keys).

**Task 25 [sonnet] Hooks and helpers.** Files: `src/hooks/use-device-sync*.ts`, `src/utils/sync-devices.ts`,
`sync-removal.ts`, `sync-state.ts` (+ tests). Tests: grouping (active, check, former; former older than
30 days already absent), `checkReason` (with `lateVouch`) and `joinedVia` keys, `replacesKit` → Kit line
key, the three problems → one rejoin state, `waitingForRemover` → its state, `syncingSince` → "longer than
usual" after 60 s, popover lines.

**Task 26 [sonnet] Desktop device list and removal dialog (§4.3, §6.1, §6.2).** Files:
`SyncDevicesCard.tsx`, new `DeviceRow.tsx`, `RemoveDeviceDialog.tsx`, locales ×10.

**Task 27 [sonnet] Desktop Kit: setup step, done screen, Kit row, check, new Kit (§6.3).** Files:
`SyncSetupDialog.tsx`, `RecoveryKitPanel.tsx`, delete `ShowRecoveryKitFlow.tsx`, new `KitRow.tsx`,
`CheckKitDialog.tsx`, `NewKitDialog.tsx`, print stylesheet, locales ×10.

**Task 28 [sonnet] Desktop pairing-code entry and the holder's finish (§5).** Files: `join/JoinChoice.tsx`,
`join/PairingJoinFlow.tsx`, `AddDeviceDialog.tsx`, `src/mobile/components/sync/PairingSheet.tsx` (holder
finish), locales ×10.

**Task 29 [sonnet] Desktop notices and status (§6.4).** Files: `SyncSection.tsx`, `SyncProblemAlert.tsx`,
`SyncStatusCard.tsx`, `TurnOffSyncDialog.tsx`, `SyncDangerCard.tsx`, the top-bar `SyncStatusSection.tsx`,
an unlock-time removed dialog, new `NoticeList.tsx`, the "Data jsou zpět" card wired to
`sync_kit_join_decided`, locales ×10.

**Task 30 [sonnet] Phone list, device sheet, removal sheet** = plan Task 21 with §6.1–§6.2 ("Je moje"
included).

**Task 31 [sonnet] Phone Kit and notices** = plan Task 22 plus the Kit row, check, new Kit, setup step
(`SyncKitSheet.tsx`, `KitBody.tsx`, `SetupSheet.tsx`), "Data jsou zpět", the removed sheet at unlock,
locales ×10.

**Task 32 [sonnet] Docs.** ADR 0032 (phase 5 with this revision: D1–D4, §0, §2.10, the residuals of §8
verbatim in substance), ADR 0026 (typed codes everywhere, Kit v2, the ticket announcement), ADR 0027 status
line, `decisions/README.md`, the phase-5 spec's As built pointer to this document, `overview.md`,
glossary (§6.4), relay `DESIGN.md` check.

**Task 33 [opus] Live checks and the final review** = plan Task 24, with: two desktops paired **by typed
code**; the phone by QR; remove the phone (no Kit step when the Kit was printed); remove the creator of a
saved Kit (Kit step follows); "Ověřit sadu" with the old and the new page; a Kit join on a fresh profile
after two removals, including the "Data jsou zpět" hold; the removed phone's notice at unlock; the
"longer than usual" line (stop the server mid-round). Languages en, cs, de, ja on desktop and phone; the
phone at **320 pt** width with the largest text size (rows wrap); one pass in dark mode; the relay admin's
Kit line. Then the final review against the plan's Review Focus plus: the Kit seed never in the
database, logs or IPC outside `RecoveryKit`; no UI string contains "klíč synchronizace" or "výměna klíče"
(`grep` in `src/i18n/locales/cs`); `IssuerRemoved` transitive and `LateVouch` covered by tests.

## 8. Residuals for the owner

Each is accepted by the decision named, or needs the owner's yes. **The owner accepted R1–R5 and the copied-Kit trade on 2026-11-16.**

- **R1 — An old copy served to a Kit join (D1).** The Kit cannot check that the server's copy is current.
  A dishonest server (with a removed device, to read the result) can hand someone joining with the Kit a
  stale copy from before a removal. Bites only if the person, at "Data jsou zpět", keeps a device on the
  list that they had removed earlier ("Mám je"); otherwise the joiner's first key change leaves it out
  before anything is sent. Today's design does not have this.
- **R2 — A device added from a backup of a device that left (D3).** A profile backup made while sync was
  on holds that device's signing key. On an honest server a device it adds after the original left
  shows as "Zkontrolujte" and gets nothing; it bites only if the person answers "Je moje" for a device
  they don't own. On a dishonest server it falls under R4.
- **R3 — Where the Kit is kept is what the app saw.** A screenshot, a photo of the screen, a code pasted
  into a synced note or a PDF saved to iCloud from the print dialog is not known to Moony, so removing that
  device does not replace the Kit. Bites if such a copy sits on a stolen device; the remedy is "Vytvořit
  novou sadu".
- **R4 — Slots before a key change (As built 65 (b), O7).** The signed head fixes where a key change
  happened, not what fills the positions before it. A dishonest server working with a removed device can
  give a device that was behind (had not yet downloaded those positions) the removed device's later
  changes as if they were older ones; Moony applies them (and says so afterwards only in part,
  `keyChangeOverrun`). Only signing every change per device (not in phase 5) closes it. Bites only with a
  dishonest server operator plus a stolen removed device plus a device that was offline over the removal.
- **R5 — Waiting for the remover (As built 65 (c)).** A device that was behind waits for the remover's
  signed confirmation before it accepts older changes past its place. If the remover never syncs again
  (lost right after removing), that device stays at "Synchronizace čeká na zařízení {by}"; the way out is
  "Připojit znovu" (turn off, join again; unsent changes stay in the backup).
- **Also accepted, no action:** a copied Kit is cut off only by a new Kit (D1, made visible by `kitUsed`
  and the Kit-join notice); printing from the built app is unverified until Task 33 (the primary button
  may become "Uložit jako soubor…").

## 9. Changes from the security review

Adopted: transitive `IssuerRemoved` and descendants in `added_by_removing` (HIGH 1); a Kit join never
leaves the newest epoch unchecked (MEDIUM 2); the envelope carries the confirmed set (MEDIUM 3);
superseded IDs bounded and under the R2 rule (MEDIUM 4); failures-only per-vault Kit limit; `kitUsed` on
`pinned_at`; the envelope signer checked against the restored store; TransientType on the macOS clipboard
(concealed type and 60 s clear already existed, so not 90 s); 256 retired Kit hashes plus a `401` text
that covers an old Kit; R1 restated as freshness with the push hold; the R2 log-order rule, made
pass-safe for the D3 trap by a ticket announcement in the holder's own chain (a first-seen position alone
would fail the trap whenever the new phone's row reaches the server after the old phone's `left`
record); R4 and R5 stated.

## As built (revision)

Decisions made while building, in the spirit of this document; each supersedes the text above it.
Numbered R-1, R-2, … so they never clash with the phase-5 spec's items 1–69.

**Task 13 (Kit formats).**

R-1. **The v1 code stays beside v2 until Tasks 18 and 19.** Setup still shows a v1 Kit and the
     Kit join still reads one; deleting v1 now would break both before they are rewritten. The v2
     API is separate: `recovery::{encode_kit_code, decode_kit_code, kit_qr_payload,
     parse_kit_qr_payload, read_kit_input, KitCode, KitQr, KitInput, KIT_CODE_LEN}`.
     `read_kit_input(code, relay_url, default_relay)` takes no account (it is in the code). The v2
     reader refuses an 80-character code and a `moony-recover:1:` QR with
     `validation.syncRecoveryCodeInvalid`; an account field of 10^16 or more is refused the same
     way. Tasks 18/19 delete `encode_recovery_code`, `decode_recovery_code`, `recovery_qr_payload`,
     `parse_recovery_qr_payload`, `read_recovery_input` and their types.
R-2. **`identity::kit`:** `KitSecret` (the seed: `generate`, `from_bytes`, `expose`; prints
     `<redacted>`) and `KitSecret::derive(vault_id) -> KitKeys` (`box_secret`, `box_pub`,
     `sign_seed`, `sign_pub`, `token`, `id`, `vault_id`); `kit::kit_id(vault_id, sign_pub)`. The
     box secret is the HKDF output taken as an X25519 scalar (clamped by `x25519-dalek`).
R-3. **A Kit's ID is bound to its key inside the certificate check.** Besides "a kind 3 whose
     issuer is its subject", `verify_signatures` refuses a kind 5 whose device is not
     `kit_id(vault, sign_pub)` and a kind 3 whose issuer is not `kit_id(vault, issuer_sign_pub)`.
     So a Kit certificate cannot claim another device's ID, which retires the phase-5 squat check
     (As built 30, `KitClaimsKnownDevice`). Kinds 3, 4 and 5 share one signature shape (no ticket,
     `issuer_sig` by `issuer_sign_pub`); only genesis is self-issued (`issue_self` refuses every
     other kind). The kind-3 variant keeps its name `CertKind::RecoveryKit`; kind 5 is
     `CertKind::Kit`. Issuers: `cert::issue_kit(creator, creator_device, &KitKeys, issued_at)`,
     `cert::issue_kit_join(&KitKeys, fields, subject)`; `Issuer::Kit { kit: &KitKeys }`.
R-4. **The v1 Kit join until Task 19** signs its kind-3 certificate with a throwaway Kit (a fresh
     `KitSecret`): no other device knows that Kit, so the joiner stays unverified there, as any
     Kit joiner without a pinned Kit would. Task 19 replaces it with the Kit from the code.
R-5. **The envelope** is `rekey::envelope::{build, open, peek, Envelope, EnvelopeInput, Confirmed,
     Header}`, with `bundle::OldKey` for its keys. `open(bytes, &KitKeys)` checks the header's
     vault and Kit ID, opens the body, and verifies the signature with the bundled signer
     certificate (which must verify on its own, name the vault and be `by`'s); the store check of
     §2.6 step 6 is the Kit join's. Keys must be at least one, strictly ascending, the last the
     header's epoch; confirmed items name each device once (lowercase hex, 64-hex hashes). Every
     refusal is one internal error ("a Kit envelope is not valid"), which Task 19 maps to
     `validation.syncRecoveryKitUnreadable`. The envelope does not judge heads: the sealer passes
     only verified ones (`head::verified_epochs`, Task 17).

**Task 14 (trust store revision, migration 041).**

R-6. **A device that a `LateVouch` entry vouches for is `LateVouch` too** (an extension of §3.1,
     which makes only `IssuerRemoved` transitive). Without it a backdated certificate from a left
     device's copy (excluded) could pair a second device, whose issuer is then `trusted`, and that
     second device would get the next key: the R2 mitigation would be laundered one step down. The
     fixpoint (`excluded_by_issuer`) carries each reason down its own chain; `IssuerRemoved` wins
     where both apply. The UI text of a `LateVouch` descendant ("{by} no longer syncs") does not fit
     it exactly; Tasks 24/26 may want a third `checkReason` or the issuer-removed wording.
R-7. **The chain runs through entries of every state.** A device in an `IssuerRemoved` chain that
     has since left still passes the reason on to what it paired (it is reported as `Left` itself).
     A `confirmed` entry and this device are never in the set and stop the chain; so does an entry
     whose issuer is not pinned here (trusted on first use by a Kit joiner).
R-8. **Positions:** a vouch is late only when both positions are known and the vouch's is greater
     than the issuer's end; an unknown position on either side counts as in time (that side came
     by a vetted path: an export, a bundle, this device, or before migration 041). The vouch's
     position is the earlier of its certificate's first sighting and the announcement of its
     ticket by its own issuer (an announcement by another device does not count). The end is the
     earlier of the issuer's `left` record's first sighting (when it left) and its earliest pinned
     re-join successor's certificate's first sighting. `trust::{note_seen, note_ticket, SeenWhat}`
     write them (first one wins); Task 15 calls them.
R-9. **`speaks_at` takes the store** (`trust::Store`, the entries and each superseded ID's earliest
     successor `issued_at`) and bounds a superseded ID whatever its state, records included (a
     removal it signs after its successor does not count).
R-10. **At most one trusted Kit:** in `refresh`/`tofu_from_rows` a kind 5 is pinned only while no
     other Kit is `trusted`, at most one per pass (lower ID first), else `ConflictKind::SecondKit`
     (reported, not stored: a later pass after the first Kit's removal pins it). Records apply
     before certificates, so the removal of the old Kit and the new Kit arriving together pin the
     new one. `import_bundle(conn, signer, recipients, kit: Option<BundleKit {cert, replaces}>,
     removed, to_epoch, now)` applies `replaces` with the removals and then pins and confirms the
     Kit; a Kit that is not kind 5, or a second Kit once the removals are applied, fails the whole
     import. `ConflictKind::KitClaimsKnownDevice` is gone (R-3).
R-11. **`recipients(conn, removing) -> Recipients {included, excluded, kit}`**: the Kit (the
     `trusted` kind-5 entry) only in `kit`; a Kit's row that is not pinned (`platform =
     'recoveryKit'`) in no list. A Kit entry takes part in the issuer chain like any other: a Kit
     whose creator was removed and that no key change confirmed passes `IssuerRemoved` to its
     joiners (Task 17's commit and every bundle confirm the Kit, so a genuine one never does).
R-12. **`keep(conn, device) -> bool`** refuses this device and an entry not pinned or not
     `trusted`; `mark_confirmed(conn, devices)` (the remover's commit; passes over entries no
     longer `trusted`); `apply_confirmed_set(conn, &[(device, cert hash)]) -> usize`. Confirmation
     records (type 3) are no longer read by `refresh`, and `record_counts` answers `false` for one.
R-13. **New-device notices** (`trust::NEW_DEVICES_KEY` = `sync.new_devices`, `trust::new_devices`,
     `NewDevice {device, by, at, viaKit}`; `at` is this device's clock when it pinned the entry,
     never the certificate's own date; newest 20 kept) are written by `refresh` only (not by an
     import, a bundle or a Kit joiner's first store). Turning sync off and a join's replace clear
     the key and the two new tables (`state::disable`, `join::replace_profile`: minimal additions
     to files of later tasks).
R-14. **Minimal changes to `rotate.rs` (Task 17's file) so it compiles:** `plan` and `Removal` lose
     `opted_in`; `Plan::confirming` is every recipient but this device, and the commit calls
     `trust::mark_confirmed` for them instead of `trust::confirm`. `pickup.rs` passes no Kit to
     `import_bundle` and drops the `kit_unconfirmed` check. The `rekey_rotate` test of the Kit tick
     became "a key change confirms every recipient".

**Task 15 (first-seen positions, ticket announcements).**

R-15. **The announcement** is `Changeset::tickets` (`"tickets"`: an array of base64url ticket
     public keys, at most 16, left out when empty; a reader skips fields it does not know, so a
     build from before the revision applies the envelope as an empty change).
     `pairing::holder::announced_payload(conn, now)` builds the slot-2 payload and, when it carries
     an identity, queues the announcement (`announce_ticket`: `journal::build_marker` +
     `push::seal_into_unsent`, one savepoint) in the same `engine::exclusive` hold `confirm`
     already takes. It is queued before the person's typed code is checked: a wrong code leaves an
     announced ticket nobody can use (its secret is wiped with the payload), which is harmless. A
     failure to queue fails the confirm (the pairing is not sealed).
R-16. **Where positions are noted:** `engine::pull::note_trust_positions(conn, rows, position)` for
     every `sync_devices` row carrying a non-NULL `cert` or `left_record` value, called by `act`
     after a changeset applied (`Outcome::Applied`, the op's relay position) and by the snapshot
     restore (`settle_heads`, both replace and merge, the snapshot's `seq`); an applied changeset's
     `tickets` go to `trust::note_ticket` with the changeset's author as issuer. A value is noted
     whether or not the merge kept it (a later, losing value of a write-once column still counts as
     a sighting of that column): only the first position of each kind matters, and before the
     issuer's `left` record its key could vouch anyway (O9). Held, quarantined, skipped and scanned
     envelopes note nothing.
R-17. The D3 trap is exercised at the trust level in `engine/tests/trust_order.rs` (an announced
     pairing whose certificate reaches the server after its holder's `left` record stays a
     recipient; an unannounced one is `LateVouch`). The refresh after a pull is still called by
     hand there: the pull's refresh hook is Task 20's (built, R-49).

**Task 16 (relay client and fake: the Kit).**

R-18. **The calls** (`relay/mod.rs`, `Relay`; all three are implemented by `HttpRelay`, `FakeRelay`
     and every forwarding test relay): `put_kit(vault: &VaultAuth, kit_token: &str, envelope: &[u8])
     -> bool` (`true` = created, `false` = a retried setup replaced the envelope);
     `get_kit(vault_id: &str, kit_token: &str) -> KitRead {envelope, since, updated_at}` (the Kit
     token is the only credential, so it takes the vault ID, not a `VaultAuth`);
     `rotate_token_with_kit(vault, new_token, bundle, KitRotation {envelope, token: Option<&str>})`.
     **`rotate_token` keeps its signature** (a rotation without a Kit part, which a vault with a
     Kit refuses with `BadRequest("kit_required")`); Task 17 calls the new method always. Also new:
     `VaultInfo.kit: Option<KitInfo {since, last_read: Option<i64>}>` (a server without the field
     reads as `None`), `ServerInfo.kit: bool` (the `"kit"` feature), `MAX_KIT_ENVELOPE` (256 KiB).
R-19. **Errors:** `RelayError::KitExists` (`409 kit_exists`) and `RelayError::KitReplaced
     { replaced_at }` (`410 kit_replaced`; a body without a non-negative `replacedAt` is
     `Protocol`; a `429` stays `RateLimited`), both `SyncError::Refused`. `kit_required`,
     `invalid_kit`, `invalid_kit_token` are `BadRequest(code)` and `kit_too_large`
     `BadRequest("kit_too_large")`, like `invalid_rekey` / `rekey_too_large`. The client refuses
     bad Kit arguments (token not 43 base64url characters, envelope empty or over the cap) before
     sending, as for the rekey bundle.
R-20. **The fake's limit** is the server's failures-only rule per vault only (the fake has no
     clients): the 31st failed `get_kit` (`Unauthorized` case) inside a fake-clock hour answers
     `RateLimited`; the right Kit token and a replaced one (`KitReplaced`) are never counted or
     limited. The per-client limits are not modelled (tests use `fail_next(Route::GetKit,
     RateLimited)`). The fake's routes: `Route::PutKit`, `Route::GetKit`; a rotation with or
     without a Kit is `Route::RotateToken`. The fake checks in the server's order (DESIGN.md "As
     built: Recovery Kit", item 5) and a refused rotation changes nothing; a lost answer
     (`lose_next_answer(RotateToken)`) keeps the whole rotation including the Kit.
R-21. **Tests of this task** live in new files beside the existing ones, as the blob and mailbox
     tests do: `relay/http/kit_tests.rs`, `relay/fake/kit_tests.rs`, and the error mapping in
     `relay/mod.rs`'s tests; not in `http/tests.rs` / `fake/tests.rs`.

**Fixes from the review of Tasks 13–15.**

R-22. **A device left out for its issuer speaks for nothing** (finding 1). As built 50 had dropped
     the `kit_unconfirmed` clause with no replacement, so a `LateVouch` or `IssuerRemoved` entry (still
     `trusted`) could make a key change every device took (its signer then `confirmed` everywhere) or
     sign removals that counted. Now the trust store computes the issuer exclusions with every load
     (`Store::excluded`); `trust::speaker(conn, cert)` (pinned with exactly that certificate,
     `trusted`, not excluded) is the pickup's signer check; `import_bundle` refuses, before it pins or
     confirms anything, a signer that is not such a speaker (also one not pinned here: the pickup never
     passes one); `removal_counts` (and so `refresh`, `mark_removed`, `record_counts`, a bundle's
     removals) ignores a record by an excluded signer. A `confirmed` entry is never excluded, so "Je
     moje" or a key change naming it gives it its voice back (a removal it signed then counts at the
     next `refresh`). **`vouch` is unchanged:** an excluded issuer's pairings are still pinned, so they
     show as "Zkontrolujte" and can be kept; the transitive exclusion leaves them out and silent too.
     Refusing to pin them would hide them from the person and gain nothing. The one exception is a Kit
     (R-24).
R-23. **Restored positions, a pairing joiner, ghost issuers** (finding 2). (a) **Equal positions are
     late** (`vouch >= end`; the two tests that asserted "in time" were rewritten). Only one restored
     snapshot gives a certificate and its issuer's end the same position, and it cannot tell their
     order; the lead's decision ("a restored entry whose issuer is gone counts as late") is implemented
     as this, which is the same where it matters and narrower in one case that is sound: an entry
     restored at the snapshot's position whose issuer's `left` record was seen *later* (by an op after
     the snapshot) is in time, because the log order is then known. The successor whose position is
     its superseded issuer's end is not judged against itself (`end_of` leaves the judged entry's own
     sighting out). Cost (accepted by the lead): on a device that gets both from one snapshot, a
     genuine device paired by one that has since left is "Zkontrolujte" until a key change names it
     or the person keeps it. (b) **A pairing export carries the holder's positions** (`ExportEntry`
     `certSeen`, `leftSeen`, `ticketSeen`, each left out when unknown; the announcement only when its
     issuer made it); `import_export` replaces the joiner's own positions for every exported device
     with them (the join's snapshot restore has already noted its own position for everything), so the
     joiner's verdicts are its holder's, unknown positions included. Positions rather than verdicts:
     they are relay positions, the same on every device, and the verdicts follow from them and the
     exported states; verdicts would go stale. The export also refuses more than one `trusted` Kit.
     (c) **Ghost issuers:** an unconfirmed entry whose issuer is not pinned is left out
     (`ExclusionReason::IssuerUnknown`, new, transitive like `LateVouch`, so R-7's "an unpinned issuer
     stops the chain" is withdrawn). `tofu_from_rows` still pins such certificates (trust on first use),
     now excluded: the envelope's confirmed set (§2.6) or "Je moje" lets them in. Tasks 24/26 map the
     new reason (wording as for `LateVouch`).
R-24. **The Kit is never chosen by ID** (finding 4). `refresh`/`tofu_from_rows` pin a kind 5 only as
     the vault's first Kit: no Kit entry of any state is pinned here, it is the only Kit certificate
     the pass would pin, and its creator is not excluded (R-22); otherwise every candidate is
     `SecondKit` (two first Kits in one pass: neither). A replacement Kit is pinned only by
     `import_bundle` from the bundle that names it (or, Task 17, by the device that creates it), so a
     rogue member's Kit posted ahead of a replacement never wins, whichever order the old Kit's removal
     and the bundle arrive in. `Recipients.kit` (`sealing_kit`) is a `trusted` Kit not excluded for its
     creator, the `confirmed` one when there is one, and none when that leaves more than one (logged).
     R-10's "a later pass after the first Kit's removal pins it" and "lower ID first" are withdrawn.
R-25. **A superseded ID's bound is this device's `pinned_at` of its first successor** (finding 3),
     not the successor's `issued_at`, which whoever made the certificate chose: a copy of a live device
     posting a back-dated re-join used to void everything the live device had signed since that date,
     here and inside later bundles. `speaks_at` uses the earliest `pinned_at` of the pinned re-join
     successors (positions were the other option, but `speaks_at` compares a signer's own times, and
     `pinned_at` is a time); `end_of` keeps using the successor's first-seen position. A second re-join
     of an ID already superseded is a separate row and entry (the merge keeps write-once `cert`s per
     row) and, pinned later, cannot move the bound earlier. O11's quiet check (`SupersededQuiet`) counts
     writes since the earlier of the first successor's `pinned_at` and `issued_at`: a back-dated
     re-join makes the live device count as writing (it stays a recipient), and a future-dated one can
     no longer make it look quiet.
R-26. **A Kit's row is a row whose certificate is a Kit's** (minor 10): `recipients` skips an unpinned
     row only when its `cert` is a kind 5 that verifies in this vault and names that row
     (`trust::is_kit_row`), whatever its `platform`; a row that only calls itself `recoveryKit` is listed
     as `Unverified`/`NoKey`. Task 24's `devices::list` filter must use `is_kit_row` too, not the
     platform.
R-27. **Only a value the row holds and that verifies is a sighting** (minor 6): `trust::note_sighting`
     (called by `note_trust_positions`) notes a `cert` that the row now holds and that verifies for the
     row in this vault, and a `left_record` the row holds that its subject's pinned key (else the row's
     own certificate's key) verifies. The snapshot restore notes after the rows are in (replace and
     merge), no longer in `settle_heads`. R-16's "noted whether or not the merge kept it" is withdrawn.
R-28. **Notes for later tasks, not built here.**
     - *Task 17:* the bundle v2 `kit` must reach `import_bundle` (today pickup passes `None`, so a
       bundle never confirms or pins a Kit, review minor 12; with R-24 a replacement Kit is then pinned
       nowhere until Task 17 lands: test it — a receiver of a new-Kit bundle seals its next change to
       the new Kit). The device that makes a new Kit must pin it itself at commit (a `refresh` will not,
       R-24) and mark it `confirmed`. Its own signer check is `trust::speaker` as in pickup.
     - *Task 19:* the Kit joiner trusts the envelope's confirmed set only from a signer that is not
       `removed` or `left` in the restored store, ignoring the envelope's own `at` (review minor 9: `at`
       is the signer's claim). The R-4 throwaway Kit is deleted then; the sync flag stays off until it
       is (minor 11). **Genesis on first use:** `tofu_from_rows` pins every genesis certificate in the
       restored rows, and an unconfirmed genesis is never left out for its issuer, so a genesis row a
       vault-key holder wrote (honest devices never pin one through sync) is a recipient on a Kit
       joiner. Task 19 should pin genesis certificates on first use only when the anchored Kit's chain
       or the confirmed set names them, or exclude unconfirmed ones there.
     - *Tasks 24/26:* "Je moje" (`trust::keep`) should run `trust::refresh` after it, so records the
       kept device signed apply at once; map `IssuerUnknown`.
     - *Task 27:* consider ticking "Odebrat také" by default for a removed device's descendants
       (review minor 9).
     - *Not fixed (minor 13):* `StaticSecret::from(*eph)` / `from(*expand(..))` copy a secret into a
       by-value temporary; `x25519-dalek` 3 has no constructor from a reference, so there is no trivial
       fix (bundle.rs has the same pattern from Tasks 1–10).
     - *Observation for the lead:* a vault-key holder (e.g. a copy of a device that left, before the
       next key change) can upload a snapshot that omits a `left` record; a device that later joins or
       catches up from it sees that device as `trusted`. Positions cannot catch an omission; this is
       snapshot integrity, outside these findings.

**Task 17 (removal with the Kit, always rotating).**

R-29. **Bundle v2** (`rekey::bundle`): version byte 0x02, JSON `v: 2`; `kit` is the JSON's last
     field and is left out when the vault has no Kit (`BundleInput.kit` / `BundleBody.kit`:
     `KitPart {cert, replaces: Option<Record>}`, the replaced Kit's ID being the record's subject).
     `build` and `open` refuse a Kit certificate that is not a kind 5 of the vault, a `replaces`
     that is not a removal, names another device than it is listed under, or removes the new Kit
     itself. The frozen vector changed; a second vector covers a bundle with a Kit and `replaces`.
R-30. **The removal API** (`rekey::rotate`): `plan(conn, removing, also_remove, new_kit)` removes
     the union of `removing` and the ticked `also_remove` (the consent is over the union, so the
     command may pass the ticks either way); `Removal {removing, new_kit, consent}` takes the
     union. `Plan {epoch, removing, recipients, excluded, added_by_removing, kit, replaces_kit,
     consent}`; `Plan::confirming` is gone (the commit confirms every recipient, this device
     included, and the Kit). `ReplacesKit::{No, CreatorRemoved, Cloud, Asked}` (the wire's
     `replacesKit`): asked wins; a Kit `copied`, `file` or `cloud` whose creator is removed is
     `CreatorRemoved`; else `cloud` with any device removed is `Cloud`; `paper` and NULL never.
     **Nobody removed and no new Kit is refused** (`syncRemovalPlanChanged`: D2 has no "change the
     key"), and so is removing a Kit's ID. `added_by_removing` descends from the devices asked for
     (and the replaced Kit), through entries of every state, lists only `trusted` ones and never
     this device; a ticked device stays listed, so its tick box does not vanish.
R-31. **Without a Kit there is no Kit part.** A vault with no `trusted` Kit here (`Recipients.kit`
     `None`) and no new Kit asked rotates with `rotate_token` (no `kit`), the only case that still
     does; every other change calls `rotate_token_with_kit`. A vault whose server holds a Kit this
     device has not pinned then gets `400 kit_required` (the change is wiped, an internal error):
     the way out is "Vytvořit sadu", whose token replaces the server's Kit. Honest vaults made by
     Task 18 always have one.
R-32. **The Kit at the commit:** a replaced Kit's removal record is written into its row and
     applied like the device removals (cut off at the new epoch); the new Kit's row is written by
     `rekey::kit::write_row` (`kit_storage` NULL until the person saves it: "neuložená") and pinned
     `trusted` + `confirmed` by `trust::pin_created_kit` (new; R-28), which refuses a Kit not made
     by this device or a second trusted Kit. `Outcome::{Changed {epoch, removed, kit}, Pending
     {kit}, …}` carry `rekey::kit::NewKit {secret, id, created_at}` (the seed, for the command to
     show once; `Debug` without it; `Outcome` is no longer `Clone`). A resolution in the background
     returns `kit: None`. `RemovalNotDone.kitReplaced` and `KeyChangeNotice.kitReplaced` (the
     bundle's Kit has `replaces`) are new, both `false` when absent.
R-33. **The envelope's confirmed set** is computed at `prepare` as what the store will hold after
     the commit: every `trusted` entry `confirmed` now, minus the removed devices and the replaced
     Kit, plus every recipient (this device included) and the Kit sealed to; the commit confirms
     exactly the recipients and that Kit (`rekey::kit::confirmed_after`). Its keys are every epoch
     with the new key last; heads only from `head::verified_epochs` plus epoch 0's (it starts at
     0, as in the bundle), never the newest's. The pending JSON is version 2 with an optional
     `kit` (`envelope`, and `token` with `cert` for a new Kit, `replaces`); `resolve_pending` sends
     it unchanged. Notices never go to a Kit entry.
R-34. **`rekey::kit`** (new) is shared with setup (Task 18): `KitStorage` (`paper` < `copied` <
     `file` < `cloud`), `NewKit`, `create`, `storage`, `write_row`, `confirmed_after`, `seal`
     (the vault comes from the Kit's certificate).
R-35. **The list-only test of review finding 3** (a key change signed by a device whose removal
     record this device already holds) now writes that record by hand
     (`rekey_fixture::record_removal`): with D2 the case arises only from a server that holds a
     key change back. Not built here: the Local `sync.key_change_unverified = {by}` of §7.1 (the
     status's `rejoinBy`); Task 24 writes it with the status field that reads it.

**Task 18 (setup and the Kit lifecycle).**

R-36. **The draft** is `setup::KitDraft {secret, vault_id, created_at, storage}` (no `Debug` of
     the seed), made by `setup::prepare_kit(conn, open_draft, now)` (refused with sync on:
     `syncAlreadyEnabled`; an open draft of the same vault is shown again, any other gets a new
     Kit) and shown by `setup::kit_view` (v2 code in 19 groups, v2 QR; the wire's `accountNumber`
     is still filled until Task 24 drops it). `KitDraft::from_new_kit` takes a key change's
     `NewKit` (for Task 24's removal commands). The command keeps the draft in `KIT_DRAFT` beside
     the copy window, both ten minutes now (`KIT_COPY_WINDOW`, was five), wiped wherever the
     window was (`forget_kit_window`: lock, restore, account deletion, sync off, synced data
     deleted) and after `sync_finish_setup` succeeds; a failed finish puts it back for the retry.
     `with_kit_draft` is the one gate of copy, print and save (`validation.syncPasswordRequired`
     without an open draft). The Kit's certificate is dated at the draft's `created_at`, so a
     retry signs the same bytes.
R-37. **Registration at `finish_setup`:** after sync is on and this device is pinned,
     `register_kit` puts the Kit (`PUT /kit`, the envelope of epoch 0 with this device and the Kit
     as its confirmed set) and only then, in one savepoint, writes the Kit's row with the draft's
     storage, pins it (`trust::pin_created_kit`) and confirms this device and the Kit. A Kit
     already pinned here is not put again (a retry after registration; a copy that re-joined
     keeps it). **`409 kit_exists`** (the server holds another Kit of this vault: an earlier try
     registered a draft that expired before its answer came back) leaves the server as it is and
     registers nothing here: the device has "no Kit" and the server's Kit can only be replaced by
     "Vytvořit sadu" (R-31). A server whose `/v1/server` lacks `kit` (or that has no such route)
     is refused before the vault is created (`validation.syncServerNeedsUpdate`).
R-38. **Storage:** `setup::note_kit_storage(conn, draft, storage)` keeps the strongest value in the
     draft and in the Kit's row when the row exists (a Kit a key change made); copy notes `copied`
     whether the copy was native or the UI's, save `file` or `cloud` (`looks_cloud_synced`),
     `sync_print_recovery_kit` (new, `WebviewWindow::print`) `paper` once the print dialog opened.
     `sync_finish_setup` lost `kitSaved`; an absent draft or one without storage is
     `syncRecoveryKitNotConfirmed`. `sync_show_recovery_kit` and `setup::recovery_kit` are
     deleted. **Frontend not touched** (Task 24/27): `tauri-api.ts` still sends `kitSaved`
     (ignored) and still has `showRecoveryKit` (its command is gone), and the setup UI must call
     copy, save or print before "Sadu mám, pokračovat"; until Task 27 the old setup flow fails
     at finish unless the code was copied or the Kit saved.
R-39. **"Ověřit sadu"** is `setup::check_kit(conn, code_or_qr) -> KitCheck::{Valid, Replaced {at,
     current_created_at}, OtherSync, Unknown}` (`at` = the old Kit's `state_at` here,
     `current_created_at` = the trusted Kit's certificate date); a code that does not read is
     `syncRecoveryCodeInvalid`; no network. The command and the wire type are Task 24's.
R-40. **`kitUsed`** is `engine::status::kit_used(conn, last_read, now)` with
     `Runtime::kit_last_read` (from the round's vault info) and `dismiss_kit_used` ("Vím o tom",
     the Local `sync.kit_read_seen`, cleared by turning sync off and a join's replace); a read is
     news more than an hour after it, and a kind-3 entry with `pinned_at` at or after `read − 1 h`
     explains it. The status field is Task 24's. The macOS clipboard already carried
     `org.nspasteboard.TransientType` (keychain plugin, phase 2): nothing changed there.
R-41. **The v1 code functions stay until Task 19** (R-1 put their deletion in Tasks 18/19): the
     Kit join still reads a v1 Kit until Task 19 rewrites it; setup no longer uses them.

**Task 19 (joining with the Kit).**

R-42. **`join::recovery_credentials(relay, db, KitInput) -> (Credentials, KitJoin)`**: the account
     from the code (`syncRecoveryAccountInvalid`), `GET /kit` with the derived token, the envelope
     opened (`rekey::envelope::open`), the credentials with every key it carries (the newest
     current), `vault_info` with that key's token, and the head and rotation of the newest key
     change's bundle when the server still lists it (`KitJoin.server_head`; read from the bundle's
     unauthenticated header, which only makes the head provisional). `get_kit`'s refusals are
     read on the `RelayError` (`kit_read_refused`), never through `SyncError::Unauthorized`:
     `401` and `404` → `syncRecoveryVaultNotFound` (reworded per §2.9: one text for a wrong code,
     a vault without a Kit, an old Kit, another sync or server), `410` →
     `syncRecoveryKitReplaced|{"at":…}`, a `400` (a token or vault ID the server's rules refuse:
     only a code that does not read gives one) → `syncRecoveryCodeInvalid`, `429` stays
     `RateLimited`, `402` the subscription's reading. Every envelope failure, an envelope whose
     newest key's token the vault does not know (`401` at `vault_info`), a missing, unverifiable
     or replaced Kit row, an envelope signer not pinned with the bundled certificate or removed or
     left in the restored store, and a head of the envelope the snapshot contradicts are
     `syncRecoveryKitUnreadable`, with nothing replaced (the replace's savepoint rolls back).
     `syncRecoveryKitRotated` is gone from Rust and the locales; `syncRecoveryCodeInvalid` says
     95 characters now. The command `sync_join_with_recovery_kit` reads a v2 code
     (`read_kit_input`) and ignores its `account` argument until Task 24 drops it from the wire.
     The UI shows `{{at, unixdate}}` with a new i18next formatter (`src/i18n/index.ts`: unix
     seconds through `getFormatters().date`, long style).
R-43. **The store** (in the replace's savepoint, after the restore): `trust::pin_kit_anchor`
     pins the restored Kit row's kind 5 (`trusted`, `confirmed`) when its `sign_pub` is the
     derived one and no removal record on the row verifies against its signer's row certificate;
     then this device's kind 3 (`Issuer::Kit`), its row, the envelope's verified heads
     (`record_joined_epochs`), and `trust::tofu_from_rows(conn, now, genesis)`, which now takes
     the set of genesis certificates it may trust (`trust::kit_join_genesis`: the genesis at the
     top of the anchored Kit's issuer chain in the rows, and any the confirmed set names with that
     certificate's hash; any other genesis is never pinned, and what it vouched for waits for it,
     closing the genesis gap of R-28). The signer check (R-28: pinned with exactly the envelope's
     certificate and `trusted`, the envelope's `at` not consulted) comes after the TOFU, then
     `trust::apply_confirmed_set`.
R-44. **The newest epoch E** (§2.6 step 5): known from the snapshot's `Keys` record → nothing
     more; a snapshot sealed under E (its stream header's epoch, `fetched_snapshot_at`) → the Local
     `sync.kit_epoch_floor = {epoch, after}` (`state::KitEpochFloor`), which
     `engine::pull::epoch_fits_position` applies as a head would (an op past `after` under an
     older epoch is `Invalid`); an older snapshot → `head::note_unverified` with the server's head
     and rotation when it listed the bundle (also recorded provisionally in `sync_epochs`, as a
     pickup does), else with neither: `UnverifiedEpoch.head` and `.rotation` are `Option` now,
     the look-ahead starts at `after`, and `settle` takes the signed record's head and rotation
     (recorded in `sync_epochs`; a different recorded head refutes). `head::verify_heads` takes
     `now` for that. The floor and the hold are cleared by turning sync off and a join's replace.
R-45. **The push hold** (§2.6 step 7) is the Local `sync.after_kit_join` (`join::AfterKitJoin
     {devices, newestChangeAt}`: every other `trusted` non-Kit entry and the newest change time
     among the other devices' clocks; not written when there is none). `engine::round_pass`
     returns before the reseal, the blob upload, the push, the snapshot and the daily pass while
     it exists (pulls go on). `join::after_kit_join` reads it (an unreadable value still holds),
     `join::lift_kit_join_hold` lifts it; the command (`sync_kit_join_decided`) and the status
     field are Task 24's. **Not held:** the runtime's blob transfer lanes (uploads of documents
     the person adds meanwhile) — Task 24 should gate the upload lane on the same key.
R-46. **A Kit joiner's removal before its row went out:** a device it removes while the hold
     stands never saw the joiner (its row waited behind the hold), so it cannot verify the
     removal notice and shows `keyChangeUnverified` rather than "removed" (it gets no key either
     way: "Odebrat je" still leaves it out). Accepted; Task 22 may want a scenario for it.
R-47. **v1 is gone:** `recovery::{encode_recovery_code, decode_recovery_code,
     recovery_qr_payload, parse_recovery_qr_payload, read_recovery_input, RecoveryKey,
     RecoveryQr, RecoveryInput, RECOVERY_CODE_LEN}` and their tests are deleted (R-1, R-41); the
     v2 reader's v1-refusal test keeps a frozen v1 string. The v1 Kit-join tests of
     `engine/tests/join_new.rs` are replaced by `engine/tests/kit_join.rs`; the e2e Kit join
     (`relay_e2e_pairing.rs`) types a v2 code of the Kit its setup registered
     (`relay_e2e::first_device_with_kit`) and lifts the hold before converging (Task 23 runs it).

**Task 20 (leaving and records about this device).**

R-48. **Leaving** (`setup`): `write_left_record(conn, now)` signs this device's `left` record (its own
     key, the current epoch) and writes it into its own row, which the capture queues; `false` when
     sync is off, there is no signing key or row, or the write-once column holds one already.
     `leave_best_effort(relay, db, budget)` takes the round lock, writes it and runs one
     `push_all` (no pull), all inside `LEAVE_BUDGET` (5 s, waiting for a round counted); every failure
     and the timeout are logged and ignored, and a profile with sync off sends nothing, so a device
     another one removed never signs one. The push sends envelopes in sequence order, so a holder's
     ticket announcement (queued at the confirm) always goes before its `left` record
     (`trust_order.rs` now leaves through the real function). `commands::sync::say_left(db)` (flag on,
     sync on, a valid relay address) is called by `sync_turn_off` inside its `guarded_change`, only with
     sync on and the password checked, and by `delete_account` when "delete the synced data" is off.
R-49. **The refresh hook** is `rekey::refresh_trust(conn, data_dir, now) -> bool`: `trust::refresh`, then
     a verified record naming this device (`RefreshReport::about_me`, its first one) ends sync through
     `removed_here(conn, data_dir, record.signer, record.at)` and the function returns `true`. A refresh
     that fails is logged and counts as nothing new (a store that is behind leaves devices out, never
     lets one in); a locked profile is the error. Called (1) in the pull, right after an applied
     changeset that touched `sync_devices` (`PullReport::devices_changed`, per op, so a pull that fails
     later still refreshed; a hit sets `PullReport::removed_here`, ends the pull and `round_pass`
     returns `Pass::Done` with `RoundReport::removed_here`); (2) after a snapshot restore in
     `round_pass`; (3) after a key pickup adopted a key (`round_steps`); (4) at the end of the engine's
     `start()` (unlock), with the data folder (the binding file goes with the turn-off). A `left` record
     another holder of this device's key made turns sync off the same way; the notice's `by` is then this
     device's own name (the record's signer), and its `left` flag says so (R-68). *Corrected by R-69:* a
     refresh that fails is not simply "nothing new"; it is owed and run again at every round's start.
R-50. **Not done:** `sync_pairing`'s joiner and the Kit join call `trust::refresh` by hand as before (their
     savepoints are not rounds). The notice of a removal that arrives by record has `unsent` counting the
     outbox and unsent envelopes at that moment, as a removal notice does.

**Task 21 (documents and photos across a key change).**

R-51. **What already held, now tested** (`engine/tests/blobs.rs`, module `key_change`): an upload the
     retired token meets (`TokenReadOnly`, `Outcome::Blocked`) records nothing (attempts stay 0, the
     entry is inactive) and the run stops at it (one `PUT` for two blobs); after the pickup the content
     is sealed again from the local plaintext under epoch 1 (an upload is sealed as it streams) and a
     device at epoch 1 opens it; content under an epoch a device lacks reads as a local failure ("on
     its way", `SinkOutcome::UnknownEpoch`) and opens after the pickup; a blob sealed under epoch 0
     opens at epoch 1; the retired token cannot delete a blob (the fake authorises `DELETE` as owner)
     and a removed device stops at its removal. A token that rotates while the body streams is the
     same path (`classify` maps the answer, whenever it comes, to `Blocked`; `blobs/tests/transfer.rs`
     has that mapping), and the fake checks the token before the body, so it has no separate test.
R-52. **The one code change:** `engine::daily_pass` does not run while the round's own look at the vault
     (`RoundReport::vault`) shows a rotation above `rekey_seen`: the pickup follows the pass
     (`Trigger::AfterPass`), the token in hand is retired, and the pass would only fail its deletes
     and count as a started pass (`PASS_RETRY_AFTER`). It runs in the next round with the new token.
R-53. **Not done, for Task 24:** the runtime's transfer lanes still read the credentials of the moment
     and run uploads while only the round knows of a pending change (the lane stops on the first
     refusal, R-51); the `after_kit_join` hold (R-45) is not applied to the upload lane either.

**Fixes from the review of Tasks 17–19.**

R-54. **Where the Kit is kept only grows through sync** (Task 17 review I1). A device about to be removed
     (the creator of a copied Kit, say) could sync `kit_storage = 'paper'` or NULL with a newer clock, so
     its removal kept the Kit it still held. `merge::columns` now merges `sync_devices.kit_storage` by
     strength, `(rank, clock)` lexicographically: NULL and unknown values 0, then `paper` < `copied` <
     `file` < `cloud`; the clock decides only between equal strengths (so every order converges). Local
     writes already only went up (`setup::note_row_storage`; `rekey::kit::write_row` inserts only). A
     device can still raise the value (up to `cloud`, which replaces the Kit at any removal): that errs on
     the safe side. Spec §2.7 and `docs/architecture/database.md` say "strongest wins" instead of LWW.
R-55. **A new Kit retires every other `trusted` Kit** (Task 17 review I2 (a)). `Plan.retired_kits` holds
     the `trusted` Kits other than the one sealed to (silenced for their creator, or a second one) when
     the Kit is replaced, and the consent covers them (domain `moony removal consent v3`). `prepare`
     signs their removals; they travel in the bundle's `removed` list (no new bundle bytes: the v2 `kit`
     part keeps one `replaces`), in the pending JSON as `kit.retired` (optional), and the commit applies
     them like the replaced Kit's, so `trust::pin_created_kit` no longer meets a second Kit. Receivers
     apply them as any removal; the key-change notice names no Kit as a removed device and says
     `kitReplaced` instead. `kit::confirmed_after` takes a list of replaced Kits.
R-56. **A key change the server took that cannot complete here is given up** (Task 17 review I2 (b)).
     `rotate::commit_taken`: a locked profile keeps the change pending; a database error does too, up to
     three in a row (Local `sync.key_change_commit_failures`); any other commit failure (a refusal of the
     trust store, an epoch that no longer follows) gives up at once. So does `resolve_pending` when the
     new token reads the vault but the server no longer lists its bundle (bundles go after 30 days).
     Giving up (`give_up`, one savepoint) wipes `rotation_pending` (the new key and a new Kit's token) and
     writes the Local `sync.key_change_stuck` (unix time); `Outcome::Stuck` comes back. While it stands,
     every round stops at its start with `keyChangeUnverified` (the existing "Toto zařízení je potřeba
     připojit znovu" state, whose way out, turning sync off and joining again, is the one wanted: no new
     problem or text), `rotate::plan` refuses (`syncKeyRotated`) and the upload lane rests (R-65).
     Turning sync off and a join's replace clear it. `remove()` answers a give-up with an internal error
     (the status then shows the rejoin card). The other devices take the change as usual, a new Kit
     included: a Kit shown with `Outcome::Pending` that then gives up **does** work (the server holds its
     envelope), although this device never pins it.
R-57. **No Kit to seal to** (Task 17 review M1). `ReplacesKit::Missing` (wire `'missing'`, Task 24): this
     device has no Kit it can seal to (`Recipients.kit` is `None`) but the server holds one for the vault
     (Local `sync.server_kit`, noted from every round's vault info, from `remove()`'s own look, and by
     setup) or a `trusted` Kit is here that cannot be sealed to: the removal then makes a new Kit (its
     token replaces the server's), retiring every `trusted` one (R-55); without it the server answered
     `400 kit_required` (R-31). `Plan.kit_not_updated` is `true` only when no Kit is involved at all (none
     here, none known on the server, none asked for): the UI says "Tímto odebráním se záchranná sada
     neaktualizuje." / "This removal doesn't update the recovery kit.". **Texts for Task 26** (plan
     convention: the UI task adds them in ten locales): that line, and the Kit line for `missing`:
     "Synchronizace teď nemá platnou záchrannou sadu, proto si hned potom vytvoříte novou." / "Sync has
     no valid recovery kit right now, so you'll make a new one right after."
R-58. **A bundle's new Kit must be its creator's** (Task 17 review M3): `trust::import_bundle` pins a Kit
     not pinned here only when its issuer is pinned here with the signing key the certificate names and
     is the bundle's signer or `trusted`; otherwise the whole import is refused (`keyChangeUnverified`), as
     for a Kit that is not a kind 5. A signer can no longer name a Kit "made by" a victim device to steer
     `CreatorRemoved`.
R-59. **Smaller Task 17 points.** A v2 bundle without `kit` adopted while a Kit is sealed to here logs a
     warning and is taken (only a speaking signer makes one; M2). Not changed (M5): the envelope's
     confirmed set may name `confirmed` devices that are not recipients (a quiet superseded ID) —
     harmless, they are confirmed already. Tests added: the envelope's confirmed set leaves out
     `IssuerUnknown`, `LateVouch` and `SupersededQuiet` entries, and the old Kit's seed does not open the
     new Kit's envelope (`engine/tests/rekey_review.rs`).
R-60. **The Kit join's anchor is bound to the envelope** (Tasks 18–19 review I1). `kit_id` and
     `sign_pub` are public, so `pin_kit_anchor` took any row whose certificate carried the Kit's key: a
     holder of the vault key **and** the Kit's seed (needed for the certificate's proof of possession)
     could issue the Kit's certificate from a device of its own, write that device's genesis, upload a
     snapshot, and every later Kit joiner trusted that device on first use (the R-28 genesis gap, again).
     `pin_kit_anchor(conn, kit, sign_pub, confirmed, now)` now also requires the envelope's confirmed set
     to name `(kit_id, hash of the row's certificate)` (every envelope names its Kit: setup's and
     `confirmed_after`'s), else `syncRecoveryKitUnreadable`, nothing replaced
     (`kit_join::a_forged_kit_row_with_a_forged_genesis_is_refused`, which passes the join without this
     check). §2.6 states the trust root.
R-61. **Kit join refusals** (Tasks 18–19 review M4). `newest_bundle_head` maps `401`/`403` from `rekeys`
     to `syncRecoveryKitUnreadable` (the envelope's key read the vault a moment before), no longer to
     "the key changed". Before `GET /kit`, a server whose `/v1/server` lacks the `kit` feature reads
     `validation.syncServerNeedsUpdate` (a failing `/v1/server` is left to the Kit read).
R-62. **Setup that the server's own Kit beats** (Tasks 18–19 review I2). `register_kit` returns
     `FinishSetup::KitNotRegistered` on `409 kit_exists` (and notes `sync.server_kit`), and
     `finish_setup` returns it once its first round is through (`FinishSetup::Done` otherwise); the
     command logs it and closes the window. **The "no usable Kit" signal** (covers Task 17 review M1/M4)
     is `rekey::kit::current(conn) -> Option<CurrentKit {id, created_at, created_by, storage}>`: `None`
     is sync on without a Kit this device can seal to (after such a setup, or with two Kits), and
     `storage: None` is "neuložená" (also right after a key change made the Kit, until the creator's
     save syncs). **For Task 24:** `SyncStatus.recoveryKit` is `kit::current` plus `lastReadAt`
     (`null` = no usable Kit), and `sync_finish_setup`'s answer should carry `kitNotRegistered` (or the
     UI reads `recoveryKit == null` right after setup). **For Task 27:** with `recoveryKit` null after
     setup, the done screen sends the person straight to "Vytvořit novou sadu" (the §6.3 "No Kit" texts
     exist); the Kit row's "neuložená" state uses `storage == null` (the §6.3 text exists).
R-63. **The draft's seed goes when its window ends** (Tasks 18–19 review M1): `expire_kit_draft` wipes the
     draft and closes the window once `KIT_COPY_WINDOW` ran out (or no window is open), called by
     `with_kit_draft` when it refuses and by every status poll (`current_status`).
R-64. **Texts** (Tasks 18–19 review M5, ten locales): `syncRecoveryAccountInvalid` no longer speaks of 16
     digits (the account is inside the code; it reads "Synchronizační server tuto záchrannou sadu nezná.
     Zkontrolujte kód a adresu serveru."); `syncRecoveryKitNotConfirmed` names printing, saving and
     copying; cs `syncRecoveryKitReplaced` has "dne" before the date.
R-65. **The upload lane rests while nothing may go out** (R-45, R-53; Tasks 18–19 review): the runtime's
     upload lane checks `runtime::uploads_held` first: a Kit joiner's hold, this device's key change
     pending, or given up (R-56). Downloads go on. R-45's and R-53's "not held" notes are withdrawn.
R-66. **Tests added for Tasks 18–19 review I3:** the seed scan also looks for the v2 code (grouped and
     not), the QR payload, the box secret and the raw secrets in every file of the data folder; a forged
     Kit row with a forged genesis (R-60); an envelope signed by a device that `left`; a stale envelope
     with a current snapshot (refused, nothing replaced); a current envelope with a stale snapshot, which
     **documents the R1 residual**: the removed device looks active in what was restored and nothing in
     the join can tell (the confirmed set cannot tell "removed" from "paired after the envelope"), so it
     is listed under the hold and "Odebrat je" is the answer; the hold stops snapshots, documents and the
     daily pass (also in a `FirstSnapshot` round).
R-67. **Notes only (Tasks 18–19 review).** M2: printing counts as `paper` once the print dialog opens
     (accepted with R3; "Sadu mám" is the guard). M3: a kind-3 entry pinned here within the hour before a
     Kit read hides that read from `kitUsed` (R-40) even when the read was another Kit holder's —
     accepted, the join notice is still shown. M6: a forged removal record on the Kit's row in a snapshot
     makes a Kit join refuse (DoS only; whoever can write it can replace the Kit anyway). M7: the push
     hold is written only when other devices are active — part of R1.

**Fixes from the review of Tasks 20–21.**

R-68. **The leave comes after the recheck; an own `left` record says so** (finding 1). `sync_turn_off`
     called `say_left` before `turn_off_rechecked`, so a turn-off refused at the recheck (a setup that
     finished meanwhile) or failing after it left a signed `left` record in the own row, and the next
     unlock's refresh ended sync with "removed by <this device's name>". `turn_off_rechecked(db, dir,
     password_checked, leave)` now runs `leave` only once `turn_off_allowed` passed on the profile as it
     is, right before `setup::turn_off_between_rounds`. A turn-off that still fails after the leave (a
     database error) ends sync at the next refresh; `RemovedNotice` then carries `left: true` (Local
     JSON `"left"`, default `false`), set by `rekey::refresh_trust` whenever the record naming this
     device is a `left` record (only this device's key signs one). **Texts for Task 29/31** (the UI task
     adds them in ten locales; Task 24 exposes `removedNotice.left`): "Synchronizace je na tomto
     zařízení vypnutá: toto zařízení ze synchronizace odešlo (třeba z jiné kopie profilu). Data tady
     zůstala." / "Sync is off on this device: it left sync (perhaps from another copy of this profile).
     Your data stayed here."
R-69. **A failed refresh is owed** (finding 2). The pull takes `devices_changed` before the refresh and its
     cursor moves on, so a refresh that failed was not tried again until the next device-list change or
     unlock, and a `left` or removal record merged meanwhile left its device `trusted`, a recipient of the
     next key change. `rekey::refresh_trust` now writes the Local `sync.trust_refresh_owed` when the
     refresh fails, or when the turn-off it starts fails (a locked profile excepted), and clears it after
     one that runs; `engine::round_steps` runs an owed refresh first, before resolving a pending key
     change. `rotate::plan` (and so `remove`, which re-plans before `prepare` seals the bundle and the
     Kit envelope) runs `trust::refresh` itself before it reads the recipients, and a refresh that fails
     refuses the plan (`plan` takes `now` for it; a record naming this device found there is left to the
     next round). Setup's envelope needs none (nothing to refresh before the first round). This part of
     the fix landed with the Tasks 17–19 commit, which changed `plan`'s signature.
R-70. **Tests** (finding 3 and the "confirm" item): a `left` record naming this device signed by a
     stranger's key changes nothing (sync stays on, no notice, the entry `trusted`); this device's own
     entry is pinned once, by `identity::create_own` → `trust::seed_own` (setup's genesis, a paired or Kit
     joiner, a re-join successor), with its own signing key, and another certificate in its own row is
     only `ConflictKind::OtherCertificate` with the pinned one kept (`rekey_leave.rs`).
R-71. **Smaller points.** Minor 5: `round_pass` ends on `PullReport::removed_here` before it releases held
     blobs or wakes the download lane. Minor 8: the daily pass's wait for a pickup trusted the server's
     unauthenticated `rotation`, so a dishonest server could hold it off for ever; `state::pass_waits`
     (Local `sync.pass_waits = "<rotation>:<since>"`) bounds it to `PASS_WAIT_SECS` (a day) per claimed
     rotation, after which the pass runs; the log line is `debug`. Minor 9: the unused `NoBinding` import
     of `rekey_leave.rs` is gone. Also: a clippy finding in `engine/tests/blobs.rs` (Task 21's test,
     `--all-targets`) is fixed.
R-72. **Note for the final review (minor 6):** `delete_account` (`commands/auth.rs`) checks no password
     (unchanged, as before phase 5) and, with "delete the synced data" off, now also sends a `left`
     record (`say_left`, R-48). Not changed here: the account deletion is the owner's own flow; the final
     review (Task 33) should decide whether it needs the password check `sync_turn_off` has.

**Task 22 (multi-device scenarios).**

R-73. **A join whose credentials a key change overtook is a failed pairing, not a damaged copy.** A
     joiner that received slot 2 before a removal (plan Task 14's "A removes C while B is pairing D")
     and finds a snapshot sealed under the new key on the server read `syncSnapshotInvalid` ("the
     synced copy of your data is damaged"). `join::fetched_snapshot_at` now refuses a sealed stream (`MNY1`)
     whose header names an epoch above the credentials' newest before opening it (bytes that are no
     sealed stream stay `syncSnapshotInvalid`)
     (`SyncError::ReadOnlyToken`, which a re-sync or repair reads as `syncKeyRotated`), and
     `join_new_device` answers `syncPairingFailed` ("start again on both devices"), or
     `syncRecoveryKitUnreadable` for a Kit join a key change overtook between its two reads; nothing
     is replaced. The header's epoch is not authenticated before the open: a server can only cause a
     refusal, as with any bytes it makes up. A joiner that finds the old snapshot still joins and
     ends `keyNotGiven` (unchanged). Both paths: `rekey_scenarios::a_pairing_overtaken_…`.
R-74. **Every receiver of a key change writes the rows its remover's commit writes** (R-56 corrected).
     A change given up by its maker never commits there, so nobody wrote the new Kit's row or the
     removal records into the removed devices' rows: R-56's "a Kit shown with `Pending` that then
     gives up does work" was false (the Kit join's anchor found no row: `syncRecoveryKitUnreadable`),
     and a later Kit joiner restored the removed devices as active. `pickup::adopt` now writes, in
     its savepoint and captured, each removal record of the bundle (the replaced Kit's included) into
     its subject's row where the row holds none, and the bundle's Kit row (`kit::write_row`, which
     leaves a row already there as it is). The values are the bundle's own, so they agree with the
     remover's wherever both arrive (write-once columns, `kit_storage` merged by strength). Cost: a
     receiver usually adopts before it pulls the remover's rows, so each key change queues one more
     small envelope per receiving device (`rekey_pickup::only_what_never_reached_…` now counts it).
R-75. **A Kit's envelope sealed by a device that has since left still opens a Kit join** (found by the
     property test, seed 3 of the wide run). Leaving seals no envelope, so the last envelope stays its
     sealer's until the next key change; R-28/R-43 refused any `left` signer, which made the Kit
     useless from the moment its creator (or the last remover) turned sync off until someone changed
     the key — a person who sells the Mac that made the Kit and then loses the phone could not
     recover. The joiner now accepts a `left` signer whose `left` record's epoch is at least the
     envelope's newest epoch: a later envelope under its name could only come from a copy that
     rotated after it left. A removed signer stays refused (every removal seals a new envelope).
     `kit_join::an_envelope_signed_by_a_device_that_left_is_unreadable` now covers the later-epoch
     case; `rekey_scenarios::the_kit_still_works_after_the_device_that_sealed_its_envelope_left` the
     honest one. **Corrected by R-78: the epoch comparison protects nothing** (both epochs are
     self-asserted); it is kept only as a plausibility check.
R-76. **The scenarios** (`engine/tests/rekey_scenarios.rs`, all with real rounds over the fake relay):
     a device offline with sealed and queued changes over a removal (each applied once, in order);
     two devices removing each other at once (exactly one wins, the loser is removed); a pairing a
     removal overtakes (R-73); a vault from before phase 5 syncs and refuses removal; the D3 trap end
     to end with a real pairing (`announced_payload` + `join_new_device`), both arrival orders of the
     new device's row and the holder's `left` record; a backdated certificate from a left device's
     copy (a late vouch: no key unless kept before the change); a chain F→G forged by a removed device
     in slots a colluding server serves a lagging device (both `IssuerRemoved`, neither gets a key in
     the lagging device's next change, the up-to-date device never pins them as recipients); a
     Kit-joined device offline over a new Kit (it picks up, is confirmed, takes the next change); a
     given-up change's new Kit (R-74). **The property** (`rekey_scenarios/property.rs`): four devices
     (D paired by B) and a Kit; seeded edits, rounds, offline periods, removals, new Kits, leaves and
     late vouches (one or two levels deep) from the copies of devices that left; checks after every
     step that no removed device holds a key of its removal epoch or later, and at the end
     convergence, every remaining device's chain applied once and in order everywhere (heads equal the
     author's own last envelope), every edit of a remaining device everywhere, no stored bundle giving
     a key to a removed device, a device after it left, or any late-vouch chain member, the Kit's
     envelope holding every key, and a Kit join after all the changes converging. Eight fixed seeds
     of 40 steps by default (two tests, seconds); `wide` (ignored) runs 64 seeds of 80 steps, or
     `REKEY_PROPERTY_SEEDS=a..b`.
R-77. **Observations, not changed.** (a) A device a key change left out cannot be let in later: the
     pickup walks bundles in order and stops at the first that gives it nothing (`keyNotGiven`), so
     "Je moje" helps only before the next change; after it, the way in is joining again. Tasks 24/26:
     the "Zkontrolujte" row should say so, or keep acting on the plan's excluded list at removal
     time (it does). (b) The forked-slot residual (R4) holds as documented: the lagging device applies
     the forged rows and reports `keyChangeOverrun`; what limits the damage is the transitive
     `IssuerRemoved`, which the scenario covers. (c) R-46 (a Kit joiner removed before its row went
     out) is covered by `kit_join::odebrat_je_…`.
R-78. **A left signer's confirmed set vouches for nothing** (review of R-75). A copy of a device that
     left (a backup holds its signing key and, since leaving does not rotate, the current vault key)
     could re-seal the Kit's envelope by token rotation with a confirmed set naming a genesis it made
     itself, and `kit_join_genesis` took any genesis the set named, so a Kit joiner trusted and
     confirmed the attacker's device; R-75's epoch comparison does not stop it (the `left` record's
     epoch and the envelope's are both the signer's own claim). `join::kit_join_store` now trusts
     genesis certificates on first use from the Kit's own issuer chain first, and widens them with the
     confirmed set, and applies the set, only when the signer is not `Left`; for a `Left` signer the Kit
     still opens (R-75's liveness) but nothing it names is taken. A `Removed` signer stays refused.
     **Generalised by R-106:** not only a `Left` signer, any signer that does not speak on the restored
     store (superseded, late vouch, issuer unknown or removed by another device) opens the Kit and
     vouches for nothing, and the genesis is widened only after that check.
     Test: `kit_join::a_left_signers_confirmed_set_vouches_for_nothing`. Also (R-73's sentinel):
     `fetched_snapshot_at` answers a private `KeyAhead` instead of reusing `SyncError::ReadOnlyToken`;
     `fetched_snapshot` (repair, re-sync) still reads it as `syncKeyRotated`.

**Task 24 (commands, status and the wire, §6.5).**

R-79. **Status fields** (`engine::status::status_at`, the stores read by `services::sync::view`).
     Named as §6.5 has them, not as the plan's notes did: `removalPending` (= `rotate::pending`),
     `removalNotice` (= `rekey::key_change_notice`: `{by, at, removed, kitReplaced}`), `removalNotDone`
     (`{by, devices, alreadyRemoved, kitReplaced}`), `keyChangeOverrun` (`{at}`: the time this device
     adopted the epoch the server hid, from `sync_epochs.adopted_at`, the notice itself has none),
     `removedNotice` (`{by, at, unsent, left}`, **filled with sync off too**: it is why sync is off),
     `rejoinBy`, `waitingForRemover`, `newDevices`, `recoveryKit`, `kitUsed`, `afterKitJoin`,
     `syncingSince`, `firstUpload`, `removalSupported` (`trust::is_supported`, false while off). The
     notices are wire types of `models/sync.rs`, not the `rekey` structs (no `deny_unknown_fields`
     coupling). `waitingForRemover = {by, since}` is the **oldest non-refuted unverified epoch adopted
     more than a day ago**, whatever the problem is: the UI shows it beside `keyChangeWaiting` (the
     pull's own stop) or `keyRotated`; a refuted one is `keyChangeUnverified` and not waiting.
     `newDevices` resolves names (this device's list, else 8 hex), leaves out devices no longer
     `trusted` here, gives `by = ""` for a Kit joiner, and flags `lateVouch` only for the `LateVouch`
     exclusion. `recoveryKit = {createdAt, createdBy (null when the creator is not on the list),
     storage (null = "neuložená"), lastReadAt}` from `kit::current`; `null` = no usable Kit.
     `afterKitJoin.names` are the held devices' names. `syncingSince` is
     `Runtime::round_started_at` while a round runs. `firstUpload` = no snapshot uploaded **or
     restored** here yet (`snapshot_seq` and `snapshot_uploaded_at` both empty). `kitUsed` is
     `status::kit_used` against `Runtime::kit_last_read`.
R-80. **The device list** (`devices::list_devices(conn, now)`; `sync_list_devices` passes the wall
     clock): every row but the Kit's (`is_kit_row`, platform, entry kind 5; without a bound vault only
     the platform test), each as `active | check | former`: **this device** always active; a pinned
     entry by its state (`Left` / `Removed` → former with `stateAt`, `stateBy`); a `Trusted` entry by
     `trust::recipients(&[])`'s exclusion: `IssuerRemoved` → check/`issuerRemoved`, `LateVouch` →
     check/`lateVouch`, **`IssuerUnknown` → check/`unverified`** (the "{by} no longer syncs" text would
     be wrong for it), `SupersededQuiet` → not listed (O11), a superseded ID that still writes →
     check/**`copy`** (the UI appends the localised "(kopie)", the name is the row's own); a row with
     no entry → check/`unverified` in a phase-5 vault, active/`unknown` in an older one (all rows);
     a row marked removed and never pinned → former, with the facts of its record read **unverified,
     for display only**. A `LateVouch` **descendant** (R-6) also reads `lateVouch`, its `addedBy` then
     naming a device that is itself "Zkontrolujte": Task 26's text may soften "no longer syncs" there.
     Former rows are listed for 30 days after their state time (`FORMER_LISTED_SECS`; unknown time:
     listed); `inactiveDays` is set for an active **other** device unseen for **30 or more** days
     (`INACTIVE_AFTER_DAYS`, my choice: the offline limit of a key change; the spec gives no number),
     counted from `joined_at` when it never wrote. `joinedVia` from the entry's kind; `addedBy` only for
     paired devices. Order: this device, then active, check, former; newest change first. A vault over
     64 devices lists without verdicts instead of failing.
R-81. **Commands** (`commands/sync_devices.rs`, all flag-first, scanned by
     `every_sync_command_checks_the_flag_first`, now 44): `sync_removal_plan(deviceIds, alsoRemove,
     newKit)`; `sync_remove_devices(password, deviceIds, alsoRemove, newKit, consent)` (empty password
     refused, then checked, then `guarded_change`: the engine stops, `rotate::remove` runs, the
     engine **starts again and its first round sends the records and the re-sealed changes**, so the
     command runs no round of its own); `sync_check_recovery_kit(code)`; `sync_keep_device(deviceId) ->
     bool` (`trust::keep`, then `rekey::refresh_trust` with the data folder; no password);
     `sync_kit_join_decided(removeThem)` (lifts the hold, wakes the engine; the argument is only
     logged: the UI has removed them before it calls); `sync_dismiss_notice(kind)` with the enum
     `SyncNoticeKind` (`removal | removed | removalNotDone | keyChangeOverrun | newDevices | kitUsed`),
     no answer (the UI refetches the status). **Deviations from the notes:** (a) **`sync_removal_plan`
     does not run a round first**: a round run inside a command would bypass the runtime's own
     binding-file and removed-here handling, so the command wakes the engine and builds the plan on
     what the list holds; `remove` catches up (`vault_info` → pickup) and re-plans, and a changed plan
     is `syncRemovalPlanChanged`, which the dialog already handles. (b) `RemovalOutcomeKind` has a
     fourth value **`removedHere`** (a pickup inside `remove` found this device removed; the status
     carries `removedNotice`). `Raced` and `Stuck` never reach the wire: they are errors
     (`syncRotationRaced`, an internal one: the status then shows the rejoin state). (c) The new Kit of
     `done` or `pending` is put into the process-global copy window (`open_kit_draft`, ten minutes)
     so copy, print and save work on it; for `pending` the storage the person notes lives only in the
     draft, because the Kit's row is written at the commit (a background commit writes `NULL`:
     "neuložená" although it was saved; accepted, the Kit row's "Vytvořit novou sadu" is the way out).
R-82. **`RemovalPlan` wire:** `{removing, staying, droppingOut: {device, reason}[], addedByRemoving,
     replacesKit, kitNotUpdated, consent}` with `RemovalPlanDevice {id, name, platform, joinedAt,
     thisDevice}` (a device the list has no row for is left out of the lists). `replacesKit` is
     `'no' | 'creatorRemoved' | 'cloud' | 'asked' | 'missing'` (R-57's `missing` added to §6.5's four)
     and `kitNotUpdated` is R-57's flag. `droppingOut` holds only excluded devices the **list shows as
     active or check** (so a left or removed one never shows as "will also stop syncing"), reasons
     `issuerRemoved | lateVouch | unverified` (`IssuerUnknown`, `Unverified`, `NoKey` → `unverified`).
     `SyncCheckReason` is the type of both. `RecoveryKit` is `{recoveryCode (19 groups), qrSvg,
     relayUrl, createdAt, createdBy}`; `createdBy` is **null in setup** until the device row is
     recorded (the UI shows the typed device name there).
R-83. **R-35 built:** the Local `sync.key_change_unverified = {"by": name}` (`state::
     KEY_CHANGE_UNVERIFIED_KEY`, cleared by turning sync off, a join's replace and the next key change
     adopted) is written by `pickup::check` whenever the signer may not speak, was removed, or its
     signature fails; the name is the list's, else 8 hex of its ID. `rejoinBy` reads it.
R-84. **Locales:** the validation keys the wire needs were mostly there already (R-42, R-64:
     `syncRecoveryKitReplaced`, `syncRecoveryKitUnreadable`, `syncRecoveryVaultNotFound`,
     `syncRemovalPlanChanged`); Task 24 rewords `syncRotationRaced`, `syncTooManyKeyChanges` and
     `syncTooManyDevices` in all ten locales per §6.2 and §4.4 (no "klíč" any more). **The texts of
     the status's notices, problems, the device list and the Kit rows are not added here**: per the
     plan's convention each UI task (25–31) adds its own, with the component that shows it.
R-85. **Frontend adapted, minimally** (the UI tasks redo it): `tauri-api.ts` gains `removalPlan`,
     `removeDevices`, `checkRecoveryKit`, `keepDevice`, `kitJoinDecided`, `dismissNotice`,
     `printRecoveryKit`; loses `showRecoveryKit`; `finishSetup(deviceName)` and `joinWithRecoveryKit`
     lose `kitSaved` and `account`. The two "show the Kit again" flows (`ShowRecoveryKitFlow.tsx`,
     `SyncKitSheet.tsx`) and the buttons that opened them (Keep-safe card, sync status card, phone
     sync and security sections) are **deleted**; the Keep-safe rows keep their text. The Kit join
     forms lose the account field; the kit page and file lose the account line; `RecoveryKit` has no
     `accountNumber`. `scannedCodeKind` reads a 95-character code, `payloadServer` and
     `kitQrSummary` read `moony-recover:2:<code>[:<server>]` (the phone's QR confirmation no longer
     shows an account). `useSyncDevices` leaves former devices out (`select`) until the list shows
     them. `problemAction` maps the three new problems to `none`. **Still wrong until their tasks:**
     the setup flows tick "Uložil(a) jsem si…" and finish (R-38: finish refuses with
     `syncRecoveryKitNotConfirmed` unless the code was copied, printed or saved first: Tasks 27, 31);
     `status.rotationPending`-style fields are unused; no text exists yet for `keyNotGiven`,
     `keyChangeUnverified` or `keyChangeWaiting` (Task 29); `deviceSync.json` keeps now-unused keys
     (`showKit.*`, `status.showKit`, `join.accountLabel`, `setup.kit.file.account*`,
     `keepSafe.kit.show`): the UI tasks drop them.

**Tasks 25–26 (hooks, helpers, desktop device list and removal dialog).**

R-86. **Hooks and helpers** (Task 25). `useSyncDevices()` still returns the devices on the list now
     (active and "Zkontrolujte", no former ones): every caller that counts or names "other devices"
     wants that. The list itself reads `useSyncDeviceGroups()` (`groupDevices`). `useRemovalPlan(ids,
     alsoRemove, newKit, enabled)` is never cached (`gcTime 0`, `keepPreviousData` so ticking keeps the
     list on screen). `removeDevices` takes the password through a ref `mutationFn` empties (variables
     hold only ids and the consent), `gcTime 0`, and the dialog `reset()`s it on leaving. Also added:
     `keepDevice`, `printRecoveryKit`, `checkRecoveryKit`, `dismissNotice`, `kitJoinDecided`.
     `quietDays`/`QUIET_AFTER_DAYS` stay for the phone's list until Task 30 reads `inactiveDays`.
R-87. **`syncState` kinds** `syncingLong` (round older than 60 s, tone neutral so the popover keeps its
     sync-now button; Task 29 labels the button "Zkusit znovu"), `firstUpload` (also while idle) and
     `removing` (`removalPending` in a running round), keys `indicator.*` and `phone.row.*` in ten
     locales. `keyRotated` without `waitingForRemover` reads as plain `syncing`. `syncProblemState`
     gives the one rejoin state (`keyNotGiven`, `keyChangeUnverified`, `unauthorized`; `by` = `rejoinBy`
     except for `unauthorized`) and the waiting state; **Task 29 still has to show them**, and
     `problemAction` still answers `none` for these problems. `statusPopoverLines` gives the popover's
     lines (`popover.line.*`, texts added; dates are formatted by the view through `dates`).
R-88. **Device list** (Task 26). Active other devices open for their details and "Odebrat…"; a
     "Zkontrolujte" row shows its reason at once with "Je moje" (late vouch only) and "Odebrat…", plus the
     R-77a caveat ("Potvrďte ho před příštím odebráním zařízení; potom by se muselo připojit znovu.").
     Own sentences: `devices.check.lateVouch` is worded to fit an adder that is itself "Zkontrolujte"
     ("nebo ho Moony nemůže ověřit"), and `lateVouchNoName` when `addedBy` is unknown. A failing
     `keepDevice` (the Rust side may still answer an internal error for a row no longer trusted) is a
     toast `devices.check.keepFailed`; the list re-reads either way. The "Ztratili jste zařízení…"
     collapsible is gone from the desktop (`limits.lostTitle` stays for the phone until Task 30). The
     pre-phase-5 note (`devices.unsupported`) replaces the line under the list when
     `removalSupported` is false.
R-89. **Removal dialog.** One `Dialog` (560 px): plan, then "Odebírám {name}…", then a toast or the Kit
     step. "Odebrat také" ticks start unticked (spec §4.3). `droppingOut` rows carry "Je moje" for a late
     vouch (reloads the plan). Outcomes: `done`/`pending` with a kit open the Kit step (`NewKitStep`:
     panel, "Sadu mám, hotovo" after copy, print or save; Esc and outside clicks are off there), `done`
     without a kit toasts, `alreadyRemoved` and `removedHere` toast their sentences
     (`remove.toast.*`). `syncRemovalPlanChanged` shows its text under the password and reloads the plan.
     **Task 27 note:** `RecoveryKitPanel` gained "Vytisknout" (`sync_print_recovery_kit`) and mounts a
     `KitPrintSheet` (portal in `body`, `.kit-print-sheet` in `index.css`, print media shows only it);
     the panel's false "cannot be cut off" line (`limits.lostDevice`) is no longer rendered. Not
     verified in the built app: the print dialog, the dialog at 1080 px and the layout of long German
     strings (not run, no Tauri host here).
R-90. **Task 24 review fixes** (I1–I3, M1–M4). I1: `sync_keep_device` only marks (`trust::keep`) and wakes the
     engine; the next round's refresh owns everything it can end (removed here). I2: `trust::keep`
     answers `false` for "nothing to keep" (this device, never pinned, not trusted, gone), no error.
     I3: a removal that makes a kit checks the kit's view (code, QR, account, vault) with a throwaway seed
     before `rotate::remove` sends anything (`precheck_kit_view`), the copy-window owner is read first, and
     the post-commit `show_kit` cannot fail: a problem is logged and the outcome carries `kit: null`, so
     the Kit row's "neuložená" / "Vytvořit sadu" leads on. M1: `sync_removal_plan(…, refresh)` wakes a
     round only when `refresh` is true; `useRemovalPlan` sends it on a dialog's first read only (reset
     when the dialog closes); the round already notes the server's Kit (`note_server_kit` after every
     round that read the vault info), nothing added. M2: `sync_dismiss_notice(kind, devices)`: for
     `newDevices` only the listed device IDs go (`trust::dismiss_new_devices`), `null` clears the list
     (`dismissNotice('newDevices', ids)` in `tauri-api.ts`; the mutation takes a kind or `{kind,
     devices}`). M3: a failed `trust::recipients` in `view::extras` is logged (the new-device list loses
     only its "lateVouch" mark); removed rows that were never pinned and carry no record (pre-phase-5
     removals) are no longer listed (they had no time to age out). M4: `sync_kit_join_decided` goes
     through `join::decide_kit_join`: `validation.syncRemovalPending` (new locale key, ten locales)
     while a key change of this device is pending, or with `removeThem` while a device the hold names is
     still trusted and not removed. Wire types did not change (command arguments are not in
     `bindings.rs`), so `generated-types.ts` is unchanged.
R-91. **Desktop Kit** (Task 27). Setup: the kit step shows the §6.3 intro, the panel (Vytisknout · Uložit jako
     soubor… · Zkopírovat kód), "Sadu ukážeme jen teď…" and the "not the recovery key" line; the tick-box
     is gone, "Sadu mám, pokračovat" waits for a print, save or copy (`kitKept`). After `finishSetup` the
     dialog shows "Synchronizace je zapnutá" (Přidat druhé zařízení opens `AddDeviceDialog` through
     `SyncSection`, Později closes), or, when the returned status has `recoveryKit == null` (R-62),
     goes straight to `NewKitDialog missing` ("Vytvořit sadu"). `SyncSection` now renders its dialogs
     after the cards in one place, so the setup dialog survives the phase turning `on`. `KitRow` (in
     `SyncStatusCard`): created at/by, where it is kept (`kit.storage.*`), last read, "Ověřit sadu" →
     `CheckKitDialog` (typed or pasted code, answer valid/replaced/otherSync/unknown, code cleared after
     each answer and on close, mutation `gcTime 0` + reset), "Vytvořit novou…" → `NewKitDialog`
     (`useRemovalPlan([], ticks, true)`, `removalRequest(…, newKit=true)`, ticks for the devices that
     joined with the old kit, then `NewKitStep`; a `done` outcome with `kit: null` toasts
     `kit.notShown`); `storage == null` shows "neuložená" with "Vytvořit novou sadu", `recoveryKit ==
     null` "Vytvořit sadu"; `kitUsed` is an alert in the row with "Vytvořit novou sadu" and "Vím o tom"
     (`dismissNotice('kitUsed')`). Task 29 may move that alert into `NoticeList`. Buttons that carry the
     new texts wrap (`whitespace-normal`). The printed sheet and the saved page say "Vytvořena {date} ·
     {device}" from the kit itself (`useRecoveryKitCopy(kit)`), not today's date.
R-92. **Deviations, Task 27.** (a) One "Uložit jako soubor…" button saves the page (HTML); the text
     variant has no button on the desktop any more (`saveRecoveryKit('text')` stays in the API; the
     phone's file export still uses `recoveryKitText`). (b) `showKit.*`, `setup.kit.description`,
     `setup.kit.confirm`, `setup.kit.saveFirst` and `setup.device.success` stay: the phone's setup sheet
     (Task 31) and the desktop password question still use them. Dropped: `status.showKit`,
     `join.accountLabel`, `setup.kit.file.accountLabel/accountNote`, `setup.kit.savePage/saveText`
     (deviceSync), `keepSafe.kit.show` (settings). (c) `setup.kit.differs` has the §6.3 wording for both
     desktop and phone. (d) Not verified in the built app: print, the dialogs at 1080 px with de and ja,
     the done screen after the real phase flip (no Tauri host).

**Tasks 28–29 (desktop pairing code, notices and status).**

R-93. **Desktop typed pairing code** (Task 28, dbf0fb39). `JoinMethod` is `pair | code | kit`; `JoinChoice` has the
     third card ("Mám jiný počítač se synchronizací") between the phone and the kit ("Nemám u sebe žádné
     zařízení"). `PairingJoinFlow` takes `mode: 'show' | 'type'`: `type` shows a pairing-code field (a
     textarea, not `TypedCodeField`: that is the six digits the holder types) plus the password in Settings, no
     server field (the holder's server comes with the code), and answers with `pairingScan(code, {password,
     deviceName})`, the phone joiner's path; the digits and the join are the existing steps. The code is
     checked with `pairingCodeProblem`, moved from `mobile/lib/sync-scan.ts` to `utils/pairing-code.ts`
     (re-exported there). No backend change. Both `JoinDeviceDialog` and the first run's `ConnectStep` offer it.
     `pairing.codeHint` (the text code under the holder's QR) now says the code can be sent in a message
     (the six digits protect it); the holder's "Zkopírovat kód" already existed (cs text reworded).
     `validation.syncPairingCodeInvalid` no longer says "scan" (ten locales).
R-94. **The holder finishes visibly** (Task 28). `useHolderFinish(view)` (in `use-device-sync.ts`): while a
     pairing as the holder runs it reads the device list once for the baseline, after `sent` it polls the
     list every 3 s for a device not in the baseline (`newlyJoinedDevice`, not this one, not former) and
     reports `joined` (its name) or `late` (two minutes without it). `PairingSteps` (`finish` prop) says
     "{name} je připojené" / "Dokončete připojení na druhém zařízení…" (`pairing.deviceConnected`,
     `pairing.sentLate`) and the dialog's and the phone sheet's closing button reads "Hotovo" once joined.
     Frontend only; not seen live (no Tauri host).
R-95. **Notices** (Task 29). New `NoticeList.tsx` in the status card (after the problem alert): "Data jsou
     zpět" (`afterKitJoin`; "Odebrat je" opens `RemoveDeviceDialog` for the named devices still on the list
     and, once they are gone, calls `sync_kit_join_decided(true)`; with none left the button reads
     "Pokračovat"; "Mám je" is `false`; a failure, among it `validation.syncRemovalPending` "Odebrání ještě
     probíhá", is a toast; while a removal of this device is pending the card says so and the button waits),
     `removalNotice` (the "also made a new kit" sentence is appended when devices were removed too),
     `removalNotDone` (kit variant adds "Vytvořit novou sadu"), `keyChangeOverrun` (with and without a date)
     and one box per `newDevices` entry, dismissed with `{kind: 'newDevices', devices: [id]}` so a device
     that arrives meanwhile stays. A Kit join gets the strong box (title, "To jsem já", "Odebrat a vytvořit
     novou sadu" = `RemoveDeviceDialog` with the new `newKit` prop), a late vouch "Je moje" (keeps, then
     dismisses) · "Odebrat…". `RemoveDeviceDialog` also gained `onRemoved` and names several devices
     (`removalTitleNames`, `remove.bodyMany`). Pure helpers in `utils/sync-notices.ts`. `kitUsed` stays in
     `KitRow`.
R-96. **Status states** (Task 29). `SyncProblemAlert` shows the rejoin state (title, body, "{by}, which this
     device doesn't know…" from `rejoinBy`, "Připojit znovu") and the waiting state (`waitingForRemover`) from
     `syncProblemState`; plain `keyRotated` (no waiting) shows no alert, only "Synchronizuji…".
     "Připojit znovu" = `RejoinDialog` (password → `turnOff` → `SyncSection` opens the join dialog; the
     dialog says unsent changes stay in the backup). `syncingLong` turns the "Synchronizovat" button of the
     status card and the popover into "Zkusit znovu" (not loading, it calls `sync_now`); `firstUpload` adds
     "Nahrávám první kopii dat. Můžete pracovat dál." (`setup.done.body`) to both. The popover lists
     `statusPopoverLines` (dates formatted by the view) and, with sync off, `SyncRemovedSection` shows the
     removed line. Reworded in ten locales: `status.problem.keyNotGiven|keyChangeUnverified|keyChangeWaiting`
     (new), `keyRotated` (now the ordinary "Synchronizuji…", the old "klíč synchronizace" text is gone),
     `captureBroken`, `invalidChain`, `serverWentBack` (no "synchronizační účet", no mention of the
     re-sync button), `indicator.waiting` ("Čeká se na změnu z jiného zařízení").
R-97. **Removed device** (Task 29). `RemovedNotice.tsx`: `RemovedNoticeDialog` (mounted in `AppShell`, readable
     with sync off) shows once per unlock; closing it with Esc keeps the notice (Settings card and popover line),
     "Rozumím" dismisses (`removed`), "Připojit znovu" dismisses and opens the join choice. `left: true` has
     its own title and the R-68 sentence; `removedNotice.unsent` adds the plural line. In Settings with sync
     off the same text is a `RemovedNoticeCard` above the intro.
R-98. **Turn-off, danger card** (Task 29). The turn-off dialog uses the §4.6 description (the others see it
     left) and the "{n} změny odsud se ještě neodeslaly" count; the danger card keeps "Vypnout synchronizaci
     tady" and "Smazat synchronizovaná data na serveru". "Synchronizovat toto zařízení znovu" left the
     danger card: `SyncProblemAlert` offers it as a second button for `offersResync` problems
     (`undecryptable`, `invalidChain`, `missingParent`, `quarantined`, `internal`; never the gap, the server
     problems or the rejoin state) and after a repair that could not send everything (as before).
R-99. **Deviations, Tasks 28–29.** (a) `turnOff.noCutOff` stays as a key (the phone's turn-off sheet still
     reads it, Task 30) with a new text ("Ostatní zařízení uvidí, že se toto odpojilo."); the desktop does
     not render it. (b) `limits.lostDevice`/`limits.lostTitle` (still with "odříznout") and `resync.hint`
     stay for the phone until Task 30. (c) "Synchronizační účet" wording remains in the setup flow's account
     step (`setup.account.*`) and `common` `syncVaultLimitReached`/`syncAccountNotFound`: not part of
     these tasks. (d) The kit-join box's "To jsem já" only dismisses; nothing is recorded about the join.
     (e) The Task 28 commit carries no As built entry; this one does. (f) Not verified in the built app: the
     typed join between two real computers, the holder's finish across the real flip, the dialogs at
     1080 px with de and ja, the unlock-time dialog.
R-100. **Phone device list, device sheet, removal sheet** (Task 30). `SyncDeviceRows` reads the groups
     (`useSyncDeviceGroups`): active (this device first) then "Zkontrolujte" rows with `RowBadge`s ("Zkontrolujte",
     "Odebírá se…", `devices.inactive`), "(kopie)" through `deviceLabelKey`; this device's row renames it, another
     device's row opens `DeviceSheet` (label-value rows: type, last active, Moony version, how it joined; the
     check reason with `keepNote`; "Je moje" for a late vouch; "Odebrat zařízení" = `remove.confirm`, a danger
     text button, hidden while `removalAvailable` is false). The sheet is addressed by device ID and reads the
     live list, so "Je moje" updates it in place. Left or removed devices are one folded row
     (`devices.formerGroup`) opening `FormerDevicesSheet`. Under the list: `devices.underList`, or
     `devices.unsupported` when `removalSupported` is false. `RemoveDeviceSheet` mirrors `RemoveDeviceDialog`
     (plan, "Odebírám…", toast or Kit step; `newKit`, `onRemoved`, several names, `PLAN_CHANGED_KEY` reload,
     gcTime 0 and the password in a ref through `removeDevicesWithPassword`); "Odebrat také" ticks are 44 pt
     label rows (`AlsoRemoveList`), the password field is `PasswordField` (the `SyncPasswordSheet` pattern, 16 px
     floor). The Kit step is `NewKitStep`, a sheet nothing dismisses but "Sadu mám, hotovo" (or "Zavřít" once
     `KitBody`'s lifetime or a password request ends it).
R-101. **Deviations, Task 30.** (a) `KitBody` (changed here, not in Task 31, because the removal's Kit step shows
     it): one "Uložit nebo sdílet…" button (`setup.kit.saveShare`, iOS only) saves the Kit's page through the
     Files picker, as §6.2 says for the phone; the "as text" button (`phone.saveText`, `phone.savePage`) and the
     `limits.lostDevice` paragraph are gone. `recoveryKitText` and `SYNC_KIT_TEXT_FILE` stay (tested, unused by
     the UI). (b) Dropped keys: `devices.quiet*`, `devices.lastChange`, the whole `limits.*`, `phone.saveText`,
     `phone.savePage`; `quietDays`/`QUIET_AFTER_DAYS` and their test are gone. (c) New keys: `devices.sheet.
     {platform,activity,version,joined}`, `setup.kit.saveShare`. (d) Not ticking a removed device's descendants
     by default (the spec is silent; same as the desktop). (e) Not verified on a device: the sheets at 320 pt with
     the largest text size, dark mode, the kit step on iOS.
R-102. **Phone Kit and notices** (Task 31). `KitSection` (after the server rows): when the kit was made and where
     it is kept, "Naposledy použita", "Ověřit sadu" (`CheckKitSheet`, typed or pasted code, no password) and
     "Vytvořit sadu / novou sadu / novou…" (`NewKitSheet`, `missing` when `recoveryKit` is null, disabled while
     `removalAvailable` is false); the `kitUsed` alert with "Vytvořit novou sadu" · "Vím o tom" sits above it.
     Setup: the kit step shows `setup.kit.introPhone`, `KitBody`, `setup.kit.shownOnce`; the tick box is gone and
     "Sadu mám, pokračovat" unlocks once the kit was copied or saved; after `finishSetup`, `afterSetup(status)`
     opens the done sheet ("Synchronizace je zapnutá", "Přidat druhé zařízení" → the scan flow, "Později") or
     `NewKitSheet missing`. `SyncNotices` (the `NoticeList` equivalent, `InlineAlert`s with 44 pt text actions) is
     rendered by `SyncStatusBlock` under the problem alert: "Data jsou zpět" (`RemoveDeviceSheet` with
     `onRemoved` → `kitJoinDecided(true)`, "Mám je"), removal, removal not done, overrun, one box per new device
     (late vouch "Je moje" · "Odebrat…", Kit join "To jsem já" · "Odebrat a vytvořit novou sadu"). The problem
     card has the rejoin state (`problem.rejoin.*`, `rejoinBy`) and the waiting state (`problem.waiting.*`), both
     with "Připojit znovu" = password sheet (`rejoin.*`) → `turnOff` → the join choice; plain `keyRotated` shows
     no alert; `syncingLong` turns "Synchronizovat" into "Zkusit znovu"; `firstUpload` adds `setup.done.body` under
     the status. `RemovedBlock` (sync off, above the intro) and `RemovedNoticeSheet` (mounted in `MobileShell`
     while unlocked, once per unlock; Esc/scrim keeps the notice) carry `removed.*` and `left`.
R-103. **Deviations, Task 31.** (a) "Připojit znovu" in the unlock sheet dismisses the notice and navigates to
     Nastavení → Sync with `?join=1` (`JOIN_PARAM`), where the page opens the join choice once and clears the
     parameter; the shell cannot host the join sheets. (b) "Ověřit sadu" on the phone has no camera: the code is
     typed or pasted (the QR's text works too); `scanQrCode` would need the check sheet to step aside as the
     setup sheet does. (c) "Synchronizovat toto zařízení znovu" left the danger section (as on the desktop,
     R-98): the problem card offers it for `offersResync` problems and after a repair that could not send
     everything. (d) New key `setup.kit.introPhone` (the desktop intro says "tento počítač" and "Vytiskněte");
     the phone's "save first" hint stays `setup.kit.saveFirst`; `phone.row.waiting` now has the text of
     `indicator.waiting`; `turnOff.noCutOff` is kept as the second sentence of the phone's turn-off sheet
     (its new text is not "cannot be cut off"). (e) Dropped keys: `resync.hint`, `setup.kit.description`,
     `setup.kit.confirm`, `setup.device.success`; `showKit.*` and `join.choice.*` stay (the desktop setup and join
     choice read them); `status.showKit`, `keepSafe.kit.show`, `join.accountLabel` and
     `setup.kit.file.account*` were already gone. (f) Not verified on a device: every new sheet at 320 pt with
     the largest text size, dark mode, the kit step and "Uložit nebo sdílet…" on iOS, the unlock sheet across a
     real removal, the rejoin flow, de and ja wrapping.
R-104. **Review fixes of the phase 5 UI (desktop and phone).** (a) The new kit leaves state when it expires:
     both `NewKitStep`s take `kit: RecoveryKit | null` and `onGone`; the removal dialog/sheet and the new-kit
     dialog/sheet answer with `removeDevices.reset()` and a kit step whose kit and `view.kit` are `null`
     (`withoutKit`), so neither the step nor the mutation result keeps the seed until "Zavřít". The setup flows
     already did this through `dropKit`. (b) **Wire change:** `afterKitJoin` is `{devices: [{id, name}],
     newestChangeAt}` (`SyncAfterKitDevice`; `names` is gone, nothing else read it); `afterKitJoinDevices`
     matches by ID, and "Data jsou zpět" shows a skeleton (desktop) or a disabled "Odebrat je" (phone) until
     the device list has loaded, so "Pokračovat" is never offered on an empty list. (c) `SyncProblemAlert`
     keeps its dialogs outside the box (`ProblemBox`), so a problem that clears does not unmount an open
     rejoin, repair, re-sync, turn-off or skip dialog. (d) A failed removal also reloads the plan on
     `validation.syncRotationRaced` (`planNeedsReload`); `kitJoinDecided` also invalidates the device list;
     the phone's saved kit page says "· {device}" (`useRecoveryKitCopy(kit)`); `dropReasonKey('copy')` reads
     as `unverified` (the backend never sends it, so there is no `dropReason.copy` text); the removal
     dialog/sheet takes `deviceNames` for its title until the plan names them; `useRemovalPlan` asks for
     `refresh` only on a dialog's first read (not again after a failed submit); `useHolderFinish` takes its
     baseline from a fresh read at the start of the pairing (and from the first answer even when it comes after
     `sent`), never from a cached list. (e) Notices: dismissal is pending per notice (its key) and a second tap
     is ignored; "Je moje" that returns `false` shows a quiet "list reloaded" toast and does not dismiss, and a
     failed dismissal after a successful keep no longer reads "keepFailed". (f) Two sentences join through
     `utils/sentences.ts` (`sentences(locale, …)`, moved from the phone's `list-text`), so ja and zh get no
     ASCII space: problem alerts, notice boxes, device rows and sheets, the removed notice, the leftover card.
     (g) Dead key `devices.joined` dropped; `leftover.noServer` no longer says "sync account" in any locale;
     "recovery kit" is lower case in `delete.afterwardsOwnServer` (en, cs, es, it, pl). The print sheet's
     `#000`/`#fff` carry a `colour-literal-ok` comment. A failed turn-off or rejoin (phone and desktop) resets
     the mutation, so the typed password does not linger in its variables. Printing counts as kept when the
     print dialog opens (R-91, kept on purpose). Not verified live: the skeleton and the disabled phone button
     while the list loads, the baseline of `useHolderFinish` on a slow list, a problem clearing under an open
     dialog.

R-105. **Fixes from the Task 33 desktop live run.** (a) **A Kit made by a Kit joiner opened nowhere.** A Kit
     joiner's first store (`trust::tofu_from_rows`) refused every Kit but the anchor as `SecondKit`, so a device
     a replaced Kit had issued was never pinned: when that device was the envelope's signer (a Kit joiner that
     removed the others, or made a new Kit), the next Kit join answered `syncRecoveryKitUnreadable` ("data na
     synchronizačním serveru k ní nesedí"); otherwise the device was silently left out of the joiner's next key
     change. Now, once the anchor is pinned, `pin_candidates(former_kits)` pins a replaced Kit too, **removed at
     once** by the `removed` record its row holds (`former_kit_removal`: the Kit's issuer vouches for it here, the
     record verifies by its signer's row certificate, the signer pinned here or issued by this very Kit), so it
     vouches only for what it issued before it was replaced, and what it vouches for is left out with it unless
     the envelope's confirmed set names it. A replaced Kit without such a record stays out (`SecondKit`), as
     before; its creator being left out for its issuer does not keep it out (the joiner's confirmed set applies
     only after the store is built). Tests: `kit_join::a_kit_made_by_a_kit_joiner_lets_the_next_device_join`,
     `a_device_an_older_kit_issued_is_kept_by_the_next_kit_joiner`,
     `a_kit_chain_through_a_removed_first_device_still_joins`. (b) **"Synchronizace trvá déle než obvykle" never
     appeared:** a stuck round's status reads the same at every poll, so React Query kept the reference and
     nothing re-rendered past the 60 s mark; `useDeviceSyncStatus` now notifies on `dataUpdatedAt` too (polls
     run only while syncing). (c) The biometric offer stacked under the removed-device dialog at unlock (two
     modal dialogs); `EnableBiometricsDialog` now waits while `removedNotice` is unanswered or the status loads.
     (d) "Data jsou zpět" named a device removed since the join (the stored names of a stale snapshot);
     `afterKitJoinNames` names only the held devices still on the list once it has loaded. (e) After "Sadu mám,
     hotovo" the Kit row flashed "Záchrannou sadu jste si neuložili" until the next status read;
     `RecoveryKitPanel` invalidates the status when the kit is kept. (f) `auth.onboarding.connect.subtitle`
     said "synchronizační účet" (forbidden by the glossary) in every locale; now "přijdou ze synchronizace".

As run (Task 33, desktop, 2026-11-17). Three debug instances (`.livecheck1`–`.livecheck4`, ports 1421–1424)
against a local relay of `feat/recovery-kit`; cs, de, ja, en; 1080 px; dark and light. Passed: typed join
(wrong code, connection code pasted, outer spaces, wrong six digits then right with a space, "{name} je
připojené" · "Hotovo", the 2-minute hint); setup done screen across the phase flip; removal without and with
the Kit step (Kit line `creatorRemoved`), wrong password under the field, Escape does not close the Kit step,
the step drops the kit when the window is hidden; "Ověřit sadu" old, unsaved-replaced, current (lower case,
spaces) and mistyped; Kit row kept/unsaved/"Naposledy použita"; Kit join after two removals with the replaced
kit refused ("nahradila novější"), skeleton then "Odebrat je" (removal + Kit step) and "Mám je"; Kit-join box
"Odebrat a vytvořit novou sadu"; removed dialog at unlock (Escape keeps the card, "Rozumím", "Připojit znovu" →
join choice); popover removed line with sync off; newDevices dismissal with another notice present; syncingLong
"Zkusit znovu" (relay paused with SIGSTOP); the relay admin's Kit line. Not reproducible here: the `left`
variant, the unsent count, a pending removal (an unreachable server fails the removal before anything changes:
`vault_info` first), late vouch, rejoin/waiting cards, a problem clearing under an open dialog, `kitUsed`, a Kit
of another sync. Not checkable: print and save dialogs (native), Windows/Linux.

**Final review of phase 5 (2026-11-17).**

R-106. **A Kit join could be made to trust a planted device** (final review I-3). R-78 stopped only a
     `Left` signer: a holder of the current vault key (a backup of a device that left, a superseded copy)
     could write a genesis row X of its own and re-seal the Kit's envelope signed by X with a confirmed set
     naming the Kit and X; `kit_join_store` widened the genesis from the set before checking the signer, so X
     was pinned trusted and then confirmed. The same with an older ID's key after its re-join, or a late
     vouch the joiner leaves out (the signer check looked at the state only). Now `join::kit_join_store`
     builds the store with the Kit's own chain as the only genesis, refuses a signer not pinned with that
     certificate or `removed` (as before), and applies the confirmed set, widening the genesis first, only
     when `trust::kit_join_speaker` says the signer speaks: `trusted`, not superseded, not left out for its
     issuer. Two exceptions keep honest chains whole, each resting on a fact a vault-key holder cannot make
     up: **the Kit's creator** (the anchored Kit's certificate names it as issuer with its pinned key; only
     the device that made the paper could have issued a certificate carrying the Kit's proof of
     possession), which keeps R-105's chains (a Kit joiner that replaced its own Kit and made the next one);
     and **a remover of its own issuer** (left out as `IssuerRemoved` only because its issuer is `removed`
     by the signer's own record, the issuer not a Kit): `removal_counts` counted that record only while the
     signer was not left out, which keeps the security review's MEDIUM 3 (B removes A, which paired B and C;
     B's envelope keeps C). **Accepted cost:** an honest envelope signed by a device that is a late vouch on
     the joiner (R-23, equal positions), a superseded ID, or a device whose issuer another device removed
     confirms nothing: those devices show "Zkontrolujte" and a late vouch or an unknown issuer can be kept.
     A replaced Kit vouches only for what it issued before its removal record (`vouch` against the Kit's
     `state_at`; a device dated before it is left out, `IssuerRemoved`, and does not speak). Tests:
     `kit_join_attacks::a_planted_genesis_signer_is_refused`, `a_superseded_signer_vouches_for_nothing`,
     `a_late_vouch_signer_vouches_for_nothing`, `a_replaced_kits_device_issued_after_its_removal_is_refused`,
     `a_replaced_kits_backdated_device_vouches_for_nothing`,
     `a_signer_whose_issuer_another_device_removed_vouches_for_nothing`; R-105's three and MEDIUM 3's
     (`kit_join::the_confirmed_set_keeps_a_device_whose_issuer_was_removed`) still pass.
R-107. **"Je moje" no longer keeps a device whose issuer was removed** (final review M-1). `trust::keep`
     confirmed any `trusted` entry, `IssuerRemoved` included, without a password. It now answers `false`
     for one left out as `IssuerRemoved` (whose issuer was removed, or is left out for that): only a late
     vouch (`LateVouch`) or an unknown issuer (`IssuerUnknown`) is the person's to keep, as the UI offers.
     Tests: `trust_tests::keep_refuses_a_device_left_out_for_its_removed_issuer`; `devices_tests::
     a_device_a_removed_device_added_is_marked_check_and_cannot_be_kept` and `rekey_pickup::
     a_key_change_by_a_device_left_out_for_its_issuer_is_not_taken_until_it_is_kept` (its removed-issuer
     case now stays `keyChangeUnverified`) changed with it.
R-108. **Saving the Kit on the phone counts** (final review I-2). "Uložit nebo sdílet…" on the phone
     (`KitBody`, setup and the new Kit after a removal) hands the Kit's page to the Files picker, and nothing
     recorded it: a phone setup kept only that way was refused (`syncRecoveryKitNotConfirmed`), and a Kit saved
     there (likely iCloud Drive) kept `kit_storage` NULL, so no removal replaced it. A saved page now calls
     `sync_note_recovery_kit_saved` (phone only, within the copy window, `with_kit_draft`), which records
     `cloud` (`setup::note_phone_kit_saved`): the picker does not say where the file went, so the strongest,
     most cautious value. Test: `setup::a_phone_save_alone_finishes_setup_as_a_cloud_kit`.
R-109. **Smaller review fixes** (final review I-1, M-2, M-5). (a) `shared/generated-types.ts` is the
     generator's output again (a Prettier pass had reformatted it and CI's drift check failed);
     `.prettierignore` lists it. (b) `sync_remove_devices` checks that a new Kit's view can be built
     (`precheck_kit_view`) for any removal whose plan replaces the Kit (`replaces_kit` other than `No`: its
     creator or a cloud Kit removed with it), not only when "Vytvořit novou sadu" was asked for. (c) Migration
     041's comment says `kit_storage` keeps the strongest value (R-54), not "last-writer-wins" (comment only);
     ADR 0032 names the As built range up to R-110.
R-110. **Fixes from the desktop live run, texts** (2026-11-17; with final review M-6). (a) The device that added a device no
     longer gets its own new-device notice ("Přibylo zařízení X, přidané ze zařízení {toto zařízení}"):
     `trust::note_new_devices` skips a device whose issuer is this device. Test:
     `trust_tests::new_device_notices_are_written_for_pairings_and_kit_joins_only`. (b) A device name is no
     longer the bare subject of a gendered participle or adjective: cs "Zařízení {{name}} je připojené",
     "Zařízení {{name}} bylo mezitím odebráno", "Zařízení {{name}} se připojilo…", subject-first
     "Zařízení {{by}} mezitím odebralo…"; the same in pl ("Urządzenie {{name}}…"), it ("Il dispositivo
     {{name}}…"), es ("El dispositivo {{name}} está conectado", "Se quitó {{name}}"), sv/nb ("Enheten
     {{name}}…", where a later "den" refers to it); de needs none (its predicates do not agree), but
     `notice.notDone.alreadyRemoved` no longer says "wurde" of a list. (c) The top bar's descriptor
     ("Nastavení / synchronizace") is lower-cased except in German, whose nouns keep their capital
     (`utils/breadcrumb-tag.ts`): "Einstellungen / Synchronisierung"; de `remove.stays` is "Diese Geräte
     synchronisieren weiter:". (d) ja and zh sync strings have no space between an interpolation and the
     CJK text around it (`deviceSync` and the `sync` keys of `common` and `settings`). (e) The dialog's close
     button is named in the app's language (`common:buttons.close`), not "Close". (f) A removal or a new Kit
     that fails because the sync server cannot be reached (it fails before anything changes, and nothing
     retries it) says so in its dialog (`remove.unreachable`: "Synchronizační server není dostupný, takže se
     nic nezměnilo. Zkuste to znovu, až budete připojeni k internetu.") instead of the common "synchronizace to
     zkusí znovu" (`utils/sync-removal.ts` `removalErrorKey`, desktop dialogs and phone sheets). (g) M-6:
     `phone.kitConfirmTitle` no longer says "synchronizační účet" in any locale ("Připojit tento telefon
     pomocí záchranné sady?"); `auth.onboarding.connect.subtitle` was already fixed by R-105 (f). Left as owner
     questions: spaces inside a pairing code, a server address without `http://`, the hosted-account copy
     (`validation.syncAccountNotFound`, `setup.account.*`).
