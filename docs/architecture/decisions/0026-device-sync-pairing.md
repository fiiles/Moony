# 0026. Device sync: pairing a second device, joining it and the phone

Date: 2026-11-10

## Status

Accepted. Records phase 3 of `docs/specs/2026-11-06-device-sync-client-design.md` as built: pairing
over the relay's mailbox, a new device's join (pairing or the Sync Recovery Kit), the device list
and the phone app's sync, still hidden behind the runtime flag (`src-tauri/src/services/sync/`:
`pairing/`, `join.rs`, `devices.rs`, `feature.rs`, `relay/`; `commands/sync_pairing.rs`; the iOS
binding and clipboard of `plugins/moony-keychain`; `src/components/settings/sync/`, `src/mobile/`).
Builds on ADR 0024 and ADR 0025. Where the spec's text differs, the code and this ADR win.
Amended by [ADR 0027](0027-device-sync-audit-fixes.md) (2026-11-12): pairing protocol 2 (see the
marked paragraph under **Bytes**), joins that keep their backup, and the Recovery Kit.

## Context

Phase 2 synced one device and joined only through repair and re-sync. A second device needs the
vault's credentials (sync server, account, vault ID, every key epoch) without the relay, a
photographed QR code or a stranger who answers first ever learning them. The spec (§11.3, D21)
named an X25519 exchange through a short-lived relay mailbox and a six-digit code compared on
both screens, but not its bytes. Its server, its roles and the lost answers of a read-once,
write-once mailbox were settled by the plan's corrections and the pre-flight review (PF1–PF12),
and by owner answers Q1 (no checkout on the phone) and Q2 (old device rows stay listed).

## Decision

**Bytes.** The shower (S) shows `moony-pair:1:<mailbox>:<S public>[:<sync server>]`: 16 random
mailbox bytes and S's ephemeral X25519 key, each canonical base64url without padding, and a 4th
field (the base64url of the normalized address) only for a server that is not the default; at most
512 bytes. The scanner (C) writes slot 1 = `0x01 ‖ role (0x01 holder, 0x02 joiner) ‖ C public`
(34 bytes). Both run X25519, refuse a non-contributory (low-order) result, and take 40 bytes of
HKDF-SHA256 (salt = mailbox, info = `"moony pair v1" ‖ S public ‖ C public`): the first 32 are the
session key, the last 8 as a big-endian u64 mod 10^6 the six-digit code. Slot 2 = `0x01 ‖ 24-byte
nonce ‖ XChaCha20-Poly1305` under the session key, with 95 bytes of AD (`0x01 ‖ "holder->joiner" ‖
mailbox ‖ S public ‖ C public`), of the JSON `{"v":1,"relayUrl","accountNumber","vaultId","keys":
[{"epoch","key"}]}`, read strictly (unknown fields refused, `v` 1, at most 64 keys and 16 KiB). The
cancel marker is exactly `0x01 0x00`, checked on either slot before parsing. The role byte is not in
the HKDF info: a relay that flips it only stops the pairing (`syncPairingBothOn`/`BothOff`, or both
sides waiting until expiry). The `hkdf` crate's PRK is not zeroized; the shared secret, the output
and the X25519 secret (an `Option` taken at derive) are. The vectors in `pairing/tests.rs` are the
contract with every other device.

