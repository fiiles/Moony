# Android app — implementation plan (mobile Phase 7, Android half)

**Goal:** the phone app that runs on iOS today runs on Android from the same code, with the same
screens, data model and encryption (spec `docs/specs/2026-10-16-mobile-app-design.md`, ruling 1:
one visual language). The owner asked for it on 2026-11-17 ("po vzoru iphone appky postavit i
android appku").

**Branch:** `feat/android`, worktree `../Moony-tauri-android`, cut from local main 4e4ac42e.

**Toolchain (installed 2026-11-17):** Android Studio's JBR as `JAVA_HOME`, SDK in
`~/Library/Android/sdk` with cmdline-tools (latest), NDK 28.2.13676358, platforms 36 and 37, the
four Rust Android targets. Every `tauri android` command needs:

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home" ANDROID_HOME=~/Library/Android/sdk NDK_HOME=~/Library/Android/sdk/ndk/28.2.13676358
```

`scripts/android-env.sh` sets these (and the NDK's `llvm-ranlib`, below); `npm run android:dev`
and `npm run android:build -- --debug --apk --target aarch64` source it.

Emulator: AVD `Medium_Phone_API_37.0` (arm64, Google Play image). Install and start a debug build:
`adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`,
`adb shell am start -n com.moonyfinance.mobile/.MainActivity`.

## Build notes (found on the first build, 2026-11-17)

- **SQLCipher needs OpenSSL on Android.** Apple platforms use CommonCrypto; Android has no system
  OpenSSL, so `libsqlite3-sys` gets `bundled-sqlcipher-vendored-openssl` for the Android target
  only (`src-tauri/Cargo.toml`).
- **NDK r23+ has no `<triple>-ranlib`.** The vendored OpenSSL's install step calls it; the env
  script points `RANLIB_<target>` at the NDK's `llvm-ranlib`.
- **Non-ASCII checkout path.** tauri-build writes the plugin paths into `tauri.settings.gradle`
  with Rust's `{:?}`, which escapes the combining accents of "Programování" as `\u{301}`; Groovy
  rejects that. `gen/android/settings.gradle` reads the file, unescapes it into the build folder
  and applies the copy. (Worth an upstream issue.)
- **Launcher icon.** Adaptive icon from `src-tauri/icons/ios-source-1024.png`: the art cropped by
  80 px a side (drops the drawn rounded corners), centred on a 1536 px transparent canvas so it
  fills the 72 dp visible area, background `#F7F5F1`; generated with `tauri icon <manifest> -o
  <tmp>` and only `android/mipmap-*` copied into `gen/android/app/src/main/res`.

## What iOS does that Android must match

| Concern | iOS today | Android |
|---|---|---|
| Project | `gen/apple`, committed | `gen/android` from `tauri android init`, committed (build folders ignored) |
| Biometric unlock, privacy screen, status bar | `moony-keychain` Swift | `moony-keychain` Kotlin, written in Phase 1 but never compiled: compile, run, fix |
| Profile out of device backups | `isExcludedFromBackup` on the data folder | `android:allowBackup="false"` + `fullBackupContent` + `dataExtractionRules` in the manifest; Kotlin `excludeFromBackup` reports the real manifest state |
| Export (backup, data JSON, recovery key, Sync kit, class CSVs) | stage in Documents → `save()` exports that file | stage → `save()` returns a `content://` URI of an empty document → Rust copies the staged file into it through the fs plugin (`FsExt::open(FilePath::Url)`, mode `wt`) → discard the staged copy |
| Reading a picked file (restore, statement import) | `open()` gives a `file://` path Rust reads | `open()` gives `content://`; Rust copies it into an app-private inbox folder and hands back that path, which then follows the iOS received-copy flow (read, discard, startup sweep) |
| Files opened in Moony (share sheet, "Open with") | `RunEvent::Opened` with `file://` Inbox copies | tao turns VIEW / SEND / SEND_MULTIPLE intents into `RunEvent::Opened` with `content://` URLs; Rust copies each into the same inbox folder before queueing; manifest intent filters for the statement and backup types |
| Scheduled notifications | local digits + literal `Z` | the plugin reads the date as UTC: encode the local wall-clock time as its UTC instant on Android |
| System back | n/a | closes the top sheet or pops a detail (sheet history), never leaves the app from a sheet |
| Safe areas | `viewport-fit=cover` + `env(safe-area-inset-*)` | edge-to-edge (target SDK 35+): check that the WebView reports the insets; keyboard pushes sheet footers |
| Portrait only | `UISupportedInterfaceOrientations` | `android:screenOrientation="portrait"` on the activity |
| Icons | `ios-source-1024.png` through `tauri icon` | adaptive launcher icon from the same source |
| Copy naming iCloud | `backup.osBackupNote`, `firstRun.backupLead` | Android wording in all ten locales, or the sentence gated on the exclusion |

## Tasks

1. Project: `tauri android init`, commit `gen/android`; manifest (backup exclusion, portrait,
   intent filters, `adjustResize`), icons; first debug build for arm64 and a run on the emulator.
2. Kotlin `moony-keychain`: compile fixes, `excludeFromBackup` real state, biometric enrolment and
   unlock on the emulator (fingerprint via `adb emu finger touch`), privacy screen, status bar.
3. Picked files and exports through `content://` (Rust bridge + one frontend helper used by the
   four export call sites and the two pickers); `exportsOnThisDevice` true on Android.
4. Received files from intents.
5. Notifications on Android (UTC encoding, POST_NOTIFICATIONS permission, tap → path).
6. Shell: system back, insets, keyboard, 360 dp width with German strings, dark mode.
7. Copy for Android in ten locales; CI Android build job; signing config documented (keystore
   stays with the owner, never in the repo).
8. Gates, opus whole-branch review, local merge to main.

Owner-side, out of this plan: Play Console account, upload key, store listing, a real Android
device check (biometrics on hardware, notifications after reboot, share from a bank app).

## State at the end of the first session (2026-11-17)

Built and checked on the emulator (arm64, Android 17 image, host GPU), with the demo profile
copied into the app's data folder:

- Start, lock page, password unlock, all four tabs, Settings; Czech, English and German; light and
  dark (window background and status bar icons follow the theme); 360 dp width with German strings.
- System back: closes a sheet, a dialog, and now also an open menu, popover or select list
  (`withPhoneHistory` on their roots); pops a detail; at the root it leaves the app.
- HTTPS (prices, history, logos) after the rustls-platform-verifier JNI init.
- Backup through the save picker (a valid archive in Downloads, the staged copy removed, the backup
  recorded); restore picker reads a `content://` backup up to the confirmation; a CSV shared from
  the Files app opens the import sheet "Ze sdílení" with its columns recognised, and its copy is
  removed when the sheet closes.
- Notifications: permission prompt, the plan scheduled at 09:00 local (alarm checked with
  `dumpsys alarm`), a test notification shown with the Moony status icon, the plan stored by the
  plugin (vendored patch).
- The Android wording where the phone copy names iCloud or Files.

Not verified (needs the owner or a real device):

- Biometric enrolment and unlock with a fingerprint (the emulator's fingerprint setup starts with
  a consent screen that is the owner's to accept), invalidation after a new fingerprint.
- A real device: performance of a release build, edge-to-edge on devices with cutouts, OEM skins.

(Notification taps, the reboot restore and the camera were checked later the same day; see
"Second pass" below.)

Known quirks and follow-ups:

- `adb install -r` followed at once by `am start` can end the new process (a stale activity record
  of the killed one); starting from the launcher, or a few seconds later, works. Not seen otherwise.
- The emulator's software renderer (`-gpu auto` falling back) leaves stale tiles in the WebView;
  run it with `-gpu host`.
- A backup picked for restore and then cancelled stays in the inbox folder until the next start's
  sweep (iOS behaves the same).
- App shortcuts (spec §6: Zaznamenat zůstatek · Importovat výpis · Přidat transakci) exist on
  neither platform; the navigation-resilience spec lists them as a non-goal for the native phase.
  An Android-only `shortcuts.xml` would need deep-link handling the shell does not have yet.
- Release: signing config, `npm run android:bundle` and a manual `Android bundle` workflow are in
  place (second pass); the upload key, the secrets and the Play Console listing are the owner's.
- Android 18 will drop the implicit URI grant wry's camera intent relies on (logged as an error
  with a stack trace by the platform today, no effect yet). Fix upstream in wry's generated
  `RustWebChromeClient` (an explicit `FLAG_GRANT_WRITE_URI_PERMISSION` + `ClipData`).
- tauri-build's `{:?}` path quoting is worth an upstream issue (the `settings.gradle` workaround).

## Whole-branch review (opus, 2026-11-17) and what changed

All ten findings were addressed: copies from `content://` land in the inbox only when complete
(`moony-incoming` + rename), a failed export deletes the half-written document, the Android inbox
sits under the app's private cache (not `TMPDIR`), only `content://` URLs are queued from intents
and a shared file is capped at 25 MB, tao's log is off on Android (it logged intent URIs), the TLS
verifier holds the application context, the rustls Maven repository is pinned to a commit and the
`org.rustls` group, the FileProvider exposes only the camera captures' folder, file names are capped
by UTF-8 bytes and claimed atomically, and menus hold a history entry on Android only, with menu
items that navigate waiting until it is gone (`navigateFromMenu`). Checked on the emulator after
the fixes: Cashflow › Budgets verwalten and one back press returns to Cashflow.

Outside this branch: `cargo audit` flags RUSTSEC-2026-0459 (`libsqlite3-sys` 0.38.2, published
2026-10-30, fix in >= 0.39), already on main; it needs the rusqlite bump that brings 0.39.

## Second pass (2026-11-17, afternoon): the checks left from the first session

Run on the emulator with the demo profile, after main had moved on (sync phase 5 merged in).

- **Notification taps.** With the app in the background a tap opens its screen. A tap that
  started the app from cold was dropped: the plugin reported it from `load`, before the page had
  registered its listener, and `trigger` drops events nobody listens to. The vendored plugin now
  holds the tap until the first `actionPerformed` listener registers; after the unlock the screen
  opens (`/agenda`, `/cashflow` checked). Tapping the system's group summary (two notifications
  or more) opens the app without a target, as Android intends.
