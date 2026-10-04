# Public release 0.9.0 in the new repository — design

Date: 2026-10-04 · Status: approved for execution (owner brief of 2026-10-04) · Scope: one release

## 1. Goal

Move Moony out of `fiiles/moony-tauri` into the empty public repository `fiiles/Moony` as a
**new application** at version **0.9.0**, test it there, and publish **1.0.0** once testing is
done. The repository must read as a finished, professional open-source project: no secrets, no
internal process artefacts, no placeholders, no dead code paths that a reader could call
unfinished. CI/CD is reduced to what is needed and cheap to keep alive: gates on pull requests,
GitHub Releases built from tags, and the in-app updater served from those releases.

## 2. Facts that shaped the design

- `fiiles/Moony` exists, is public and **empty** (created 2026-08-10). `fiiles/moony-tauri` is
  public, has releases `v0.2.0` … `v1.3.0` and no stars, forks or issues; `latest.json` was
  downloaded 180 times, the installers 1–2 times (the owner's own machines).
- The current tree carries version 1.3.0 in `package.json`, `src-tauri/Cargo.toml` and
  `src-tauri/tauri.conf.json`; the About dialog reads the version at runtime.
- The database chain is a 2026-08-06 baseline plus migrations 002–014, guarded by
  `golden_schema.snapshot`, with a legacy path for databases written by Moony ≤ 1.3.0. Six dead
  tables (`savings_accounts`, `savings_account_zones`, `instruments`, `purchases`,
  `transaction_rules`, `entity_history`) and the unused `csv_import_presets` are still created.
- The only production database is the owner's (`~/Library/Application Support/com.filipkral.moony-tauri/`).
- The GitHub runner image `ubuntu-22.04` used by `release.yml` has been in deprecation
  brownouts since 2026-09-17 and is retired on 2027-04-17. CI pins Node 20, which reached end
  of life in April 2026.
- Placeholders that must not ship: `security@TODO-OWNER-FILL-IN` (SECURITY.md, CODE_OF_CONDUCT.md),
  the Aptabase region `TODO(owner)` in PRIVACY.md.
- Process artefacts in the tree: `docs/audits/` (2.1 MB, Czech review reports, 72 screenshots,
  prompts for agents), `docs/plans/` and `docs/specs/` (47 frozen snapshots containing local
  paths and worktree names), `docs/design-system/prototypes/` and the agent brief,
  `docs/Categorization.md` and `docs/training_guide.md` (describe a removed ML classifier), an
  empty tracked file `src-tauri/src/utils_stub_touch_ignore`, test data naming the owner's
  employer, and two near-duplicate agent instruction files (`CLAUDE.md`, untracked `AGENTS.md`).

## 3. Decisions

### D1. Fresh history; the old repository is archived, not deleted

The new repository starts from **one commit** holding the 0.9.0 tree. The 611 commits of
`moony-tauri` reference review stages, audits and release tags (`v1.0.0`–`v1.3.0`) that would
contradict a 0.9.0 "new application" story. The full history stays in the local checkout on the
branch `archive/moony-tauri`, and the owner is advised to make `fiiles/moony-tauri` **private**
(or archive it) rather than delete it, so the history and the old releases remain reachable.

Alternative considered: pushing the full history. Rejected because the history contains the
old release tags and process-stage commit messages, and because `docs/audits`, `docs/plans`
and `docs/specs` would remain reachable in every old commit.

### D2. Versions and tags

`0.9.0` in the three manifest files and both lockfiles. Tags are `vX.Y.Z`; a tag with a
hyphen (`v1.0.0-rc.1`) is published as a GitHub pre-release, a plain tag as a full release.
`v0.9.0` is a **full release** so that the updater path (`…/releases/latest/download/latest.json`,
which skips pre-releases) can be exercised with a `0.9.x` follow-up before `1.0.0`. The
updater only offers higher versions, so anyone on a `1.3.x` build must install `0.9.0` by hand;
the release notes say so.

### D3. App identifier `com.filipkral.moony`

The `-tauri` suffix is a leftover of the move from the Node/Electron predecessor. A new
application is the only moment at which the identifier can change, because it names the data
folder. Consequences: the data folder, log folder and the Windows installer product change;
the owner's data is **copied** (never moved) into the new folder by the one-time conversion in
D4; documentation lists the new paths. `[lib] name` becomes `moony_lib`, the window title
`Moony`, and the bundle descriptions are unified to one sentence.

### D4. Data model: one baseline, no pre-release compatibility in the app

- `src-tauri/src/db/migrations.rs` keeps the append-only mechanism (`_migrations` table,
  `(name, sql)` list, one transaction per migration, pre-migration file backup) and starts
  again with a single hand-written, commented `001_initial_schema` that reproduces the current
  effective schema (baseline + 002–014) **minus** the seven dead or unused tables. Seed rows
  (`transaction_categories`, `institutions`) stay in the baseline.
- A `schema.snapshot` test (schema plus seed rows dumped from an in-memory database) guards the
  baseline the way `golden_schema.snapshot` did; the regeneration command is documented.
- The runner knows three states only: empty (`run the chain`), known names (`run pending`),
  **unknown names → typed error** "created by a pre-release build of Moony; this version cannot
  open it". No stamping path for Moony ≤ 1.3.0 or for the 2026-08 baseline chain remains in
  the public code.
- The owner's database is converted once by a **local, git-ignored tool**
  (`src-tauri/src/bin/local_*.rs` pattern): it copies the old data folder into the new
  identifier's folder, unlocks the copy with the password typed at a prompt (never an
  argument), drops the dead tables, resets `_migrations` to `001_initial_schema`, verifies that
  the resulting schema equals a fresh baseline schema, and runs `VACUUM`. The old folder is
  left untouched. The tool is tested end to end on the demo profile (`demo1234`).