*Amended 2026-11-12 (ADR 0027): protocol 2.* The QR text starts `moony-pair:2:` and slot 1 is
`0x02 ‖ role ‖ C public`. The HKDF info is unchanged, but 72 bytes are taken: the session key
(0..32), the code (32..40, as above) and an acknowledgement key (40..72). The shower, after reading
slot 1, writes slot 3 = `0x01 ‖ HMAC-SHA256(ack key, "shower->scanner" ‖ mailbox ‖ S public ‖ C
public)` (33 bytes); the scanner shows anything only after it verifies. Only the joiner shows the
six digits; the person types them on the holder, which checks them in Rust (three attempts), so
"They match" and "They don't match" no longer exist and abandoning is "Cancel". Slot 2 and the
cancel marker are unchanged. A `moony-pair:1:` code is refused ("the other device needs an
update"), a version `0x01` slot 1 ends the pairing, and a sync server without slot 3 answers "this
sync server needs an update". The
vectors in `pairing/tests.rs` now include the acknowledgement.

**The channel the code relies on (PF2).** The six digits protect the exchange only because S's
public key reaches C over a channel the attacker cannot modify: a QR on a screen the person is
looking at. Then S's key needs no commitment: the relay and anyone who photographs the QR can only
guess, one chance in 10^6 per pairing. Over a channel the attacker can modify (a pasted or typed
code, a chat message), an attacker who swaps in their own key sees both public keys before choosing
the one it writes to slot 1, and grinds a matching code offline in seconds. So pasting a pairing
code is a developer tool only: the code's text crosses IPC (`PairingView.pairingCode`), the desktop
offers "Copy code" and the phone "Paste a pairing code — developer tool" only with
`feature::dev_tools()`. Release builds pair by an on-screen QR alone. The Recovery Kit's typed and
pasted fallbacks stay, because the Kit is the secret itself.

**Who shows, who scans, who holds.** The desktop always shows: "Add a device" (sync on) and "Join
from your phone" (sync off). The phone always scans: with sync off it joins, with sync on it sends.
Desktop to desktop, phone to phone, or without a second device at hand, the joiner uses the Kit.
The role follows the device's own sync state (holder with sync on, joiner without) and slot roles
are fixed: slot 1 scanner → shower, slot 2 holder → joiner, which serves both directions. The
mailbox server: a holder-shower uses its stored account's server; a joiner-shower uses the
Advanced field, else a stored account's, else `default_base_url()`; the scanner uses the server
the code names, else the default. The `relayUrl` sealed in slot 2 may name another server than the
mailbox's; the code authenticates it.

**Confirm gates and the password.** Before a QR is shown or slot 1 is written, both roles check the
device password (PF1; empty → `validation.syncPasswordNeeded`), and a joiner gives its device name.
The holder seals slot 2 only after its own person confirmed (`sync_pairing_confirm` reads the
credentials then, with sync still on). The joiner reads slot 2 only after its own person confirmed,
and joins only after that. Nobody polls in Compare. The UI polls every 2 s while it waits (≤ 60
calls a minute, under the relay's 120 per address).

**Lost answers and contested scans (PF3, PF4).** A put whose answer was lost (any network failure
but `connect`, which sent nothing, and every 5xx) makes that slot ambiguous; a later `slot_taken` on
it is our own write. Without that, `slot_taken` on slot 1 means someone answered first: the
scanner reads slot 1 once (the cancel marker → "cancelled"), otherwise it shows its own code, which
cannot match, with a warning and only "Cancel". Confirming it is `syncPairingCodeUsed`, which tells
the person to choose "They don't match" on the other device. It also puts the cancel marker into
slot 2 once, best effort, so a holder-shower whose person confirms the impostor's code fails its
write, and a joiner-shower reads "cancelled". A scanner whose announce failed for the moment stays
in `announcing` and every poll retries. Slots are read once, so a lost read answer ends the pairing
(`syncPairingFailed`; start again). Abandoning ("They don't match", closing, any ending error) puts
the cancel marker into every slot this side has not written, then deletes the mailbox, within 3 s,
best effort; a holder in Sent sends nothing, because the joiner may still read slot 2.

**Lifetime.** A pairing lives 600 s on the device (the relay's mailbox 10 minutes from its first
write). Expiry is sticky: once a call is at or past `wiped_at`, every later call answers
`syncPairingExpired` whatever its clock. Received credentials stay `RECEIVED_GRACE_SECS` (300 s)
longer for "Try again", and a task spawned at receipt wipes them at `wiped_at`, sleeping in ticks of
at most 30 s against the wall clock, so a machine that slept keeps nothing longer. One pairing at a
time, bound to the profile session that started it (`KitOwner`); `forget_pairing` gives it up at
lock, restore and Delete Account, and at turn-off and deleting the synced data only when sync was
on (a holder needs sync on; cancelling a half-finished setup must keep a joiner's session). It
never waits behind a running join, which gives the pairing up when it ends.

**The join.** `join_new_device` checks the name and server, then under `engine::exclusive`: sync
off, and no setup of another account or on another server stored (`syncJoinSetupPending`, PF5),
both before anything is written; then the verified backup (`sync-repair-<unix>.zip`, read back),
then the replace of ADR 0025 under a new device ID, and the binding. The command runs it as a sync
change (`guarded_change`) on a copy of the credentials; a momentary failure (offline, a relay
failure, a rate limit), `syncChangeInProgress` or `syncJoinSetupPending` keep the received session,
anything else ends it. The error key cannot tell which (`syncRelayError` is both), so after a failed
join both UIs ask `sync_pairing_poll` (`joinFailureAction` in `src/utils/pairing-code.ts`): stage
`received` offers "Try again", which joins again, and on `syncJoinSetupPending` also "Cancel setup"
(turn-off with sync off, then the join again); "not started" or "expired" offers "Start again". The
busy modal (desktop) and the busy cover (phone, which also holds back navigation and the edge
swipe) block edits until the replace ends. On the phone a scanned Kit QR joins only after a confirm
sheet that shows its account number (in groups of four) and sync server; a typed code has its
fields on screen already. The Kit join checks the password,
then (PF7) refuses with sync on before any relay call, asks `account_status`
(`syncRecoveryAccountInvalid`), opens the vault with the Kit's key as epoch 0 (`vault_info`;
`syncRecoveryVaultNotFound`) and refuses a vault whose `rotation` is not 0
(`syncRecoveryKitRotated`, rotation is phase 5); a typed code needs the account number and the
server defaults to `default_base_url()`.

**Device list.** `sync_list_devices` lists the rows not removed: this device first, then newest
change first, then by name, with a "Changes held" badge from `sync_heads.quarantined`. "Last change
synced" is derived, never stored: for this device its last successful sync, for another the newest
clock it wrote (`MAX(hlc)` per device in `sync_clock`). A pull-only device shows none, and a device
whose every edit was overwritten shows an older time. Renaming this device is a captured write
(sync on only). Old rows of re-joined or turned-off devices stay listed until removal (phase 5, Q2).

**The flag on a phone.** An iPhone app gets no environment, so a debug build made with
`MOONY_DEV_SYNC=1` (and `MOONY_RELAY_URL`) carries them (`option_env!`, behind
`cfg(debug_assertions)`; the run-time value wins). A release build reads neither, at build or run
time: `a_release_build_ignores_the_dev_variables` runs under `cargo test --release` with both set.
Xcode's build phase does drop the variables (seen in the 2026-11-10 simulator run), and a
`src-tauri/.cargo/config.toml` is not picked up either; what works is an uncommitted, gitignored
`.cargo/config.toml` at the repository root whose `[env]` entries use `force = true`
(`MOONY_DEV_SYNC = { value = "1", force = true }`), plus a touch of `services/sync/feature.rs` so
cargo rebuilds. Delete it afterwards: it also turns sync on for every debug build and test run in
that checkout (PF10).

**iOS.** The binding ports the macOS design byte for byte: a Secure Enclave P-256 key-agreement key
with `.privateKeyUsage` and `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` (no prompt; the binding
is checked only after unlock), ECIES blob `0x42`, the key handle in the blob. Swift reports the
stage and the error chain; Rust classifies it (PF11, `classify_ios_binding`): a failed seal and a
momentary code anywhere in the chain (a locked phone's −25308 among them) are `Failed`; anything
else, unknown errors included, is `NotThisDevice`, as on macOS. The Xcode 27 simulator has a Secure
Enclave, so the binding is written and checked there too; the XCTest self-test also runs the ECIES
with a software key. The recovery code goes on the pasteboard `.localOnly` with a 60 s
`.expirationDate`; the clear at lock or in the background can miss (iOS may not keep `changeCount`
fresh there), and the expiry is the backstop. `NSCameraUsageDescription` now names scanning pairing
codes and the Recovery Kit's QR, and `NSLocalNetworkUsageDescription` covers a sync server on the home network, in all ten
languages. The barcode scanner runs `windowed: true` under the app's own overlay with five named
permissions. Android answers `Unsupported` for the binding and the clipboard (the UI copies and
asks the person to clear it).

**Phone setup (Q1).** The phone never opens a checkout (spec §16a Q4). It calls `startCheckout` only
to learn whether the server is self-hosted: a self-hosted server shows the `ALLOWED_ACCOUNTS`
instruction and waits; the default server says paying is not available in the phone app and waits
for the account to become active. On the default server that call leaves an unused checkout session
behind, to be replaced by a relay flag before release.

## Consequences

- The pairing lives in `services/sync/pairing/` (bytes in `mod.rs`, the state machine in
  `session.rs`, which has no clock and never retries on its own); `commands/sync_pairing.rs` keeps
  the one slot, the password gate and the wiper. User copy says "sync server", never "relay".
- Known limits, accepted:
  - an ambiguous put followed by `slot_taken` counts as ours even when an impostor wrote that slot:
    the codes then differ, but the scanner shows no warning and closes no slot 2, and a holder
    reports Sent though the joiner may receive nothing;
  - a lost read answer ends the pairing (the slot is gone once read), and the person starts again;
  - scanning needs the camera (a refusal links to Settings); a phone cannot show a code, so two
    phones or two desktops join with the Kit;
  - a typed Kit account is not tied to the vault by the relay (it only drives the payment status);
  - `Credentials.account` is a plain `String`, not zeroized (phase 5 hardening), and the HKDF PRK
    is not zeroized;
  - old device rows stay until removal; removal and key rotation are phase 5, so a rotated vault
    cannot be joined with a Kit;
  - Android is not built: its binding and clipboard answer `Unsupported`;
  - `cargo audit` reports RUSTSEC-2026-0459 (libsqlite3-sys), which predates this phase; check it at
    the release pre-flight.
- Owner checks owed on an iPhone (a debug build made with `MOONY_DEV_SYNC=1`): the Secure Enclave
  seal and open (`binding_written = 1` after a join); the error a blob from another iPhone gives
  (the log line `bindOpen failed at restore: …`); a locked or background start answers −25308 and
  reads `Failed`; the `.localOnly` clipboard expiring after 60 s and clearing at lock; the camera
  scan of the desktop's QR in both directions (the phone joining from the desktop's "Add a device",
  and the phone's "Add a device" scanning the desktop's "Join from your phone"); a pairing join cut
  by a network drop (Wi-Fi off while "Joining…" shows, on the phone and on the desktop), which must
  offer "Try again", not "Start again", and join once the network is back; a real-camera scan of a
  Recovery Kit QR, which must show the confirm sheet with the Kit's account number and sync server
  before anything is replaced; the camera and local-network permission texts. Also the unused
  checkout session above, before release.
