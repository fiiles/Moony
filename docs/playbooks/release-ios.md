# Playbook: Build the iPhone app for TestFlight and the App Store

How a build of the phone app (`com.moonyfinance.mobile`) is made for App Store Connect, what the
build number is, and what in the iOS project exists because of the store. The desktop release is
`release.md`; the Android bundle is in its "Android" section.

The store build is made by hand on a Mac with `scripts/ios-store-build.sh`. Nothing uploads by
itself, and CI builds no iOS package (it only type-checks the crate for the simulator).

A store build is a release build, so it carries what `services/sync/feature.rs` says: with
`RELEASED = false` it has no device sync, and nothing of sync shows in it.

## One-time setup (the owner)

1. Membership in the Apple Developer Program, and the App ID `com.moonyfinance.mobile` registered
   under *Certificates, Identifiers & Profiles*. The app uses no capability that needs switching on
   there (the entitlements file is empty).
2. An app record in App Store Connect for that bundle ID (name, primary language, SKU).
3. The team ID where the Tauri CLI finds it: `APPLE_DEVELOPMENT_TEAM` in the environment or in the
   git-ignored `.env` at the repository root (`npm run tauri` loads it).
4. A way to sign for distribution. Either of:
   - **An App Store Connect API key** (Users and Access → Integrations): the key ID in
     `APPLE_API_KEY`, the issuer ID in `APPLE_API_ISSUER`, the path of the downloaded
     `AuthKey_<key id>.p8` in `APPLE_API_KEY_PATH`. With all three set the Tauri CLI archives
     without signing and lets `xcodebuild` sign the export with signing managed by Apple
     (`-allowProvisioningUpdates` with the key). The same key uploads the package.
   - **Xcode signed in** with the Apple ID (Xcode → Settings → Accounts). Automatic signing then
     creates the distribution certificate and profile at export. The upload still needs an API key
     or the Transporter app.

   Keep the `.p8` file and the three values out of the repository.

## The build number

iOS has two version fields. `CFBundleShortVersionString` is the app version (`1.4.0`, from
`tauri.conf.json`). `CFBundleVersion` is the build number: App Store Connect takes each one once
per version, so every upload needs a new, higher one. Apple reads at most three dot-separated
integers from it, so `--build-number` of the Tauri CLI (which appends a fourth to the version) does
not make a distinct build.

- Every local build has the build number `1` (`bundle.iOS.bundleVersion` in
  `src-tauri/tauri.ios.conf.json`; `project.yml` and the generated `Info.plist` say the same). A
  version bump does not touch it.
- `scripts/ios-store-build.sh` overrides it for the one build it makes with **the UTC minute of the
  build, `YYMMDDHHMM`** (2611201542 for 20 November 2026, 15:42 UTC). It rises by itself, needs
  nothing remembered or committed, and is the same scheme from any checkout. TestFlight shows the
  build as `1.4.0 (2611201542)`.
- `--build-number N` (or `MOONY_IOS_BUILD_NUMBER`) sets a number by hand, for example to repeat a
  build. Within one version it must be higher than every number already uploaded.

## Build

```bash
npm ci
npm run ios:bundle                 # signed .ipa for App Store Connect
npm run ios:bundle -- --dry-run    # unsigned .ipa: checks the package without any account
```

The script runs `tauri ios build --export-method app-store-connect` with the build number, then
unpacks the `.ipa` (`src-tauri/gen/apple/build/arm64/Moony.ipa`) and fails unless the package has
the right bundle ID, version and build number and contains `PrivacyInfo.xcprivacy`.
It ends by putting `Info.plist` and `project.pbxproj` back as they were: the Tauri CLI writes the
version, the build number and the signing team into both at every build.

The export method comes from the command line. The tracked `ExportOptions.plist` stays `debugging`,
which is what an install on your own phone uses
(`npm run tauri -- ios build --debug --export-method debugging`); the CLI merges the method it is
given over that file in a temporary copy.

Before the first upload of a version, install the release build on a phone and walk through it: a
debug build has sync on, a release build does not, so only the release build shows what a tester
gets.

## Upload

Not done by the script. With the API key (`AuthKey_<key id>.p8` in `~/.appstoreconnect/private_keys`
or `~/.private_keys`):

```bash
xcrun altool --validate-app src-tauri/gen/apple/build/arm64/Moony.ipa --api-key "$APPLE_API_KEY" --api-issuer "$APPLE_API_ISSUER"
xcrun altool --upload-package src-tauri/gen/apple/build/arm64/Moony.ipa --api-key "$APPLE_API_KEY" --api-issuer "$APPLE_API_ISSUER"
```

or drop the `.ipa` into the Transporter app. After the upload, read the e-mail App Store Connect
sends about the build: that is where it names a missing privacy-manifest entry or another problem
with the package.

