# 0021. Desktop biometric unlock: a device-bound copy of the master key

Date: 2026-10-28

## Status

Accepted. Reverses ruling R22 (the desktop keeps no master key after unlock) on macOS and
Windows.

## Context

The owner types the password at every start and after every auto-lock, and mistypes it often. The
phone app already unlocks with Face ID or Touch ID: `services/biometric_unlock.rs` keeps the master
key behind the device biometry through the `moony-keychain` plugin. On the desktop that plugin was
a stub that answered `Unsupported`.

The desktop cannot reuse the phone's storage. An iOS Keychain item with a biometric access control
needs a keychain entitlement on macOS, which in turn needs a provisioning profile; Moony's
desktop builds have neither, and a plain keychain item would not be gated by Touch ID at all. A
spike on 2026-10-28 showed that a Secure Enclave key can be created and used from an ad-hoc-signed
binary without any entitlement. Windows has no keychain with a biometric gate; DPAPI protects a
secret for the Windows account, not for Hello, so a DPAPI blob would open for any process of the
user without a gesture.

Alternatives considered: a keychain item per platform (needs the entitlement on macOS, no biometric
gate on Windows); storing the password (a second secret that outlives a password change); Windows
Credential Locker (opens without Hello).

## Decision

- **The key is wrapped, and the wrapped copy lives in a file.** The plugin's desktop backend
  wraps the 32-byte master key with a platform key that only the biometry can use and writes the
  result to `moony.master-key.bin` in the app data folder (`blob_file.rs`, owner-only, written
  atomically). The blob alone opens nothing.
- **macOS: a Secure Enclave P-256 key through CryptoKit** (`MoonyBiometrics.swift`, linked with
  swift-rs, called from `macos.rs`), created with `[.privateKeyUsage, .biometryCurrentSet]`. The
  master key is sealed with ECIES: an ephemeral P-256 key agreement with the enclave key's public
  half, HKDF-SHA256 with info `moony-touchid-v1`, AES-GCM. Unwrapping needs the enclave to do the
  agreement, which prompts Touch ID (biometry only, no password fallback). No keychain entitlement
  or provisioning profile is needed. CryptoKit and the Swift runtime raise the macOS minimum to
  10.15, which drops macOS 10.13 and 10.14.
- **Windows: a Hello signature as key material** (`windows.rs`, `hello_wrap.rs`). A
  `KeyCredentialManager` credential named by the app identifier signs a random 32-byte challenge;
  HKDF-SHA256 over the signature (salt = challenge, info `moony-hello-v1`) gives the AES-256-GCM
  key. The blob holds a version byte, the challenge, the nonce and the ciphertext, nothing that
  derives the key. Not DPAPI: the key exists only while Hello signs.
- **Linux is unsupported.** Every call answers `Unsupported`, the UI shows nothing and the
  password-only unlock stays.
- **The session keeps the key on macOS and Windows** (owner ruling 2026-10-28, reversing R22).
  `SessionKey` holds the master key for the unlocked session, as on the phone, so switching
  biometric unlock on does not ask for the password again. The open SQLCipher connection already
  holds the same key in process memory, so the added exposure is small. It is cleared on lock,
  logout, account deletion and restore. Linux keeps R22.
- **Only a lost credential switches the feature off.** `Invalidated` (the Secure Enclave key is
  gone or refuses to be used, the Hello credential is gone, or the blob is malformed or does not
  open with the key it is read with) turns biometric unlock off and removes the blob. A cancelled
  prompt, a misread finger, a lockout or any transient error never does: the password stays
  available and the next attempt may succeed.
- **The offer comes once.** After a password unlock where biometric unlock is available and off,
  a dialog offers it. The device-level key `moony-biometrics-asked` in `localStorage` is set by
  any answer and by closing the dialog, and switching the feature on always sets it, so an
  invalidation never brings the offer back; an invalidation and a restore leave the key as it was.
  The way back is the switch in Settings → Security. The lock screen prompts by itself
  only when the app starts (once, for a window in front); after a lock, manual or idle, it stays
  quiet and the system prompt opens only from its button (owner, 2026-10-28). A restore
  switches the feature off in Rust, because the archive carries a different master key; account
  deletion removes the stored key and clears the flag.
- **The marker stays the source of truth.** As on the phone, a `biometric-unlock` file in the data
  folder says on or off, because the lock screen needs it before the database opens.
- **The blob is never exported.** `backup.rs` zips an explicit file list that does not include
  it, and the JSON export is data only.
- **CI.** A `Rust Windows Check` job runs clippy over the workspace and the tests of the keychain
  plugin only; the app's own test binaries need a comctl32 v6 manifest on Windows (follow-up).

## Consequences

- **One touch instead of typing** at start and after auto-lock; the password still works every
  time and remains the recovery path.
- **A changed fingerprint set invalidates the Mac copy** (`.biometryCurrentSet`); the user types
  the password and turns the switch on again. Windows ties the key to the Hello credential, not to
  the enrolled fingers, so a changed finger set is not detected there.
- **Removing Windows Hello does not switch the feature off at once.** Without Hello the plugin
  cannot tell "removed" from "unusable for now" (both answer that Hello is not supported), so each
  lock shows the "failed" notice and the password form while the feature stays on. Once Hello is
  set up again, the old credential is gone (`OpenAsync` answers `NotFound`), which is
  `Invalidated`: the feature switches off and the person turns it on again.
- **Windows Hello also accepts the PIN.** Whoever knows the Hello PIN of the Windows account can
  unlock Moony. This follows from using Windows Hello as the gate.
- **Not DPAPI-gated on Windows, not keychain-gated on macOS.** An attacker with the blob and the
  logged-in user session still needs a Hello or Touch ID gesture, and on macOS the blob is
  useless on any other Mac.
- **Dev and unsigned builds work on macOS**: no entitlement or provisioning profile is involved,
  so `npm run tauri dev` can exercise the real Touch ID path.
- **The master key sits in memory for the whole unlocked session** on macOS and Windows. That
  matches what the open database holds anyway.
- **Time Machine and file-history copies of the blob** are inert: they open only on the machine and
  biometry that made them.
- Windows is built and linted in CI but its interactive Hello flow is checked by hand.
