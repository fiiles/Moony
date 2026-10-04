# Playbook: Cut a Release

How a Moony release is built, published and verified, and what is still missing before the
installers can be signed by Apple and Microsoft. This is the execution checklist; the design
of the in-app updater itself lives in `src-tauri/tauri.conf.json` (`plugins.updater`) and
`src/hooks/use-updater.tsx`.

Releases are built by GitHub Actions from a tag and published by hand from a draft. They are
unsigned by Apple and Microsoft today (see [Code signing and
notarization](#code-signing-and-notarization-not-set-up-yet)); the **updater signature**
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
   body in `release.yml`) in the same commit and expect users of older builds to update by hand.
4. **Never change the app identifier** (`identifier` in `tauri.conf.json`,
   `com.filipkral.moony`). The user's data folder is derived from it; changing it makes every
   existing installation look for its data in a new, empty place.

## Pre-flight (every release)

On a clean `main`:

```bash
# frontend and backend gates (same as CI)
npm run lint && npm run typecheck && npm test
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test

# generated types must match the Rust types
cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts
cd ..

# dependency advisories (CI blocks on these too; exceptions are in .cargo/audit.toml)
cargo audit --file src-tauri/Cargo.lock --deny warnings
npm audit --omit=dev --audit-level=high

# regenerate the licence inventory and commit it if it changed
npm run licenses
```

Then, by hand:

- [ ] `CHANGELOG.md`: move the *Unreleased* entries under the new version heading with today's
      date and leave a fresh, empty *Unreleased* section above it.
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

```bash
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json CHANGELOG.md
git commit -m "chore(release): v0.9.1"
```

> **The updater only offers a version that is higher than the installed one.** Builds from the
> previous repository (1.x) keep checking that repository's releases, so anyone still on one of
> them has to install 0.9.x by hand. Say so in the notes of the first release.

## Tag and build

The release workflow starts on any tag that begins with `v`. It cannot be started by hand:

```bash
git tag v0.9.1
git push origin main v0.9.1
```

The tag name decides how the release is published:

- `vX.Y.Z` is a full release. Once published it becomes the "latest" release that the updater
  endpoint (`…/releases/latest/download/latest.json`) serves.
- A tag with a hyphen (`v1.0.0-rc.1`, `v0.9.0-test.1`) is created as a **pre-release**
  (`prerelease: contains(github.ref_name, '-')` in `release.yml`). GitHub's "latest" skips
  pre-releases, so the updater never offers one.

`.github/workflows/release.yml` runs two stages:

1. **`check-version`** reads the version from `package.json`, `src-tauri/tauri.conf.json` and
   the first `version = "…"` line of `src-tauri/Cargo.toml` and fails unless all three equal the
   tag without its leading `v` and without a pre-release suffix (`v1.0.0-rc.1` requires
   `1.0.0` in the manifests). Nothing is built and no release is created on a mismatch. To
   recover, delete the tag (`git push origin --delete v0.9.1 && git tag -d v0.9.1`), fix the
   version, and tag again.
2. **`build`** runs four legs in parallel (15–30 minutes):

   | Runner | Target | Artifacts |
   |---|---|---|
   | `macos-latest` | `aarch64-apple-darwin` | `Moony_<version>_aarch64.dmg`, updater archive |
   | `macos-15` | `x86_64-apple-darwin` | `Moony_<version>_x64.dmg`, updater archive |
   | `ubuntu-24.04` | Linux x86_64 | `.AppImage`, `.deb`, `.rpm` |
   | `windows-latest` | Windows x86_64 | `Moony_<version>_x64-setup.exe` (NSIS installer) |

   Each leg installs Node 22 and stable Rust (Linux: the WebKitGTK build dependencies; Windows:
   a static OpenSSL from vcpkg), runs `npm ci`, and calls `tauri-apps/tauri-action`, which
   builds the bundles listed under `bundle.targets` in `tauri.conf.json`, signs each update
   archive with the updater key, uploads everything to a **draft** GitHub release named
   `Moony v<version>` and generates `latest.json` (version, notes, per-platform URL and
   signature). There is no `.msi`.

### A test build without a public release

To try the pipeline, or the installers, without publishing anything, tag the commit with a
pre-release suffix. The manifests keep their plain version, so no bump is needed:

```bash
git tag v0.9.0-test.1
git push origin v0.9.0-test.1
```

The run creates a **draft pre-release** with the full set of artifacts. When you are done,
delete the draft under *Releases*, then the tag
(`git push origin --delete v0.9.0-test.1 && git tag -d v0.9.0-test.1`).

## Review and publish the draft

1. Open *Releases* and edit the draft.
2. Check the attached files: one installer per platform, the update archives with their
   `.sig` files, and `latest.json`.
3. Check the body. `release.yml` writes a link to `CHANGELOG.md`, a download table with the
   exact file names, and first-launch steps for unsigned builds on macOS (`xattr`), Windows
   (browser warning and SmartScreen) and Linux; keep those sections and add a short
   highlights paragraph on top if the release deserves one. A tag with a hyphen must show as
   a pre-release.
4. Install the artifacts on clean machines — at least one macOS, one Windows and one Linux
   — and confirm the first-run wizard, unlock and the About version. Expect the Gatekeeper /
   SmartScreen warnings described in the README for as long as builds are unsigned.
5. Click **Publish release**. Only a published, non-pre-release release is served by
   `…/releases/latest/download/latest.json`.
6. Once an older version exists, install it on a machine, open Moony, let it check for updates
   and run the update end to end (download, signature verification, restart).

## Code signing and notarization (not set up yet)

Without OS-level signing, macOS Gatekeeper reports the app as "damaged" or from an
"unidentified developer", and Windows SmartScreen shows "Windows protected your PC". Users can
get past both (README, "Install"), but it costs trust and first-run conversions. Signing needs
accounts and money that the project does not have yet. Below is what the owner has to obtain;
once it exists, the workflow needs only environment variables and, for Windows, one extra step.

### macOS: Developer ID signing and notarization

1. **Apple Developer Program membership** (paid, yearly), enrolled as an individual or an
   organization.
2. **Developer ID Application certificate.** Create it in the developer account (Certificates,
   IDs & Profiles) or in Xcode, install it in the login keychain, and export it with its private
   key as a `.p12` file protected by a password. Encode it for a secret:
   `base64 -i DeveloperID.p12 | pbcopy`.
3. **Repository secrets** read by `tauri-action` / the Tauri CLI:

   | Secret | Value |
   |---|---|
   | `APPLE_CERTIFICATE` | the base64 of the `.p12` file |
   | `APPLE_CERTIFICATE_PASSWORD` | the password chosen when exporting |
   | `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
   | `APPLE_ID` | the Apple ID used for notarization |
   | `APPLE_PASSWORD` | an **app-specific password** for that Apple ID (appleid.apple.com → Sign-In and Security) |
   | `APPLE_TEAM_ID` | the 10-character team ID |

   An App Store Connect API key (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`) is
   an accepted alternative to the Apple ID / app-specific password pair.
