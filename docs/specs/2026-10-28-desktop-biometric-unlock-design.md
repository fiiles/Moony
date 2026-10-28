# Desktop biometric unlock (Touch ID, Windows Hello) — design

Date: 2026-10-28. Status: approved 2026-10-28, implemented on feat/desktop-biometric.

## Intent

The owner types the Moony password every time the desktop app opens and every time it comes back
from auto-lock (default 15 min idle), and often mistypes it and has to repeat. He wants to unlock
with a fingerprint instead: Touch ID on macOS, Windows Hello on Windows.

The phone app already does exactly this (mobile spec §8.1, `services/biometric_unlock.rs`, the
`moony-keychain` plugin): the master key is stored behind biometry and read back from Rust only.
On desktop the plugin is a stub (`plugins/moony-keychain/src/desktop.rs`) that answers
`Unsupported` to every call. This project fills that stub for macOS and Windows and brings the
lock-screen and settings UI to the desktop.

Success: on the owner's Mac, opening Moony (cold start or after auto-lock) needs one touch of the
sensor and no typing; the password still works every time as the fallback.

## Goals

- Unlock with Touch ID (macOS) and Windows Hello (Windows) on cold start and after auto-lock.
- The password stays the master secret. Recovery key, password change, and SQLCipher encryption
  are unchanged.
- One-time offer to switch it on, plus a switch in Nastavení → Zabezpečení.
- The stored key is device-bound and never leaves the machine or the Rust side (never crosses
  into the webview), same as on the phone.

## Non-goals

- Linux. No standard biometric-gated secret store; the option is not shown there at all.
- Biometric confirmation for individual actions (delete account, export, …). Unlock only.
- Sync of the setting between devices. It is per machine.
- Changing the auto-lock behaviour or its options.

## Anti-goals (would make it a failure even if it works)

- **A nagging offer.** The owner explicitly does not want a dialog after every password unlock.
  The offer appears **once**; closing it or answering "Teď ne" means it never comes back on its
  own. Re-enabling is only through Settings.
- **A weaker lock than the password.** On macOS, the Mac login password must not open Moony
  (biometry only, no "Use Password" fallback in the system prompt). On Windows we accept the
  Hello PIN because the platform cannot exclude it (see below) — but we must not fall back to
  anything weaker than Hello itself (no "Hello as a mere UI gate over a DPAPI blob").
- The master key or anything derived from it reaching the webview, logs, backups, or the
  profile-transfer export.

## Behaviour (owner-decided unless marked)

### Lock screen (`src/pages/auth-page.tsx`)

- When biometric unlock is on and available, the lock screen offers it first and keeps the
  password field below it as the fallback.
- *My call:* the system prompt opens automatically when the lock screen appears while the window
  is focused (cold start, auto-lock while the person is at the machine). If the window is not
  focused, it opens on the next focus. A button ("Odemknout otiskem" / "Odemknout přes Windows
  Hello") retries it at any time. The automatic prompt fires at most once per lock-screen
  appearance; after a cancel it waits for the button.
- *My call:* cancel, not recognised, or locked out → the person types the password; no extra
  lockout of our own. Error copy is short and does not blame (mobile i18n keys
  `auth.biometricCancelled` / `auth.biometricFailed` already exist as codes).
- If the stored key no longer works (macOS: fingerprints added/removed; Windows: Hello reset or
  removed; any platform: the key no longer opens this profile), biometric unlock switches itself
  off, the lock screen says once why, and the person uses the password. *My call, following the
  owner's "no nagging" rule:* no re-offer dialog after this; the Settings switch is the way back.

### The one-time offer

- After a successful **password** unlock, if biometrics are available on this machine and
  biometric unlock has **never been decided** on this install, show a dialog: "Odemykat Moony
  otiskem prstu?" (Windows: "…přes Windows Hello?") with Zapnout / Teď ne.
- Any answer, or closing the dialog, records the decision; the dialog never appears again on its
  own. "Zapnout" runs the system prompt and stores the key.
- *My call:* not offered during first-run onboarding; the first password unlock after onboarding
  or after updating to this version is where it appears. Existing users see it once after the
  update.
- "Never decided" reuses the phone's marker file semantics (`biometric-unlock` in the data dir:
  absent = never decided, "0" = off, "1" = on). Writing "0" on dismissal is what keeps the offer
  from returning.

### Settings (`/settings/security`, next to `AutoLockSlot`)

- A switch "Odemykat otiskem prstu" (macOS) / "Odemykat přes Windows Hello" (Windows).
- Turning it on runs the system prompt and stores the key; turning it off removes the stored key.
- Linux: the row is not rendered. *My call:* macOS/Windows without usable biometrics (no Touch ID
  sensor, Hello not set up): the row is shown disabled with one line saying why and, on Windows,
  that Hello is set up in Windows Settings.

