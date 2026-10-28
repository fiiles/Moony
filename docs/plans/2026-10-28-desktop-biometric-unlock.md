# Desktop Biometric Unlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unlock the desktop app with Touch ID (macOS) and Windows Hello (Windows) on cold start and after auto-lock, the password staying the master secret.

**Architecture:** The phone's `services/biometric_unlock.rs` + `commands/biometric.rs` + `KeychainLike` stay the single code path. The `moony-keychain` plugin's desktop stub gets real backends: macOS keeps the master key wrapped by a Secure Enclave P-256 key with `biometryCurrentSet` access control (CryptoKit via `swift-rs`, no entitlement, no provisioning profile — spike 2026-10-28 confirmed key creation in an ad-hoc-signed binary and that key agreement without user interaction fails with LAError -1004); Windows wraps it with a key derived from a Windows Hello (`KeyCredentialManager`) signature. Both write one opaque blob file in the app data dir. Linux keeps the `Unsupported` stub.

**Tech Stack:** Rust (Tauri 2, `swift-rs` 1.0.9, `windows` 0.62, `aes-gcm` 0.11, `hkdf`, `sha2` 0.11), Swift (CryptoKit, LocalAuthentication), React 19 + TanStack Query, i18next (10 locales), Vitest.

**Spec:** `docs/specs/2026-10-28-desktop-biometric-unlock-design.md` (read it first; it carries the owner decisions).

**Worktree:** `/Users/filipkral/Programování/FinanceApp/Moony-tauri-biometric`, branch `feat/desktop-biometric`. Never commit in the shared checkout `../Moony-tauri`. Verify `git branch --show-current` = `feat/desktop-biometric` before every commit.

## Global Constraints

- AGENTS.md's ten rules apply; especially: `invoke()` only in `src/lib/tauri-api.ts`; `shared/schema.ts` 7-step procedure (`docs/standards/type-contract.md`); per-field `#[serde(rename = "camelCase")]` + `bindings.rs` + regenerate `generated-types.ts` (`cd src-tauri && cargo test generate_bindings -- --ignored`); `AppError`, no `.unwrap()` outside tests; every string in all ten locales (`en cs de es it nb pl sv ja zh`).
- The master key never reaches the webview, logs, backups or the profile-transfer export. The desktop capability (`capabilities/default.json`) gets **no** `moony-keychain` permission: the webview reaches biometrics only through the app commands.
- macOS: biometry only — `LAContext.localizedFallbackTitle = ""`, policy `.deviceOwnerAuthenticationWithBiometrics`, access control `[.privateKeyUsage, .biometryCurrentSet]`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`. The Mac login password must never release the key.
- Windows: no DPAPI-gated fallback. Copy says "Windows Hello", never "otisk prstu" alone.
- Owner decision: desktop (macOS + Windows) keeps the master key in `SessionKey` for the whole unlocked session (reverses R22). Linux keeps R22.
- Owner decision: the one-time offer shows once per install (`moony-biometrics-asked` localStorage flag); after any answer, dismissal, invalidation or restore it never comes back by itself. Settings is the only way back.
- Owner decision: macOS and Windows ship together; no release tag in this plan.
- Copy (cs, fixed meaning; wording may be polished): offer title macOS "Odemykat Moony otiskem prstu?", Windows "Odemykat Moony přes Windows Hello?"; buttons "Zapnout" / "Teď ne"; lock-screen button "Odemknout otiskem" / "Odemknout přes Windows Hello"; settings row "Odemykat otiskem prstu" / "Odemykat přes Windows Hello".

## Review Focus

1. **Invalidated key on macOS (fingerprint added/removed).** Expected: one notice on the lock screen, marker "0", blob removed, password form, no offer afterwards. Pinned by service tests (Task 2) and the TS decision test `invalidation never re-offers` (Task 6); the native error mapping is checked by the owner (Task 9).
2. **Biometric unlock on desktop must do everything a password unlock does** (MCP server start when enabled, attachment conversion, mcp refresh). Expected: identical follow-ups. Pinned by routing both commands through one `after_unlock` (Task 2) — reviewer checks no follow-up is missing.
3. **A restored backup must not be openable by an old blob, and must not re-offer.** Expected: restore switches biometric unlock off in Rust, asked flag untouched. Pinned by Task 2's restore hook and Task 7's restore test step.
4. **Lock screen prompt loops.** A cancelled prompt must not reopen by itself on re-render, focus changes or route bounce. Pinned by `shouldAutoPromptDesktop` tests (Task 6).
5. **Linux and machines without biometrics.** Linux: no row, no button, no offer, no command error toasts. No sensor/Hello not set up: disabled row with reason, no offer. Pinned by `biometricSettingState` tests (Task 6) and the status test `unsupported platform reports supported false` (Task 1).

---

### Task 1: Wire types — `supported` flag and `windowsHello`

**Files:**
- Modify: `src-tauri/src/models/biometric.rs`, `src-tauri/plugins/moony-keychain/src/models.rs`, `src-tauri/src/commands/biometric.rs` (`biometry()` mapping), `src-tauri/src/services/biometric_unlock.rs` (`biometric_status`), `shared/schema.ts:33-46`, `shared/generated-types.ts` (regenerated), `src/mobile/lib/biometrics.ts` (`biometryKind` must stay exhaustive)
- Test: `src-tauri/src/services/biometric_unlock.rs` tests module

**Interfaces:**
- Produces: `BiometryType::WindowsHello` (`#[serde(rename = "windowsHello")]`) in app models and plugin models; `BiometricStatus { available, enrolled, biometry_type, reason, supported: bool }` (`#[serde(rename = "supported")]`); TS `BiometryType = 'faceId' | 'touchId' | 'biometric' | 'windowsHello' | 'none'`, `BiometricStatus.supported: boolean`.