4. **Notarization** is performed by the Tauri bundler when the variables above are present: it
   signs with the hardened runtime, submits the app with `xcrun notarytool` and staples the
   ticket. Both macOS legs (Apple Silicon and Intel) are signed this way. Check whether the app
   needs an entitlements file (`bundle.macOS.entitlements`); the Tauri defaults normally do not.
5. **Verify a build by hand:**

   ```bash
   codesign --verify --deep --strict --verbose=2 /Applications/Moony.app
   spctl --assess --type execute --verbose=4 /Applications/Moony.app
   xcrun stapler validate Moony_<version>_aarch64.dmg
   # manual notarization, if ever needed
   xcrun notarytool submit Moony_<version>_aarch64.dmg --apple-id <id> --team-id <team> --password <app-specific> --wait
   xcrun stapler staple Moony_<version>_aarch64.dmg
   ```

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

### When signing is in place

- Add the `APPLE_*` variables (and the Windows ones) to the `env:` block of the `tauri-action`
  step in `release.yml`; do not touch the build matrix.
- Remove the "Unsigned builds" section from the release body in `release.yml`, and update the
  unsigned-build paragraphs in `README.md` ("Install"), `SECURITY.md` (scope) and this
  playbook.
- Re-test the whole first-run path from a downloaded installer on a clean macOS and Windows
  machine, with the quarantine flag set (a download from the browser, not a local build).

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

**macOS says the app is "damaged"** (unsigned builds) — `xattr -cr /Applications/Moony.app`
removes the download quarantine flag, or use *Open Anyway* in System Settings → Privacy &
Security; this is the first-install workaround from the README. An update installed by the
in-app updater is written by Moony itself and normally does not carry the flag again — worth
confirming on a real update before promising it in release notes.

## Key files

| File | Purpose |
|---|---|
| `.github/workflows/release.yml` | Version check, build matrix and draft-release creation |
| `.github/workflows/ci.yml` | Gates run on every push and pull request |
| `.github/dependabot.yml` | Dependency update pull requests |
| `src-tauri/tauri.conf.json` | Version, identifier, bundle targets, updater public key and endpoint |
| `CHANGELOG.md` | Source of the release notes |
| `THIRD_PARTY_LICENSES.md` | Regenerated by `npm run licenses` |
| `~/.tauri/moony-updater.key` (outside the repo) | Updater private key |
