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