- [ ] **Step 1: Write failing tests** in `biometric_unlock.rs`:
  - `status_reports_the_marker_and_the_keychain`: expected struct gains `supported: true`.
  - `unsupported_platform_reports_supported_false`: stub `status: Err(KeychainFailure::Unsupported)` → `supported == false`, `available == false`, `enrolled` follows the marker, `reason == None` (nothing to explain on a platform without the feature).
  - `unavailable_biometrics_are_supported_but_not_available`: stub `status: Ok(KeychainStatus { available: false, biometry_type: BiometryType::None })` → `supported == true`, `available == false`, `reason == Some(UNAVAILABLE_REASON)`.
  - `keychain_failure_other_than_unsupported_is_supported`: stub `Err(KeychainFailure::Failed("x".into()))` → `supported == true`, `available == false`.
- [ ] **Step 2:** `cd src-tauri && cargo test biometric_unlock` → FAIL (no field).
- [ ] **Step 3:** Add the field/variant; `biometric_status` sets `supported = !matches!(err, Unsupported)`. Follow the 7-step procedure in `docs/standards/type-contract.md` for `shared/schema.ts`; regenerate bindings. `biometryKind('windowsHello')` returns `'generic'` for the phone.
- [ ] **Step 4:** `cargo test biometric_unlock`, `cargo test generate_bindings -- --ignored`, `npm run typecheck`, `npm test -- biometrics` → PASS.
- [ ] **Step 5: Commit** `feat(biometric): supported flag and Windows Hello biometry type`.

### Task 2: Desktop wiring in the app crate

