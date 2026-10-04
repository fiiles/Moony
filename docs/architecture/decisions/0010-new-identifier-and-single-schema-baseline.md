# 0010. New app identifier and a single schema baseline

Date: 2026-10-04

## Status

Accepted

## Context

Moony is re-published as a new application, version 0.9.0, in a new
repository. There are no external users of earlier builds: the only database
in existence belongs to the maintainer.

The inherited state did not fit that story:

- The schema was a 2026-08 baseline plus migrations `002`–`014`, with a legacy
  path that stamped databases written by earlier builds. A reader of a 0.9.0
  code base could not explain why that path exists.
- Seven tables were no longer used by any code path: `savings_accounts`,
  `savings_account_zones`, `instruments`, `purchases`, `transaction_rules`,
  `entity_history` and `csv_import_presets`.
- The app identifier carried a `-tauri` suffix left over from the
  predecessor, and the identifier names the data folder. A new application is
  the only moment at which it can change without stranding anyone's data.

## Decision

- **The app identifier is `com.filipkral.moony`.** The data folder is
  `~/Library/Application Support/com.filipkral.moony/` on macOS,
  `%APPDATA%\com.filipkral.moony\` on Windows and
  `~/.local/share/com.filipkral.moony/` on Linux; logs move with it.
- **The schema is one hand-written baseline, `001_initial_schema`**, in
  `src-tauri/src/db/migrations.rs`, including the seed rows of
  `transaction_categories` and `institutions`. A `schema.snapshot` test dumps
  the schema and seed rows of an in-memory database and fails on any
  difference; the regeneration command is documented in
  `docs/playbooks/add-db-migration.md`.
- **Migrations are append-only from the baseline.** The runner knows three
  states: an empty `_migrations` table (run the chain), only known names (run
  the pending ones) and any unknown name. A database with an unknown name was
  created by a pre-release build and is refused with a typed error; there is no
  stamping or repair path.
- **The seven tables listed above are gone**, together with the code that
  served them.
- **The maintainer's single production database is converted once** by a
  local, untracked tool that copies the old data folder into the new
  identifier's folder, drops the dead tables, resets `_migrations` to
  `001_initial_schema`, verifies the result against a fresh baseline and runs
  `VACUUM`. The old folder is left untouched.

## Consequences

- The data folder moves exactly once. Anyone who ran a pre-release build starts
  from the new folder; the old one is never read by the app.
- Pre-release builds cannot open 0.9.0 databases, and 0.9.0 refuses theirs.
  This is deliberate: it keeps a compatibility path out of the public code.
- **The identifier must never change again.** It names the data folder, the
  log folder and the installer product; changing it strands every installation.
- The migration playbook applies unchanged: next number, new `(name, sql)`
  entry, regenerated snapshot, never edit or rename an applied migration.
- The baseline is a snapshot of the data model at 0.9.0, not a record of how
  it evolved; history before it lives only in the maintainer's archive.