### What happens to the stored key in other flows

Mirror the phone's rules, which already exist in `services/auth.rs` and its tests:

- Password change re-wraps the same master key, so biometric unlock keeps working (phone ruling 14).
- Recovery-key flow, restore backup, delete account, logout cleanup: follow what the phone does
  (`remove_leftover_key`, the marker in the data-dir cleanup list at `auth.rs:688-696`). If a
  flow can end with a different master key, biometric unlock must end up off.
- Backups and profile transfer never include the biometric blob or the marker (device-bound).
- *My call:* restoring a backup (any source, this machine or another) deletes the blob, removes
  the stored key, and writes the marker "0": biometric unlock ends up off, and the person turns it
  back on in Settings. No re-offer dialog.

## Platform mechanisms (constraints; the exact API choice is the builder's, after the spike)

### macOS — Touch ID

Requirement: the master key is released only after a successful **biometric** match against the
fingerprints enrolled **when it was stored** (`biometryCurrentSet` semantics), on this Mac only.
No device-password fallback in the prompt (`localizedFallbackTitle = ""` as on iOS).

Two known ways; the builder spikes first and picks:

- **A. Secure Enclave key, no keychain entitlement (preferred if it works).** Create a Secure
  Enclave P-256 key with access control `biometryCurrentSet` (CryptoKit
  `SecureEnclave.P256.KeyAgreement.PrivateKey`), keep its `dataRepresentation` (an opaque blob only
  this Mac's Secure Enclave can use) plus the master key wrapped by an ECIES / ECDH+HKDF+AES-GCM
  scheme in a file in the data dir. Unwrapping needs the SE key, which needs the finger. Believed
  to need no `keychain-access-groups` entitlement and so no provisioning profile; **the spike must
  confirm this in a Developer ID–signed build and in a dev build.**
- **B. Data-protection keychain item with biometric access control.** The same as the iOS plugin
  code. On macOS this needs the data-protection keychain, which needs an `application-identifier`
  / `keychain-access-groups` entitlement, which for a Developer ID app needs an embedded Developer
  ID provisioning profile. That means: a profile created by the owner in the Apple Developer
  portal (team `4BN52M4YYK`), an entitlements file and the profile wired into the Tauri bundle
  config, and the release workflow updated. Fallback if A fails.

*My call on the spike gate:* plan A only. The spike comes first. If A fails, the builder stops
and tells the owner before doing any B work, because B needs the owner's portal step. B would
then be planned as its own phase. If an unsigned `tauri dev` binary cannot use the Secure Enclave,
that is acceptable: dev builds report biometrics as unavailable, and Touch ID is verified on the
signed debug bundle.

Swift is needed either way for A (CryptoKit). The builder decides how to call it from Rust on
desktop (e.g. a small Swift package linked via `swift-rs`, or `objc2-*` crates for B). The
desktop app is already signed with Developer ID and notarized in CI (secrets present since
2026-10-19; local identity `Developer ID Application: Filip Kral (4BN52M4YYK)` exists).

### Windows — Windows Hello

Requirement: the master key is unwrappable only after Windows Hello user verification, with a
TPM-backed key where available.

Mechanism (decided in brainstorming, the KeePassXC pattern): create a Windows Hello credential
with `KeyCredentialManager` (Microsoft Passport, a per-user RSA key in the TPM/Hello container),
sign a random stored challenge with `RequestSignAsync` (PKCS#1 v1.5, deterministic), derive an
AES-256-GCM key from the signature with HKDF-SHA256, and wrap the master key with it. The file in
the data dir holds the challenge, nonce, and ciphertext — never the signature or derived key.
Retrieval signs the same challenge (Hello prompt) and unwraps.

Known facts and risks the builder must plan for:

- Hello always accepts the PIN as well as fingerprint/face. Copy must say "Windows Hello", not
  "otisk prstu" alone.
- Adding a new fingerprint does **not** invalidate the credential on Windows (unlike macOS).
  Accepted.
- Credential deleted / Hello reset → `OpenAsync` fails → treat as invalidated.
- The Hello dialog from a Win32 app can open behind the window or without focus; the builder must
  make it come to the front (known workaround: find and focus the credential dialog window after
  it appears).
- The `windows` crate is already in the dependency tree (0.62.2); add the WinRT features needed.

The pure parts (HKDF derivation, wrap/unwrap, file format) live in Rust and are unit-tested on
every platform with a stub signature.

## Architecture constraints (from the repo's rules)

- Reuse `services/biometric_unlock.rs` and its `KeychainLike` trait unchanged in spirit: the
  desktop backends implement the same `status / store / retrieve / remove` contract, so the
  service, its tests, and the commands in `commands/biometric.rs` serve both phone and desktop.
  Today the commands, plugin registration, and capabilities are `#[cfg(mobile)]`; widen them to
  macOS and Windows desktop, keep Linux on the `Unsupported` stub.
- Thin commands, logic in services, `AppError` (rule 4). Keychain calls run on a blocking thread
  (`with_keychain` already does this).
- `BiometryType` gains a variant for Windows Hello (*my call*: `windowsHello`) → this touches
  `shared/schema.ts`: follow the 7-step type-contract procedure, register in `bindings.rs`,
  regenerate `generated-types.ts` (rules 2, 3).
- `invoke()` only from `src/lib/tauri-api.ts` (rule 1). Reuse the phone's TS wrappers where they
  exist.
- Every new string in all ten locales (rule 8).
- No DB migration: the state is the marker file plus the platform blob, readable while the DB is
  still encrypted.
- The webview's capability set must not let the webview call the plugin's `store`/`retrieve`
  directly (same as `capabilities/mobile.json`).

### Session key on desktop — owner ruling 2026-10-28 (reverses R22)

The phone keeps the master key in memory (`SessionKey`) for the whole unlocked session so it can
be stored behind biometry without asking for the password again. Until now the desktop
deliberately did not (ruling R22 in `auth.rs:187`).

**Owner's decision:** on macOS and Windows the desktop keeps the master key in `SessionKey` for
the whole unlocked session, exactly like the phone. It is cleared on lock, logout, account
deletion, and restore. The one-time offer and the Settings switch then enable biometric unlock
without asking for the password again. Linux keeps R22, because there is nothing to enable there.
The reasoning goes into ADR 0021: an open SQLCipher connection already holds the key in process
memory, so the added exposure is small.

## Testing and verification

- Rust: service tests for the desktop paths (existing `biometric_unlock` tests extended), pure
  tests for the Windows wrap/derivation and the macOS blob format, and `cargo test` on all CI
  platforms. *My call:* `ci.yml` has only ubuntu and macOS jobs today, so this project adds a
  Windows job (`cargo clippy` + `cargo test` on `windows-latest`). The WinRT code then compiles on
  every push, not first at release time.
- TS: pure helpers only (offer-decision logic, platform label choice), per the testing policy.
- macOS: Claude builds a signed bundle with its own debug identifier (see the built-app probe
  memory) and verifies everything but the finger; **the owner verifies the Touch ID prompt, a
  cancel, a fingerprint added in System Settings (→ invalidated), and the password fallback.**
- Windows: **the owner verifies on his Windows machine** (Hello prompt in front, PIN, cancel,
  Hello removed → invalidated, password fallback) before release.
- Real-profile dry run by the owner before release, as with every auth change.

## Docs to update

- New ADR (next number, 0021): desktop biometric key storage (macOS mechanism chosen after the
  spike, Windows signature-derived key, why not DPAPI-gated, the session-key ruling).
- `PRIVACY.md`: where the device-bound copy of the key lives on each platform.
- `docs/architecture/overview.md` auth section, `CHANGELOG.md`.

## Left to the builder

Exact Swift/Rust bridging, file names and format of the platform blobs, the spike's shape,
component structure on the lock screen, focus/visibility event wiring, exact copy wording within
the decided meaning, and whether the macOS path ends up as A or B (B requires a one-time owner
step in the Apple Developer portal — the builder must tell the owner before going that way).

## Release

**Owner's decision:** macOS and Windows ship together in one release, and only after the owner
has verified Windows Hello on his own Windows machine. The work can land on local main in phases,
macOS first, but no release tag goes out until both platforms are verified.

## Calls made in brainstorming (not owner-decided — check these)

- The prompt opens automatically once per lock-screen appearance while the window is focused,
  and a button retries it.
- No extra lockout of our own after failed or cancelled prompts.
- After the key is invalidated: the lock screen shows one notice, the feature switches off, and
  the offer is not shown again.
- No offer during onboarding. Existing users see it once after the update.
- On macOS/Windows without usable biometrics, the Settings row is shown disabled with the reason.
  On Linux it is hidden.
- Restoring a backup switches biometric unlock off.
- macOS: Secure Enclave approach (A) first, with a stop-and-ask before any provisioning-profile
  work (B). Dev builds may report biometrics as unavailable.
- A Windows job is added to `ci.yml`.
- New `BiometryType` variant `windowsHello`.