**Files:**
- Modify: `src-tauri/Cargo.toml` (move `moony-keychain` from the mobile-only table to `[dependencies]`), `src-tauri/src/lib.rs` (register `moony_keychain::init()` for every platform; drop `#[cfg(mobile)]` on the four `commands::biometric::*` handlers), `src-tauri/src/commands/mod.rs` (un-gate `pub mod biometric` if gated), `src-tauri/src/commands/biometric.rs` (module doc: phone and desktop), `src-tauri/src/services/auth.rs:185-192` (`keep_session_key`), `src-tauri/src/services/session_key.rs` (doc), `src-tauri/src/commands/auth.rs` (`unlock`, `delete_account`, new `after_unlock`), `src-tauri/src/commands/backup.rs` (`restore_backup`), `src-tauri/plugins/moony-keychain/src/lib.rs` (doc)
- Test: `src-tauri/src/services/auth.rs` tests

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `pub(crate) async fn after_unlock(app: &AppHandle, db: &Database)` in `commands/auth.rs` (MCP server start when `profile.mcp_server_enabled` on desktop, `attachments::spawn_conversion`, `mcp_refresh::resume`) — takes the profile too if needed: `after_unlock(app: AppHandle, db: &Database, profile: &UserProfile)`. `pub(crate) async fn switch_off(app: &AppHandle)` in `commands/biometric.rs` (best-effort `biometric_unlock::disable`, logs on error).