## Export compliance

The app contains encryption beyond what iOS provides (SQLCipher on CommonCrypto, and in Rust
AES-GCM, XChaCha20-Poly1305, scrypt, Ed25519/X25519 and TLS through rustls). App Store Connect asks
about it for every uploaded build until `ITSAppUsesNonExemptEncryption` is in the Info.plist. The
key is **not set**: which answer is right is the owner's legal decision. Once decided, add one of
these to `src-tauri/Info.ios.plist` (the CLI merges that file into the generated `Info.plist`) and
to `src-tauri/gen/apple/moony_iOS/Info.plist`:

```xml
<!-- only exempt encryption: no questionnaire, no documentation -->
<key>ITSAppUsesNonExemptEncryption</key>
<false/>
```

```xml
<!-- non-exempt encryption: App Store Connect asks for the export documentation it needs -->
<key>ITSAppUsesNonExemptEncryption</key>
<true/>
```

## What the project carries for the store

### Privacy manifest

`src-tauri/gen/apple/moony_iOS/PrivacyInfo.xcprivacy`, bundled as a resource (xcodegen picks it up
from the `moony_iOS` source folder; `src/mobile/store-readiness.test.ts` checks that the generated
project bundles it). The app is one executable with everything linked in statically, so one
manifest covers the Rust core, SQLCipher and the Swift plugins. It declares no tracking, no
collected data, and two categories of "required reason" API:

| Category | Reasons | What uses it |
|---|---|---|
| File timestamp | `C617.1`, `3B52.1` | `stat`, `fstat`, `lstat`, `fstatat` from the Rust standard library, SQLCipher and the async runtime on the app's own files; `NSFileModificationDate` of a file the person picked, read by the dialog plugin |
| Disk space | `E174.1` | `statfs`, `fstatfs` inside SQLCipher's SQLite, which reads the file system's flags when it opens a database file |

No approved reason describes what SQLite does with `statfs` (it does not look at free space);
`E174.1` is the reason SQLCipher's own privacy manifest declares for the same code. Nothing read
this way is shown, stored or sent.

Re-check after a dependency change that could add such an API, on a built app (any configuration):

```bash
app=src-tauri/gen/apple/build/arm64-sim/Moony.app/Moony   # or the binary inside an unpacked .ipa
nm -u "$app" | grep -E '_(stat|fstat|lstat|fstatat|getattrlist|getattrlistbulk|fgetattrlist|getattrlistat|statfs|fstatfs|statvfs|fstatvfs|mach_absolute_time)$'
nm -u "$app" | grep -E 'NSFile(Modification|Creation)Date|NSFileSystem(Free)?Size|NSURL(ContentModification|Creation)DateKey|NSURLVolume'
strings -a "$app" | grep -E '^(NSUserDefaults|standardUserDefaults|systemUptime|activeInputModes)$'
```

The categories, their APIs and the reason codes are in Apple's documentation of
`NSPrivacyAccessedAPIType`. A debug build also shows the strings of the last line: they come from
the unused selector tables of the `objc2` bindings and are gone from a release build.

### Scene life cycle

The Info.plist has no `UIApplicationSceneManifest`. Built with Xcode 27.1 (SDK 27.1), the app
launches and draws without it on the iOS 27.0 and 27.1 simulators. `tao` switches to its scene
delegate when the manifest is present, which changes how the app is launched and how files and
shortcuts reach it; adopt it as its own change, with the share-in import, the app shortcuts and the
privacy screen tested on a device. Repeat the launch check after a major Xcode update.

### Permission texts

`NSCameraUsageDescription` describes photographing documents and the property. With device sync the
camera also scans pairing codes and the Recovery Kit's QR code; the text names that only once
`RELEASED` is `true`, and `src/mobile/permission-strings.test.ts` fails on the flip until the ten
`InfoPlist.strings`, `Info.ios.plist` and the generated `Info.plist` carry the scanning clause again
(the texts are in the tag `v1.4.0`). `NSLocalNetworkUsageDescription` stays: iOS shows it only when
the app first reaches a local address, which only sync does.

## Key files

| File | Purpose |
|---|---|
| `scripts/ios-store-build.sh` | The store build and its package checks (`npm run ios:bundle`) |
| `src-tauri/tauri.ios.conf.json` | Bundle ID and the local build number |
| `src-tauri/Info.ios.plist` | Permission texts and document types, merged into the generated Info.plist |
| `src-tauri/gen/apple/project.yml` | The xcodegen project: orientations, resources |
| `src-tauri/gen/apple/moony_iOS/PrivacyInfo.xcprivacy` | Privacy manifest |
| `src-tauri/gen/apple/ExportOptions.plist` | Export options of a device install (`debugging`) |
| `src-tauri/gen/apple/Localizations/` | The permission texts in the ten app languages |