- Code paths that only served dead tables go: the `EXPORT_DENYLIST` entries, the legacy
  `transaction_rules` commands and API wrappers (no UI calls them), comments about
  `entity_history`.

Alternative considered: a permanent "stamp a fully migrated pre-release database" path like the
2026-08 squash kept. Rejected: there are no external pre-release databases, and the path would
be unexplainable to a reader of a 0.9.0 code base.

### D5. The savings remnants are removed

`commands/savings.rs` disappears: its five account commands have no caller; the three zone
commands move to `commands/bank_accounts.rs` under the same names. `savingsApi` folds into
`bankAccountsApi` (`getZones`, `createZone`, `deleteZone`), the `savings` i18n namespace folds
into `bank_accounts` (`zones.*`), `components/savings/SavingsAccountZoneManager.tsx` becomes
`components/bank-accounts/BankAccountZoneManager.tsx`, `export_savings_accounts` and
`export_savings_account_zones` (no UI caller) are deleted, and the retired `savings` flag leaves
`MenuPreferences` (Rust model + `shared/schema.ts`; stored JSON with the extra key still parses).

### D6. CI/CD reduced to three files

- `ci.yml` — unchanged shape (Frontend, Rust) with Node 22, `Swatinem/rust-cache`, cargo-audit
  installed as a prebuilt binary, a `concurrency` group that cancels superseded runs, and
  read-only token permissions.
- `release.yml` — the same four legs with `ubuntu-24.04`, Node 22, a first step that fails when
  the tag does not equal the version in the three manifests, `prerelease` derived from the tag,
  a release body that points at `CHANGELOG.md` and the per-OS install notes, still created as a
  **draft** that the owner publishes after a look.
- `dependabot.yml` — security updates grouped per ecosystem plus **monthly** grouped
  minor/patch updates for npm and Cargo and weekly for Actions; majors stay manual.
- `dependency-check.yml` (weekly issue) is deleted; Dependabot covers it with less noise.
- Issue templates, PR template and `config.yml` point at `fiiles/Moony` (Discussions must be
  enabled by the owner).

### D7. Documentation set of the public repository

Removed: `docs/audits/`, the contents of `docs/plans/` and `docs/specs/` (both keep a one-paragraph
README; this spec and its plan are the first entries), `docs/Categorization.md`,
`docs/training_guide.md`, `docs/design-system/prototypes/`, `AGENT-PROMPT.md`,
`IMPLEMENTATION-NOTES.md`. CSV fixtures move to `src-tauri/tests/fixtures/csv/` (the preset
test reads them from there).

Kept and refreshed: `docs/architecture/` (overview without line counts or audit references,
`database.md` rewritten for the single baseline, ADR index plus **ADR 0010** recording D3 and D4),
`docs/standards/` (tool-neutral wording), `docs/playbooks/` (release playbook rewritten for the
new repository and runners), `docs/DEPRECATED.md` (without the entries that D4/D5 remove),
`docs/design-system/` (README trimmed to the design reference, `tokens.css`, `moony.css`,
`styleguide.html` and its three helper scripts), `docs/screenshots/` regenerated from the demo
profile on the current UI.

Root files: README (new URLs, identifier, badges, Node 22, screenshots), CONTRIBUTING (new
paths, fixture location, AGENTS.md mention, no tool names), SECURITY (private vulnerability
reporting is the single channel; no e-mail placeholder), PRIVACY (new paths; Aptabase described
without a region claim), CODE_OF_CONDUCT (contact = the private report form; the owner may add
an e-mail later), CHANGELOG (starts at 0.9.0 with a summary of the application; an empty
Unreleased section above), AGENTS.md becomes the canonical agent file and CLAUDE.md imports
it, `THIRD_PARTY_LICENSES.md` regenerated.

### D8. Analytics stay opt-in and compiled in only with a key

No product change. PRIVACY.md stops promising a region it cannot know; the owner can add the
region sentence once the key prefix is confirmed.

## 4. Out of scope

OS code signing (documented as missing), the website repository next door (links to
`moony-tauri` — owner follow-up), the remaining technical debt noted in the 2026-10-02 review
(English validation strings in Rust, synchronous crypto history, CF-01), dependency majors.

## 5. Owner-only steps (cannot be done from this session)

1. Repository settings of `fiiles/Moony`: enable **Discussions** and **Private vulnerability
   reporting**; disable Wiki and Projects; set the description and topics.
2. Secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (same key pair as
   before — the public key in `tauri.conf.json` is unchanged), optional `VITE_APTABASE_KEY`.
3. Run the local conversion tool once with the real password (command in the hand-over), then
   start 0.9.0 and check the data.
4. Push the tag `v0.9.0`, review and publish the draft release, install it on the three
   platforms, then later cut `0.9.1` to exercise the updater and `1.0.0` to launch.
5. Make `fiiles/moony-tauri` private (or archive it).

## 6. Verification

Gates (`npm run lint && npm run typecheck && npm test && npm run format:check`, `cargo fmt
--check && cargo clippy -- -D warnings && cargo test`, generated-types drift), a re-seeded demo
profile opened with the new build, the conversion tool run on a copy of the demo profile and
the result opened in the app, README screenshots from that profile, a tracked-file scan for
secrets, local paths and the old repository name, and the first CI run on the new repository.
