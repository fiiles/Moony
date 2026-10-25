# Playbook: Cut a Release

How a Moony release is built, published and verified, how the macOS signing and notarization
works, and what is still missing before the Windows installer can be signed. This is the execution checklist; the design
of the in-app updater itself lives in `src-tauri/tauri.conf.json` (`plugins.updater`) and
`src/hooks/use-updater.tsx`.

Releases are built by GitHub Actions from a tag and published by hand from a draft. They are
signed and notarized for macOS (Developer ID), and not signed for Windows yet (see [Code signing
and notarization](#code-signing-and-notarization)); the **updater signature**
(minisign key pair) is a different thing, is part of every release build, and is covered in
[Updater signing key](#updater-signing-key).

## One-time setup

1. **Updater key pair** — see [Updater signing key](#updater-signing-key). The public key is
   already in `src-tauri/tauri.conf.json`; the matching private key and its password must be
   stored as repository secrets.
2. **Repository secrets** (Settings → Secrets and variables → Actions):

   | Secret | Used by | Required |
   |---|---|---|
   | `TAURI_SIGNING_PRIVATE_KEY` | `release.yml` — signs update archives | yes |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `release.yml` | yes |

   `GITHUB_TOKEN` is provided by Actions.
3. **Updater endpoint** — `plugins.updater.endpoints` in `src-tauri/tauri.conf.json` is
   `https://github.com/fiiles/Moony/releases/latest/download/latest.json`. Installed apps carry
   this URL, so keep the repository where it is; if it ever has to move, change the endpoint
   (and the repository URLs in `README.md`, `.github/ISSUE_TEMPLATE/config.yml` and the release
   body in `.github/release-body.md`) in the same commit and expect users of older builds to update by hand.
4. **Never change the app identifier again** (`identifier` in `tauri.conf.json`,
   `com.moonyfinance.desktop`). The user's data folder is derived from it; changing it makes every
   existing installation look for its data in a new, empty place. It changed once, in 0.11.0,
   without an automatic move (ADR 0015); a second change would make every user rename the data
   folder by hand again.

## Pre-flight (every release)

On a clean `main`:

```bash
# frontend and backend gates (same as CI)
npm run lint && npm run typecheck && npm run format:check && npm test
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test

# generated types must match the Rust types
cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts
cd ..

# dependency advisories (CI blocks on these too; exceptions are in .cargo/audit.toml and
# .cargo/audit-yanked.txt)
scripts/cargo-audit.sh
npm audit --omit=dev --audit-level=high

# regenerate the licence inventory and commit it if it changed
npm run licenses
```

Then, by hand:

- [ ] `CHANGELOG.md`: move the *Unreleased* entries under the new version heading with today's
      date and leave a fresh, empty *Unreleased* section above it. The release page shows that
      section under "What's new" (and the update dialog links there), so write it for users; the
      release run fails when the version has no section.
- [ ] `PRIVACY.md` and the README privacy section still match the code (new network request,
      new stored data?).
- [ ] `npm run tauri build` works locally and the installer starts, unlocks the demo profile
      (`CONTRIBUTING.md` → "Demo data") and shows the new version in the About dialog.
      Artifacts land in `src-tauri/target/release/bundle/`.
- [ ] Migrations added since the last release were tried against a copy of a database written
      by that release (`docs/playbooks/add-db-migration.md`).

## Version bump

The version must match in three files, plus the lockfiles that echo it:

| File | Field |
|---|---|
| `package.json` | `"version"` |
| `src-tauri/Cargo.toml` | `[package] version` |
| `src-tauri/tauri.conf.json` | `"version"` |

Then update the lockfiles without building anything:

```bash
npm install --package-lock-only
cd src-tauri && cargo metadata --format-version 1 >/dev/null && cd ..
```

The About dialog reads the version from Tauri at runtime (`getVersion()`), so there is no
fourth place to edit. Use [Semantic Versioning](https://semver.org/): MAJOR for breaking
changes (for example an incompatible database or MCP change), MINOR for features, PATCH for
fixes.

Also in the bump commit (`check-version` does not read these, so nothing fails if you forget):

- **The iOS project version fields**: `CFBundleShortVersionString` and `CFBundleVersion` in
  `src-tauri/gen/apple/project.yml` and in `src-tauri/gen/apple/moony_iOS/Info.plist`.
- **The status text** in the README ("Status and compatibility") and in `ROADMAP.md`, when the
  release changes what they say.
- **`npm run licenses`**, committed when `THIRD_PARTY_LICENSES.md` changed.
- **The compare links at the bottom of `CHANGELOG.md`**: point `[Unreleased]` at
  `compare/vX.Y.Z...HEAD` and add `[X.Y.Z]` with the link from the previous version's tag.

```bash
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json \
  src-tauri/gen/apple/project.yml src-tauri/gen/apple/moony_iOS/Info.plist \
  README.md ROADMAP.md THIRD_PARTY_LICENSES.md CHANGELOG.md
git commit -m "chore(release): v1.0.0"
```

> **The updater only offers a version that is higher than the installed one.** Builds from the
> old private repository keep checking that repository, not this one; their users install the
> current version by hand.

## Tag and build

The release workflow starts on any tag that begins with `v`. It cannot be started by hand:

Push `main` alone first and wait until CI (Frontend, Rust Check, Rust iOS Check) is green; only
then tag and push the tag. Never use `git push --all` or `git push --mirror`: a local archive
branch has unrelated history and must not reach the public repository.

```bash
git push origin main
# wait for CI on that commit to be green, then:
git tag v1.0.0
git push origin v1.0.0
```

The tag name decides how the release is published:

- `vX.Y.Z` is a full release. Once published it becomes the "latest" release that the updater
  endpoint (`…/releases/latest/download/latest.json`) serves.
- A tag with a hyphen (`v1.0.0-rc.1`, `v1.0.0-test.1`) is created as a **pre-release**
  (the `create-release` job in `release.yml` sets it for any tag containing `-`). GitHub's "latest" skips
  pre-releases, so the updater never offers one.

`.github/workflows/release.yml` runs three stages:

1. **`check-version`** reads the version from `package.json`, `src-tauri/tauri.conf.json` and
   the first `version = "…"` line of `src-tauri/Cargo.toml` and fails unless all three equal the
   tag without its leading `v` and without a pre-release suffix (`v1.0.0-rc.1` requires
   `1.0.0` in the manifests). Nothing is built and no release is created on a mismatch. To
   recover, delete the tag (`git push origin --delete v1.0.0 && git tag -d v1.0.0`), fix the
   version, and tag again.
2. **`create-release`** creates the **draft** release `Moony v<version>` once, with the body
   from `.github/release-body.md` (`@VERSION@` replaced, `@CHANGES@` replaced by the version's
   `CHANGELOG.md` section; no section fails the run), and hands its id to the build. A re-run
   reuses the draft that already carries the tag. (Before 0.12.0 every build leg created its own
   draft, and v0.11.0 ended up split across two releases.)
3. **`build`** runs five legs in parallel (15–30 minutes):

   | Runner | Target | Artifacts |
   |---|---|---|
   | `macos-latest` | `aarch64-apple-darwin` | `Moony_<version>_aarch64.dmg`, updater archive |
   | `macos-15` | `x86_64-apple-darwin` | `Moony_<version>_x64.dmg`, updater archive |
   | `ubuntu-24.04` | Linux x86_64 | `_amd64.AppImage`, `_amd64.deb`, `.x86_64.rpm` |
   | `ubuntu-24.04-arm` | Linux aarch64 (native ARM runner) | `_aarch64.AppImage`, `_arm64.deb`, `.aarch64.rpm` |
   | `windows-latest` | Windows x86_64 | `Moony_<version>_x64-setup.exe` (NSIS installer) |

   Each leg installs Node 22 and stable Rust (Linux: the WebKitGTK build dependencies; Windows:
   a static OpenSSL from vcpkg), runs `npm ci`, and calls `tauri-apps/tauri-action`, which
   builds the bundles listed under `bundle.targets` in `tauri.conf.json`, signs each update
   archive with the updater key, uploads everything to that draft (`releaseId`) and generates `latest.json` (version, notes, per-platform URL and
   signature). The notes are one plain line linking the release page: the update dialog of
   0.12.2 and older prints them verbatim, newer builds show no notes and link to the release
   page under "What's new". There is no `.msi`.

### A test build without a public release

To try the pipeline, or the installers, without publishing anything, tag the commit with a
pre-release suffix:

```bash
git tag v1.0.0-rc.1      # manifests must already say 1.0.0, and CHANGELOG.md needs a "## [1.0.0]" section
git push origin v1.0.0-rc.1
```

Any suffixed tag (`-rc.1`, `-test.1`) is checked like the full one: it still needs the matching
`## [X.Y.Z]` section in `CHANGELOG.md` and the bumped manifests, so bump the version and write the
changelog first.

The run creates a **draft pre-release** with the full set of artifacts. When you are done,
delete the draft under *Releases*, then the tag
(`git push origin --delete v1.0.0-rc.1 && git tag -d v1.0.0-rc.1`).

## Review and publish the draft

1. Open *Releases* and edit the draft.
2. Check the attached files: one installer per platform, the update archives with their
   `.sig` files, and `latest.json`.
3. Check the body. It opens with the version's "What's new" section from `CHANGELOG.md`, then
   `.github/release-body.md` continues with a download table with the
   exact file names, and a note that the Mac app opens normally, first-launch steps for the unsigned
   Windows installer (browser warning and SmartScreen) and Linux; keep those sections and add a short
   highlights paragraph on top if the release deserves one. A tag with a hyphen must show as
   a pre-release.
4. Install the artifacts on clean machines — at least one macOS, one Windows and one Linux
   — and confirm the first-run wizard, unlock and the About version. The Mac app must open without any
   warning; expect the SmartScreen warning described in the README for as long as the Windows
   installer is unsigned.
5. Before clicking Publish, check the draft has 21 assets, the nine file names of the download
   table in `.github/release-body.md`, and a `latest.json` with 14 platform keys (each Linux
   architecture adds `linux-<arch>`, `-appimage`, `-deb` and `-rpm`):

   ```bash
   gh release download vX.Y.Z -R fiiles/Moony -p latest.json -O - | jq '.platforms | keys'
   ```
6. Click **Publish release**. Only a published, non-pre-release release is served by
   `…/releases/latest/download/latest.json`.
7. Once an older version exists, install it on a machine, open Moony, let it check for updates
   and run the update end to end (download, signature verification, restart).

## Code signing and notarization

macOS is set up: release builds are signed with a Developer ID Application certificate and
notarized by Apple, so Gatekeeper opens the app without any warning. Windows is **not set up
yet**: without Authenticode signing, SmartScreen shows "Windows protected your PC" and users
get past it with the steps in the README ("Install"). Signing the Windows installer needs a
certificate that the project does not have yet; the routes are listed below.

### macOS: Developer ID signing and notarization (set up)

The release workflow (`.github/workflows/release.yml`) does this on both macOS legs (Apple
Silicon and Intel):

1. **Switch.** Signing is on only when the repository secret `APPLE_CERTIFICATE` exists
   (`APPLE_SIGNING`). Without it the Macs build unsigned. Tauri starts signing as soon as
   `APPLE_CERTIFICATE` is set, even to an empty string, so the step *Apple signing and
   notarization* exports the variables into the job environment only when the secret exists.
2. **Repository secrets** the step reads:

   | Secret | Value |
   |---|---|
   | `APPLE_CERTIFICATE` | the base64 of the Developer ID Application `.p12` file (`base64 -i DeveloperID.p12 \| pbcopy`) |
   | `APPLE_CERTIFICATE_PASSWORD` | the password chosen when exporting the `.p12` |
   | `APPLE_SIGNING_IDENTITY` | the certificate name, `Developer ID Application: Filip Král (TEAMID)` |

   Notarization credentials, one of two sets (the Apple ID set wins when `APPLE_ID` is set):

   | Secret | Value |
   |---|---|
   | `APPLE_ID` | the Apple ID used for notarization |
   | `APPLE_PASSWORD` | an **app-specific password** for that Apple ID (appleid.apple.com → Sign-In and Security) |
   | `APPLE_TEAM_ID` | the 10-character team ID |

   or an App Store Connect API key:

   | Secret | Value |
   |---|---|
   | `APPLE_API_ISSUER` | the issuer ID |
   | `APPLE_API_KEY` | the key ID |
   | `APPLE_API_KEY_P8` | the content of the `.p8` key file; the step writes it to a temporary file and exports `APPLE_API_KEY_PATH` |

   If signing is on but neither `APPLE_ID` nor `APPLE_API_KEY` is set, the step fails with an
   error instead of building a signed but un-notarized app.
3. **Build.** The Tauri bundler signs with the hardened runtime, submits the app for
   notarization and staples the ticket. Check whether the app needs an entitlements file
   (`bundle.macOS.entitlements`) if a future change adds capabilities; the Tauri defaults
   normally do not.
4. **Verification.** After the build, the step *Verify the macOS signature and notarization*
   runs `codesign --verify --deep --strict`, `spctl --assess --type execute` and
   `xcrun stapler validate` on `Moony.app`. If any of them fails, the leg fails and the release
   check goes red; the assets are already in the draft, which stays unpublished. The temporary
   `.p8` key file is removed at the end.
5. **Verify a download by hand:**

   ```bash
   codesign --verify --deep --strict --verbose=2 /Applications/Moony.app
   spctl --assess --type execute --verbose=4 /Applications/Moony.app
   xcrun stapler validate /Applications/Moony.app
   ```

To renew an expired certificate, create a new Developer ID Application certificate, export it as
a `.p12` and replace `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and, if the name changed,
`APPLE_SIGNING_IDENTITY`.

### Windows: Authenticode signing

Pick one route; all of them end with a signed NSIS installer.

| Route | What the owner needs | Notes |
|---|---|---|
| **OV / EV code-signing certificate** from a certificate authority | The certificate and its private key, usually on a hardware token or in the CA's cloud HSM (new certificates can no longer be exported as a `.pfx`). If a `.pfx` is available: secrets `WINDOWS_CERTIFICATE` (base64) and `WINDOWS_CERTIFICATE_PASSWORD`, `bundle.windows.certificateThumbprint`, `digestAlgorithm: "sha256"` and `timestampUrl` in `tauri.conf.json`, and a workflow step that imports the certificate (`Import-PfxCertificate`). | EV certificates get SmartScreen reputation immediately; OV certificates earn it with downloads over time. |
| **Azure Trusted Signing** | An Azure subscription, a Trusted Signing account, a validated identity and a certificate profile; a service principal with the signing role; secrets `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` plus the endpoint, account and profile names; `bundle.windows.signCommand` calling `trusted-signing-cli`. | Cheapest managed route; identity validation is only available to some countries and organization types, so check eligibility first. |
| **SignPath Foundation** (free for open-source projects) | An approved application for the public repository, an API token (`SIGNPATH_API_TOKEN`) and the organization, project and signing-policy slugs; a workflow step that submits the unsigned installer (`signpath/github-action-submit-signing-request`). | Requires the public repository and builds that run on GitHub Actions. |

**Order matters for the updater.** The updater signature (`.sig`) is computed over the exact
installer file; changing the installer afterwards invalidates it. Prefer
`bundle.windows.signCommand` (runs inside the Tauri build, before the updater signature is made).
With a post-build service such as SignPath, re-sign the signed installer with
`npm run tauri signer sign` and re-upload the `.sig` and `latest.json`.

### Linux

No operating-system signing exists for `.AppImage`, `.deb` and `.rpm`. Optional hardening:
publish a `SHA256SUMS` file with the release, or sign the packages with a GPG key and publish
the public key in the README.

### Updater signing key

The Tauri updater refuses to install an update whose signature does not verify against the
public key built into the app.

```bash
npm run tauri signer generate -- -w ~/.tauri/moony-updater.key
```

- The **public key** (printed, and saved as `~/.tauri/moony-updater.key.pub`) goes into
  `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. A key is already configured there.
- The **private key** file content goes into the secret `TAURI_SIGNING_PRIVATE_KEY` and its
  password into `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit it. Keep the key file and
  its password together in a password manager: without the password the key cannot sign
  anything, and GitHub never shows a stored secret again.
- If the key pair is ever **regenerated**, builds in the field still carry the old public key
  and cannot verify new updates — those users must install the new version by hand. A new
  repository or a new secret store does not need a new key: reuse the same pair.

### When Windows signing is in place

- Add the Windows variables or signing step to `release.yml`; do not touch the build matrix.
- Rename the Windows section of the release body (`.github/release-body.md`, "First launch on
  Windows") or remove it, and update the unsigned-installer paragraphs in `README.md`
  ("Install"), `SECURITY.md` (scope), `ROADMAP.md` and this playbook.
- Re-test the whole first-run path from a downloaded installer on a clean Windows machine, with
  the download mark set (a download from the browser, not a local build).

## Troubleshooting

**`check-version` fails** — the tag and the three manifest versions disagree; the log lists
each value. Delete the tag, fix the version (see [Version bump](#version-bump)) and tag again.

**Build fails with "TAURI_SIGNING_PRIVATE_KEY not set"** — the secret is missing or empty in
the repository; paste the complete key file content.

**A matrix leg fails on its own** — re-run the failed job; the other legs keep their artifacts.

**The app does not offer an update** — check that the release is published (not a draft and
not a pre-release), that `latest.json` is attached, that `plugins.updater.endpoints` points at
the right repository, that the new version is higher than the installed one, and that the
update check is not switched off in Settings.

**"Signature mismatch" while updating** — the installed app carries a different public key than
the private key that signed the release, or an installer was modified after it was signed
(see "Order matters" above).

**macOS says the app is "damaged" or from an unidentified developer** — this should not happen
with 1.0.0 and later. Check that the release was built with the Apple secrets (the leg log shows
the *Apple signing and notarization* and *Verify the macOS signature and notarization* steps; if
they were skipped, `APPLE_CERTIFICATE` was missing) and run the manual checks above on the
downloaded app. The message is expected only for builds before 1.0 and for copies built from
source. For those, `xattr -cr /Applications/Moony.app` removes the download quarantine flag, or
use *Open Anyway* in System Settings → Privacy & Security.

## Key files

| File | Purpose |
|---|---|
| `.github/workflows/release.yml` | Version check, draft-release creation and build matrix |
| `.github/release-body.md` | Release page body (`@VERSION@` and `@CHANGES@` placeholders) |
| `.github/workflows/ci.yml` | Gates run on every push and pull request |
| `.github/dependabot.yml` | Dependency update pull requests |
| `src-tauri/tauri.conf.json` | Version, identifier, bundle targets, updater public key and endpoint |
| `CHANGELOG.md` | Source of the release notes |
| `THIRD_PARTY_LICENSES.md` | Regenerated by `npm run licenses` |
| `~/.tauri/moony-updater.key` (outside the repo) | Updater private key |
