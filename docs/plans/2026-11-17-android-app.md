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
- Notifications after a reboot (restore receiver), a tap on a notification opening its screen.
- Camera and photo picking for documents and property photos in the Android WebView, and whether
  the WebView leaves plaintext copies of picked files (the iOS `WKFileUploadPanel` sweep has no
  Android counterpart yet).
- A real device: performance of a release build, edge-to-edge on devices with cutouts, OEM skins.

Known quirks and follow-ups:

- `adb install -r` followed at once by `am start` can end the new process (a stale activity record
  of the killed one); starting from the launcher, or a few seconds later, works. Not seen otherwise.
- The emulator's software renderer (`-gpu auto` falling back) leaves stale tiles in the WebView;
  run it with `-gpu host`.
- `[notifications] N planned, 0 pending` can be logged on Android: `sendNotification` is not
  awaited, so the check may run before the plugin stored the plan. Harmless; the check could wait.
- A backup picked for restore and then cancelled stays in the inbox folder until the next start's
  sweep (iOS behaves the same).
- App shortcuts (iOS `UIApplicationShortcutItems`) have no Android `shortcuts.xml` yet.
- Release: signing (upload key with the owner), `versionCode` from `tauri.properties`, Play Console
  listing, an AAB build (`--aab`), and the release workflow (desktop only today).
- tauri-build's `{:?}` path quoting is worth an upstream issue (the `settings.gradle` workaround).
