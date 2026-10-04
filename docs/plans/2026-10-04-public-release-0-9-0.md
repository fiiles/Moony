# Public release 0.9.0 — implementation plan

Spec: `docs/specs/2026-10-04-public-release-0-9-0-design.md` · Branch: `release/0.9.0` (in place,
merged to `main` locally at the end) · Status: executed 2026-10-04

Each task names the files it owns; two tasks never edit the same file. Gates for every task:
`npm run lint && npm run typecheck && npm test && npm run format:check` and, in `src-tauri/`,
`cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

## Phase A (parallel)

### T1 Identity, versions, repository hygiene
Owns: `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, `src-tauri/src/bin/seed_demo.rs`,
`src-tauri/src/services/logging.rs` (doc comment), `src-tauri/src/services/categorization/tokenizer.rs`
(doc comment), `src-tauri/src/services/cashflow_actuals.rs` (test data), `src-tauri/src/services/csv_presets.rs`
(fixture path), `src-tauri/tests/fixtures/csv/*` (moved from `docs/audits/…/fixtures`),
`src/lib/app-links.ts`, `index.html`, `.claude/launch.json`, `.gitignore`,
`src-tauri/src/utils_stub_touch_ignore` (deleted).
- Version `0.9.0` everywhere; identifier `com.filipkral.moony`; `[lib] name = "moony_lib"`;
  window title `Moony`; one description sentence in Cargo.toml and tauri.conf.json; copyright
  `2024-2026` in both places; updater endpoint and `REPO_URL` → `https://github.com/fiiles/Moony`.
- `.gitignore`: drop entries for things that no longer exist (`mcp-server/`, `.agent/`,
  `docs/superpowers/`, `clippy_output.txt`, `migrate_history.rs`), add `src-tauri/src/bin/local_*.rs`
  (local one-off tools), `.codex/`, `.superpowers/`.
- Launch configurations renamed `moony-dev`, `moony-demo`, `moony-firstrun`, `moony-demo-alt`,
  `design-system`; window title in the inline config updated.

### T2 Single database baseline
Owns: `src-tauri/src/db/migrations.rs`, `src-tauri/src/db/golden_schema.snapshot` → `schema.snapshot`,
`src-tauri/src/db/mod.rs`, `src-tauri/src/services/backup.rs` (`EXPORT_DENYLIST` only),
`docs/architecture/database.md`, `docs/playbooks/add-db-migration.md`.
- Hand-merge baseline + 002–014 into `001_initial_schema`; drop `savings_accounts`,
  `savings_account_zones`, `instruments`, `purchases`, `transaction_rules`, `entity_history`,
  `csv_import_presets` (verify no live reference to each before dropping).
- Identity proof: a temporary test builds the old chain in memory, drops the seven tables, dumps
  schema + seed rows; the new baseline must produce the same dump. Keep the dump as
  `schema.snapshot` and a permanent test against it; remove the old chain and the legacy
  stamping path. Unknown `_migrations` names → `AppError::Database` naming a pre-release build.
- `database.md`: migrations section for the single baseline, catalog without "Since", no
  deprecated-tables section, no line-number references.

### T4 CI/CD and GitHub metadata
Owns: `.github/**`, `docs/playbooks/release.md`.
- `ci.yml`: Node 22, `Swatinem/rust-cache@v2`, `taiki-e/install-action@cargo-audit`, `concurrency`
  with cancel-in-progress, `permissions: contents: read`; same steps otherwise.
- `release.yml`: tags only (no `workflow_dispatch`); job `check-version` fails when the tag ≠
  version in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`; matrix on
  `macos-latest` (aarch64), `macos-15` (x86_64), `ubuntu-24.04`, `windows-latest`; Node 22;
  `prerelease: ${{ contains(github.ref_name, '-') }}`; draft release; new body.
- `dependabot.yml`: security groups + monthly minor/patch for npm and cargo, weekly actions.
- Delete `dependency-check.yml`. Templates and `config.yml` → `fiiles/Moony`.
- `release.md`: rewritten for the new repository, runners, version check, pre-release rule.

## Phase B (after A)

### T3 Remove the savings and legacy-rule remnants
Owns: `src-tauri/src/commands/{savings.rs (deleted), bank_accounts.rs, export.rs, mod.rs}`,
`src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src-tauri/src/models/**`,
`src-tauri/src/services/categories.rs`, `shared/schema.ts`, `shared/generated-types.ts`,
`src/lib/tauri-api.ts`, `src/hooks/**`, `src/pages/**`, `src/components/**`, `src/i18n/**`.
- Zone commands move into `commands/bank_accounts.rs` (same names); the five savings account
  commands, `export_savings_*`, and `get/create/delete_transaction_rule` go (after confirming no UI
  caller); `savingsApi` → `bankAccountsApi`; `savings` namespace → `bank_accounts.zones`;
  `SavingsAccountZoneManager` → `components/bank-accounts/BankAccountZoneManager.tsx`;
  `MenuPreferences.savings` removed in Rust and TS; regenerate `generated-types.ts`.

### T5 Documentation pruning and refresh
Owns: `docs/**` except the files owned by T2 and T4 and the two files of this release.
- Delete `docs/audits/`, `docs/plans/*` and `docs/specs/*` except their READMEs and the
  2026-10-04 pair, `docs/Categorization.md`, `docs/training_guide.md`,
  `docs/design-system/{prototypes/,AGENT-PROMPT.md,IMPLEMENTATION-NOTES.md}`.
- Rewrite `docs/design-system/README.md` as the design reference; refresh `overview.md`,
  `DEPRECATED.md`, `standards/workflow.md`, the ADR index; add ADR 0010 (identifier and baseline).

### T6 Root documents (orchestrator)
`README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `PRIVACY.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`,
`AGENTS.md` (canonical), `CLAUDE.md` (import), `THIRD_PARTY_LICENSES.md` (regenerated).

### T7 One-time conversion tool (orchestrator, local only)
`src-tauri/src/bin/local_convert_prerelease_db.rs` (git-ignored). Tested on a copy of the demo
profile; the owner runs it once on the real folder.

## Phase C (orchestrator)
Re-seed the demo profile, regenerate `docs/screenshots/`, run all gates, scan tracked files for
secrets and old names, merge to `main`, create the single-commit branch, push it to
`fiiles/Moony` as `main`, tag `v0.9.0` locally, hand over the owner-only checklist.
