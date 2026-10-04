# Security policy

Moony keeps people's financial records, so security reports are taken seriously. Moony is
maintained by one person; the timelines below are best-effort commitments, not a service
level.

## Supported versions

Only the **latest release** receives security fixes. If you are on an older version, update
first (Moony offers updates in the app, and every release is listed on the GitHub Releases
page) and check whether the problem still exists.

## Reporting a vulnerability

**Please do not open a public issue, discussion or pull request for a security problem.**

Use **GitHub private vulnerability reporting**: on the repository open *Security → Report a
vulnerability* (<https://github.com/fiiles/Moony/security/advisories/new>). Only the maintainer
can read what you submit there. If you cannot use that form, open an ordinary issue that says
just "security report, please contact me" without any detail, and the maintainer will arrange a
private channel.

A useful report contains:

- what is affected (version of Moony, operating system) and what an attacker gains;
- steps to reproduce, or a proof of concept (a crafted CSV, backup archive, HTTP request …);
- whether you have shared the details with anyone else.

Please never include real financial data. The demo profile (`cargo run --bin seed_demo`,
see [CONTRIBUTING.md](./CONTRIBUTING.md#demo-data)) is a safe place to reproduce things.

## What to expect

| Step | Target |
| --- | --- |
| Acknowledgement of your report | within 7 days |
| First assessment (confirmed, needs more information, or not a vulnerability) | within 14 days |
| Fix or mitigation for a confirmed issue | within 90 days, usually much sooner |
| Public disclosure | after a fixed release is available, or after 90 days, whichever comes first — earlier or later by agreement with you |

Confirmed vulnerabilities are published as a GitHub security advisory (with a CVE where it
makes sense) and noted under *Security* in [CHANGELOG.md](./CHANGELOG.md). You are credited
by name or handle unless you prefer to stay anonymous.

## Scope

In scope — anything that lets an attacker who does **not** know the user's password or
recovery key get at, or tamper with, Moony data, or run code through Moony:

- **Local encryption**: the SQLCipher database, key derivation and wrapping, the key files
  (`salt`, `key.enc`, `recovery.enc`), recovery-key handling, how keys are held in memory.
- **The MCP / local API**: the HTTP server on `127.0.0.1`, its bearer-token authentication, and
  the write tools reachable through it.
- **The updater**: update discovery, signature verification, the endpoint and the public key
  in `src-tauri/tauri.conf.json`.
- **File handling**: backup archive validation (path traversal, unexpected entries), restore,
  export, CSV and image import parsing, attachment storage paths.
- **The desktop boundary**: the content-security policy, Tauri capabilities and command
  surface — for example a script-injection bug that could reach the Rust backend.
- **Supply chain**: the release workflow and the way dependencies are pinned and audited.

Out of scope:

- An attacker who already controls an **unlocked** Moony session, or the user's account or
  device (malware, a keylogger, someone using the computer while the app is unlocked).
  Lock the app, use full-disk encryption and a strong password.
- **Attachments are stored unencrypted** (photos and documents in the data folder). This is
  documented in [PRIVACY.md](./PRIVACY.md) and is a known roadmap item, not a new finding.
- Weak or reused passwords chosen by the user, and loss of both password and recovery key
  (there is deliberately no backdoor).
- Releases being **unsigned** by Apple and Microsoft for now (see the install notes in
  [README.md](./README.md#install)); the in-app updater still verifies its own signature.
- What a third-party AI client does with data after you connected it over MCP.
- Vulnerabilities in dependencies with no demonstrable effect on Moony — please report them
  upstream. Reports that only restate the output of an automated scanner, denial of service of
  the local MCP port, and missing hardening headers on `localhost` are also out of scope.

## Safe harbour

Good-faith research that stays within this policy is welcome. Test only against your own
data or the demo profile, do not access or destroy other people's data, and give a
reasonable time to fix a problem before you go public.

## How we keep dependencies and releases in check

- `cargo audit --deny warnings` runs in CI and blocks the build on any advisory (including
  unmaintained or yanked crates); see [`.github/workflows/ci.yml`](./.github/workflows/ci.yml).
- Dependabot opens security-update pull requests for npm, Cargo and GitHub Actions
  ([`.github/dependabot.yml`](./.github/dependabot.yml)).
- Releases are built by GitHub Actions from version tags; the in-app updater only installs
  archives signed with the project's updater key. The release process and what is still
  missing (platform code signing) are described in
  [`docs/playbooks/release.md`](./docs/playbooks/release.md).
- A third-party licence inventory is in [THIRD_PARTY_LICENSES.md](./THIRD_PARTY_LICENSES.md).