- **Reboot.** The plugin's boot receiver crashed at every boot: it read the store with a plain
  Jackson mapper that threw on `extra` (Moony's `path`), so no notification came back after a
  restart. Patched (MOONY-PATCH.md): the receiver ignores unknown properties, the store skips
  what it cannot read and keeps the stored JSON as `sourceJson`. Checked: after `adb reboot` all
  three alarms were back (a missed one 15 s after boot, as upstream intends), the notifications
  fired, and a tap on a restored one opened `/cashflow`, warm and from cold.
- **"0 pending".** `sendNotification` returns before the plugin has the notification; on Android
  `show` reached Kotlin up to ~10 s after the unlock. Scheduling now awaits
  `plugin:notification|notify` through `notificationsApi.schedule` (`tauri-api.ts`), counts
  refusals, and the count is the system's own (checked: cancel → show → getPending, no warning).
- **Camera.** "Vyfotit" opened the photo picker: wry starts `ACTION_IMAGE_CAPTURE` only when
  `resolveActivity` finds a camera app, which from Android 11 needs a `<queries>` entry. Added.
  Checked: the CAMERA prompt (the permission is declared by the barcode scanner plugin), the
  camera app, the photo in "Dokument hinzufügen", the encrypted document and its viewer, and the
  capture swept at the next start ("Camera capture sweep removed 1 file(s)").
- **Release build.** Signing from `keystore.properties` or `MOONY_ANDROID_*`, `npm run
  android:bundle`, `.github/workflows/android-bundle.yml` (manual, artifact only), playbook
  section "Android: upload key and Play bundle". The first release build ever (arm64, R8 full
  mode, signed with the emulator's debug key so it installed over the demo profile) unlocked,
  drew every tab and refreshed prices over HTTPS (rustls-platform-verifier survived R8). R8 had
  removed the constructors of ML Kit's component registrars (the barcode scanner's QR reader,
  `NoSuchMethodException … BarcodeRegistrar.<init>` at start); `proguard-rules.pro` now keeps
  every `ComponentRegistrar`, and the warnings are gone. The pairing scanner itself was not
  opened in a release build. A signed arm64 AAB built and verified (`jarsigner`) at the path the
  workflow uploads.