- [ ] **Step 1: Failing test** in `services/auth.rs`: `#[cfg(any(target_os = "macos", target_os = "windows"))] fn desktop_unlock_keeps_the_session_key()` — unlock a test profile through `unlock_with_master_key` (follow the existing `change_password_keeps_the_master_key_biometric_unlock_holds` setup) and assert `session.hex().is_some()`. Plus `#[cfg(target_os = "linux")] fn linux_unlock_keeps_no_session_key()` asserting `None`.
- [ ] **Step 2:** `cargo test desktop_unlock_keeps` → FAIL.
- [ ] **Step 3:** `keep_session_key` stores under `#[cfg(any(mobile, target_os = "macos", target_os = "windows"))]`; rewrite the R22 comments to "owner ruling 2026-10-28: macOS/Windows keep it too (biometric enrolment); Linux has nothing to enable". `unlock` and `unlock_with_biometrics` both end with `after_unlock`. `remove_leftover_key` runs after password `unlock` and `delete_account` on every platform (drop the `#[cfg(mobile)]`). `restore_backup` calls `crate::commands::biometric::switch_off(&app).await` after a successful `backup::restore_backup` (all platforms; the phone's TS also disables before — harmless twice). `confirm_recover` keeps the master key (recovery re-wraps the same key — verify by reading `auth::confirm_recover`; if it can produce a different master key, call `switch_off` there too).
- [ ] **Step 4:** `cargo test`, `cargo clippy -- -D warnings` → PASS; `npm run tauri dev` is not needed yet.
- [ ] **Step 5: Commit** `feat(biometric): biometric unlock commands on desktop, session key kept on macOS/Windows`.

### Task 3: Plugin desktop backend skeleton + blob file store

**Files:**
- Create: `src-tauri/plugins/moony-keychain/src/desktop/mod.rs` (replaces `desktop.rs`), `src-tauri/plugins/moony-keychain/src/desktop/blob_file.rs`
- Modify: `src-tauri/plugins/moony-keychain/src/lib.rs` (doc + `desktop::init` gets the data dir), `Cargo.toml` of the plugin (`tempfile` dev-dep)
- Test: `blob_file.rs` tests

**Interfaces:**
- Produces:
  - `blob_file::path(dir: &Path, key: &str) -> PathBuf` → `dir.join(format!("{key}.bin"))` (`moony.master-key.bin`).
  - `blob_file::write(dir: &Path, key: &str, bytes: &[u8]) -> std::io::Result<()>` — temp file in the same dir + rename; on unix mode `0o600`.
  - `blob_file::read(dir: &Path, key: &str) -> std::io::Result<Option<Vec<u8>>>` — `None` when missing.
  - `blob_file::remove(dir: &Path, key: &str) -> std::io::Result<()>` — missing is `Ok`.
  - `trait NativeBiometrics { fn status(&self) -> Result<Status>; fn wrap(&self, secret: &[u8; 32], reason: &str) -> Result<Vec<u8>>; fn unwrap(&self, blob: &[u8], reason: &str) -> Result<Zeroizing<[u8; 32]>>; fn forget(&self) -> Result<()>; }` in `desktop/mod.rs`; `Keychain<R>` holds `data_dir: PathBuf` (`app.path().app_data_dir()`, same dir as `commands::auth::get_db_path`) and `native: Box<dyn NativeBiometrics + Send + Sync>`; Linux's native returns `Err(KeychainError::Unsupported)` from every method.
  - `Keychain::store(key, value_hex, reason)`: hex → 32 bytes (`KeychainError::Failed` on bad hex), `native.wrap`, `blob_file::write`. `retrieve(key, reason)`: no file → `Ok(None)`; else `native.unwrap` → lowercase hex. `remove(key)`: `native.forget()` (ignore `Unsupported`), then `blob_file::remove`. `status()` → `native.status()`. `set_privacy_screen` / `exclude_from_backup` stay `Unsupported`.
- `zeroize` becomes a plugin dependency.

- [ ] **Step 1: Failing tests** in `blob_file.rs` (tempdir): `write_then_read_round_trips`, `read_missing_is_none`, `remove_missing_is_ok`, `write_replaces_atomically_and_leaves_no_temp_file` (dir has exactly one entry after two writes), `#[cfg(unix)] written_file_is_private` (mode `0o600`). In `desktop/mod.rs`: a `FakeNative` (XOR wrap) proving `store`→`retrieve` returns the same hex, `retrieve` without file is `Ok(None)`, `remove` deletes the file, bad hex is `Failed`. Replace the old `every_desktop_call_is_unsupported` test with `linux_native_is_unsupported` on the Linux native.
- [ ] **Step 2:** `cargo test -p moony-keychain` → FAIL.
- [ ] **Step 3:** Implement. Platform selection: `#[cfg(target_os = "macos")] mod macos; #[cfg(target_os = "windows")] mod windows; #[cfg(target_os = "linux")]` unsupported native. Until Tasks 4/5 land, macOS/Windows use the unsupported native too (a `todo!()` is not allowed).
- [ ] **Step 4:** `cargo test -p moony-keychain`, `cargo test`, `cargo clippy --workspace -- -D warnings` → PASS.
- [ ] **Step 5: Commit** `feat(keychain): desktop backend skeleton with a private blob file`.

### Task 4: macOS Touch ID backend (Swift + CryptoKit)

**Files:**
- Create: `src-tauri/plugins/moony-keychain/macos/Package.swift`, `src-tauri/plugins/moony-keychain/macos/Sources/MoonyBiometrics/MoonyBiometrics.swift`, `src-tauri/plugins/moony-keychain/src/desktop/macos.rs`
- Modify: plugin `build.rs` (link the Swift package only when `CARGO_CFG_TARGET_OS == "macos"`: `swift_rs::SwiftLinker::new("10.15").with_package("MoonyBiometrics", "macos/").link()`), plugin `Cargo.toml` (`swift-rs = "1.0.9"` as a macOS target dependency and build-dependency with feature `build`), `src-tauri/tauri.conf.json` (`bundle.macOS.minimumSystemVersion: "10.15"` only if linking demands it)
- Test: `macos.rs` tests

**Interfaces:**
- Consumes: Task 3's `NativeBiometrics`.
- Swift exports (`@_cdecl`, swift-rs types), every result an `SRData` whose first byte is a status code — `0` ok, `1` cancelled, `2` unavailable, `3` invalidated, `4` failed — followed by the payload (ok) or a UTF-8 message (failed):
  - `moony_bio_status() -> Int` — `-1` no usable biometrics, `0` biometrics present but `biometryType` none, `1` Touch ID (anything else maps to `1` on a Mac).
  - `moony_bio_wrap(reason: SRString, secret: SRData) -> SRData` — first `LAContext.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason:)` (the enable confirmation, fallback title `""`), then a new `SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl:)`, ECIES wrap; payload = blob.
  - `moony_bio_unwrap(reason: SRString, blob: SRData) -> SRData` — restore the SE key with an `LAContext` (`localizedReason = reason`, `localizedFallbackTitle = ""`), key agreement prompts Touch ID; payload = 32-byte secret.
  - `moony_bio_selftest() -> Bool` — the same wrap/unwrap code with a software `P256.KeyAgreement.PrivateKey` instead of the SE key; round-trips a fixed secret.
- Blob v1 (built and parsed only in Swift): `0x01 | u16 BE len | SE key dataRepresentation | ephemeral public key x963 (65 B) | AES.GCM combined (12 + 32 + 16 B)`. Key = HKDF<SHA256>(shared secret, salt = ephemeral x963 ‖ SE public x963, info = `"moony-touchid-v1"`, 32 B).
- Error mapping in Swift: `LAError.userCancel/.appCancel/.systemCancel/.userFallback` → 1; `.biometryNotAvailable/.biometryNotEnrolled/.biometryLockout/.passcodeNotSet` → 2; key restore or agreement failing with anything else after a successful prompt context (including `CryptoKitError` and `errSecItemNotFound`-style failures) → 3; malformed blob → 3; `.authenticationFailed` → 4; other → 4 with the message. On Rust side `decode(status_byte)` → `KeychainError::{Cancelled, Unavailable, Invalidated, Failed(msg)}`.
- `MacNative::forget()` is a no-op (the SE key lives inside the blob; removing the file destroys it).

- [ ] **Step 1: Failing tests** in `macos.rs`: `decode_maps_every_status_byte` (0 with payload → Ok, 1→Cancelled, 2→Unavailable, 3→Invalidated, 4+"boom"→Failed("boom"), empty data → Failed), `native_selftest_round_trips` (calls `moony_bio_selftest()`, asserts `true`), `status_maps_codes` (pure mapping fn: -1 → available false/None, 1 → available true/TouchId).
- [ ] **Step 2:** `cargo test -p moony-keychain` → FAIL.
- [ ] **Step 3:** Implement Swift + Rust glue. `MacNative::status` → `Status { available, biometry_type: TouchId }`. Keep the secret out of Swift `String`s (use `Data`) and zero Rust copies (`Zeroizing`).
- [ ] **Step 4:** `cargo test -p moony-keychain`, `cargo test`, `cargo clippy --workspace -- -D warnings` → PASS. Add one `#[ignore]` test `mac_status_on_this_machine` that prints `MacNative.status()`; run it with `-- --ignored` → expected `available: true, TouchId` on this Mac. Never write a test that calls `wrap`/`unwrap` on the real Secure Enclave (it would wait for a finger).
- [ ] **Step 5: Commit** `feat(keychain): Touch ID unlock on macOS with a Secure Enclave key`.

### Task 5: Windows Hello backend + Windows CI job

**Files:**
- Create: `src-tauri/plugins/moony-keychain/src/desktop/hello_wrap.rs` (pure, compiled `#[cfg(any(target_os = "windows", test))]`), `src-tauri/plugins/moony-keychain/src/desktop/windows.rs`
- Modify: plugin `Cargo.toml` (`aes-gcm = "0.11"`, `hkdf` (version compatible with `sha2 0.11`), `sha2 = "0.11"`, `rand = "0.10"`; Windows target dep `windows = { version = "0.62", features = ["Security_Credentials", "Security_Cryptography", "Storage_Streams", "Foundation", "Win32_UI_WindowsAndMessaging", "Win32_Foundation"] }` — adjust to what compiles), `.github/workflows/ci.yml` (new job)
- Test: `hello_wrap.rs` tests

**Interfaces:**
- Consumes: Task 3's `NativeBiometrics`.
- Produces in `hello_wrap.rs`:
  - `pub fn new_challenge() -> [u8; 32]`
  - `pub fn seal(challenge: &[u8; 32], signature: &[u8], secret: &[u8; 32]) -> Vec<u8>` — blob v1: `0x01 | challenge (32) | nonce (12) | ciphertext+tag (48)`; key = HKDF-SHA256(ikm = signature, salt = challenge, info = `"moony-hello-v1"`) → AES-256-GCM.
  - `pub fn challenge_of(blob: &[u8]) -> Option<[u8; 32]>`
  - `pub fn open(blob: &[u8], signature: &[u8]) -> Result<Zeroizing<[u8; 32]>, OpenError>` with `OpenError::{Malformed, WrongKey}`.
- `WinNative` uses credential name = the app identifier (pass `app.config().identifier` into `desktop::init`): `status` = `KeyCredentialManager::IsSupportedAsync` → `Status { available, biometry_type: WindowsHello }`; `wrap` = `RequestCreateAsync(name, ReplaceExisting)` (the Hello prompt is the enable confirmation) → `RequestSignAsync(challenge)` → `seal`; `unwrap` = `OpenAsync(name)` → `RequestSignAsync(challenge_of(blob))` → `open`; `forget` = `DeleteAsync(name)` (NotFound is Ok). Status mapping: `UserCanceled` → Cancelled; `NotFound` / `WrongKey` / `Malformed` → Invalidated; `SecurityDevice`/`UnknownError` → Failed; Hello not set up → Unavailable. Bring the Hello dialog to the front: while the async op runs, a helper thread polls (≤ 3 s, 100 ms) for a top-level window of class `Credential Dialog Xaml Host` and calls `SetForegroundWindow` on it.
- CI job `rust-windows` in `ci.yml`: `runs-on: windows-latest`, checkout, Rust toolchain, Node + `npm ci` + `npm run build` if `tauri::generate_context!` needs `dist`, then `cd src-tauri && cargo clippy --workspace -- -D warnings && cargo test --workspace`. Copy setup steps from the existing macOS job.

- [ ] **Step 1: Failing tests** in `hello_wrap.rs` (run on every OS): `seal_open_round_trips`, `open_with_other_signature_is_wrong_key`, `open_truncated_is_malformed`, `open_unknown_version_is_malformed`, `challenge_of_reads_back_the_challenge`, `two_seals_use_different_nonces`, `blob_never_contains_signature_or_secret` (search the blob bytes for both).
- [ ] **Step 2:** `cargo test -p moony-keychain hello_wrap` → FAIL.
- [ ] **Step 3:** Implement `hello_wrap.rs`, then `windows.rs`.
- [ ] **Step 4:** `cargo test -p moony-keychain` → PASS on the Mac. Check the Windows code compiles: `rustup target add x86_64-pc-windows-msvc && cd src-tauri && cargo check -p moony-keychain --target x86_64-pc-windows-msvc`. If the cross-check cannot run on macOS (a C build script), say so in the report; the CI job is the gate then. YAML-check `ci.yml` (`npx --yes yaml-lint` or `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml`).
- [ ] **Step 5: Commit** `feat(keychain): Windows Hello unlock and a Windows CI job`.

### Task 6: Shared TS helpers for desktop biometrics

**Files:**
- Move: `src/mobile/lib/biometrics.ts` → `src/lib/biometrics.ts` and its test (`git mv`); update the 12 importers under `src/mobile/` to `@/lib/biometrics`.
- Modify: `src/lib/biometrics.ts` (new desktop helpers), `src/lib/tauri-api.ts:242` (comment: phones and desktop)
- Test: `src/lib/biometrics.test.ts`

**Interfaces:**
- Produces (in `src/lib/biometrics.ts`):
  - `export type BiometricSettingState = 'hidden' | 'unavailable' | 'ready';` and `biometricSettingState(status: BiometricStatus | undefined): BiometricSettingState` — `undefined` or `!supported` → `'hidden'`; `!available && !enrolled` → `'unavailable'`; else `'ready'`.
  - `export type DesktopBiometryLabel = 'touchId' | 'windowsHello' | 'generic';` and `desktopBiometryLabel(type: BiometryType): DesktopBiometryLabel`.
  - `shouldAutoPromptDesktop(input: { lockEvent: number; lastPrompted: number | null; focused: boolean; enrolled: boolean }): boolean` — `enrolled && focused && shouldAutoPrompt(lockEvent, lastPrompted)`.
  - `afterBiometricFailure(failure: BiometricFailure): { showPassword: boolean; reoffer: false }` is **not** needed — instead pin "invalidation never re-offers" through `shouldOfferEnrolment` with `asked = true` (the desktop never calls `clearEnrolmentAsked` on invalidation or restore; only after `delete_account`).

- [ ] **Step 1: Failing tests:** `biometricSettingState` (hidden for undefined; hidden for `supported:false`; unavailable for `{supported:true, available:false, enrolled:false}`; ready for `{available:false, enrolled:true}` so the person can still switch it off; ready for available), `desktopBiometryLabel` (`touchId`→touchId, `windowsHello`→windowsHello, `faceId`/`biometric`/`none`→generic), `shouldAutoPromptDesktop` (false when unfocused; false when not enrolled; true once per lock event; false again for the same event after a prompt), `invalidation never re-offers` (`shouldOfferEnrolment({available:true, enrolled:false,…}, true) === false`).
- [ ] **Step 2:** `npm test -- biometrics` → FAIL.
- [ ] **Step 3:** Implement; update imports.
- [ ] **Step 4:** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `refactor(biometric): share the biometric helpers with the desktop`.

### Task 7: Desktop lock screen, one-time offer, Settings switch, i18n

**Files:**
- Create: `src/components/auth/BiometricUnlockButton.tsx`, `src/components/auth/EnableBiometricsDialog.tsx`, `src/components/settings/BiometricUnlockCard.tsx`, `src/hooks/use-biometric-status.ts`, `src/hooks/use-desktop-biometric-enrolment.ts` (or generalise `src/mobile/hooks/use-biometric-enrolment.ts` into `src/hooks/` if the namespaces allow — builder's choice)
- Modify: `src/pages/auth-page.tsx` (unlock panel), `src/hooks/use-auth.tsx:58-59, 268-275` (doc comments: phones and desktop), `src/components/settings/SettingsSectionPages.tsx:31-39` (`SecuritySection`: card after `AutoLockSlot`), the desktop shell where the offer dialog mounts after a password unlock (`src/App.tsx` / `AppLayout`), the desktop account-deletion success path (`clearEnrolmentAsked`), `src/i18n/locales/*/auth.json` and `*/settings.json` for all ten locales
- Test: none beyond Task 6 (components and hooks are out of the testing policy) — verified in Task 9

**Behaviour (from the spec):**
- Lock screen: when `status.enrolled`, show the biometric button above the password form (label by `desktopBiometryLabel`; icon `Fingerprint` from lucide for Touch ID, `ScanFace` or `KeyRound` for Windows Hello). Auto-prompt via `shouldAutoPromptDesktop` with `lockEvent = lockMutation.submittedAt` (0 on cold start), `focused = document.hasFocus()`, re-evaluated on the window `focus` event. The prompt reason is a translated string ("odemknout Moony"). Failure handling reuses `biometricFailure` / `biometricNotice`: cancelled → nothing; invalidated → the notice "Otisky prstů se změnily, odemykání otiskem je vypnuté. Zapnete ho znovu v Nastavení." (Windows variant mentions Windows Hello) and refetch status; failed → short notice; the password form is always there.
- Offer: mounted in the unlocked desktop shell; open when the unlock that just happened was a **password** unlock (`unlockMutation.isSuccess` in this session, not biometrics, not setup, not recovery) and `shouldOfferEnrolment(status, wasEnrolmentAsked(localStorage))`. Any close path calls `markEnrolmentAsked`. "Zapnout" → `biometricApi.enable(reason)`; success toast "Odemykání otiskem je zapnuté"; cancel/fail toast "Odemykání otiskem se nezapnulo". Uses the design-system `Dialog` (check `docs/design-system/README.md`).
- Settings card `BiometricUnlockCard`: `biometricSettingState` → `hidden` renders nothing; `unavailable` renders the row disabled with hint (macOS: "Tento Mac nemá Touch ID nebo v něm není přidaný otisk.", Windows: "Windows Hello není nastavené. Nastavíte ho v Nastavení Windows → Účty → Možnosti přihlášení."); `ready` renders a `Switch` bound to `status.enrolled` → enable/disable mutations, both invalidate `BIOMETRIC_STATUS_KEY` and call `markEnrolmentAsked`.
- Account deletion (desktop) calls `clearEnrolmentAsked` so a new account is offered again.
- Restore: no TS change needed (Rust switches off; asked flag untouched) — confirm `src/components/common/RestoreBackup.tsx` does not clear the flag.
- i18n: all new keys in all ten locales; run `npm test -- locales`.

- [ ] **Step 1:** Implement the hook(s) and components; wire them.
- [ ] **Step 2:** `npm run typecheck && npm run lint && npm test && npm run format:check` → PASS.
- [ ] **Step 3:** Smoke in the dev app on macOS (worktree port 1422, identifier override `com.moonyfinance.desktop.biotest`, per the dev-server memory): first run → set up a throwaway profile → lock → password unlock → offer appears once → "Teď ne" → lock/unlock → no offer → Settings shows the switch. A dev build may report biometrics unavailable (unsigned) — then the row shows the unavailable state, which is also a valid check. Stop the dev app afterwards and remove `~/Library/Application Support/com.moonyfinance.desktop.biotest`, `~/Library/WebKit/com.moonyfinance.desktop.biotest`.
- [ ] **Step 4: Commit** `feat(biometric): Touch ID / Windows Hello on the desktop lock screen and in Settings`.

### Task 8: Docs

**Files:**
- Create: `docs/architecture/decisions/0021-desktop-biometric-key-storage.md` (add to `decisions/README.md` index)
- Modify: `PRIVACY.md` (where the device-bound copy lives: macOS — a blob in the app data folder only this Mac's Secure Enclave can open after Touch ID; Windows — a blob opened by a Windows Hello signature; never in backups or exports), `docs/architecture/overview.md` (auth section), `CHANGELOG.md` (`[Unreleased]`), spec status line → "approved 2026-10-28, implemented on feat/desktop-biometric"
- ADR content: Context (owner's typing problem), Decision (SE + CryptoKit, no entitlement — spike result; Windows signature-derived key, not DPAPI; R22 reversed for macOS/Windows with the reasoning from the spec; one-time offer semantics), Consequences (dev builds; Windows PIN accepted; fingerprint change invalidates on Mac only).

- [ ] **Step 1:** Write; `npm run format:check`.
- [ ] **Step 2: Commit** `docs: ADR 0021 desktop biometric unlock, privacy and changelog`.

### Task 9: Built-app verification and handover

- [ ] **Step 1:** Full gates: `npm run lint && npm run typecheck && npm test && npm run format:check`, `cd src-tauri && cargo clippy --workspace -- -D warnings && cargo test --workspace`.
- [ ] **Step 2:** Build a Developer-ID-signed debug bundle with its own identifier: `npm run tauri build -- --debug --bundles app --config '{"identifier":"com.moonyfinance.desktop.biotest","bundle":{"createUpdaterArtifacts":false}}'` (signing identity `Developer ID Application: Filip Kral (4BN52M4YYK)` from the login keychain via `APPLE_SIGNING_IDENTITY`). `codesign --verify --deep --strict` must pass.
- [ ] **Step 3:** Write the owner's test checklist into the final report (not a file): macOS — offer once, enable (Touch ID prompt), cold start → prompt → unlock, auto-lock → prompt, cancel → password works, add a fingerprint in System Settings → invalidated notice + switch off + no offer, Settings off/on, restore backup → off; Windows — the same with Hello (dialog in front, PIN accepted, Hello removed → invalidated). Real-profile dry run before release.
- [ ] **Step 4:** Final whole-branch review (opus) and a fix round. **Do not merge into `main`:** the feature is finished only after the owner's Touch ID and Windows Hello checks (main takes finished work only). Leave `feat/desktop-biometric` ready; the owner says when to merge.
