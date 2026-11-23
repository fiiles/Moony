# 0037. Demo profile: its own folder, a fixed key and gates in Rust

Date: 2026-11-23

## Status

Accepted. Spec: `docs/specs/2026-11-23-demo-profile-design.md` (approved 2026-11-23); plan:
`docs/plans/2026-11-23-demo-profile-app.md`; the data generator is `services::demo::seed`.

## Context

On first run a person can look at Moony on synthetic data (Alex Meyer, EUR) before creating an own
profile. The demo must open with no password, never mix with or touch a real profile, never leave
the device (sync, backup, MCP), and disappear once a real profile exists. Every path the backend
builds hangs off `app_data_dir()`, and the key files there decide whether a profile exists
(`check_setup`). The generator's services open their own transactions, so a generation cannot be
wrapped in one transaction.

## Decision

- **Folder.** The demo lives in `<app data folder>/demo/`: `moony.db`, `attachments.db` and
  `demo-profile.json` (the completeness marker, which also records the last-opened time). No
  `salt`, `key.enc` or `recovery.enc`; `check_setup` keeps reading the real folder only, so a demo
  never counts as a profile.
- **Fixed key.** Both databases use `DEMO_KEY_HEX` (`services/demo/profile.rs`, the bytes of
  `moony-demo-profile-not-a-secret.`). It protects nothing; it keeps one storage format (SQLCipher,
  the attached store of ADR 0017, migrations) for every profile.
- **One flag, one resolver.** `services/profile_dir.rs` holds the process-wide "demo is open" flag
  and `real_dir` / `demo_dir` / `open_profile_dir`. Every `app_data_dir()` call site either follows
  the open profile or deliberately stays on the real folder (audit below).
- **Lifecycle** (`services/demo/profile.rs`, thin commands in `commands/demo.rs`): `demo_status`,
  `demo_open(language)` (generates when missing; refused while a real profile exists or is
  unlocked), `demo_reset`, `demo_close` (the lock path, `commands::auth::end_session`, which also
  clears the flag), `demo_discard`. Opening sets the authenticated flag and runs the attachment
  conversion and the recalculation queue, not the MCP server, the sync engine or the biometric key
  check. A stored demo opened in another UI language switches only the profile's language.
- **Not atomic, so discard on failure.** The marker is written last. A generation that fails closes
  the database and deletes the folder; a folder without the marker, or whose database will not
  open, is generated again. Deleting checks that the folder is `<real folder>/demo`, is no symlink,
  holds none of a real profile's key files, and (for `discard`) that the demo is closed.
- **Gates** (`profile_dir::ensure_not_demo`, error `auth.notInDemo`, in ten locales): the sync
  engine start (`engine_may_start`, `spawn_engine`) and the sync commands that set up, pair, join or
  sync (`require_feature_outside_demo`; reading the sync state stays open), the MCP server
  (`LocalApiServer::start`, `set_mcp_server_enabled(true)`), biometric enable, backup create and
  restore, the password change, recovery and account deletion (services, plus an early check in the
  `restore_backup` and `delete_account` commands, which stop things before the service runs). The
  `backup_stale` reminder is not produced while the demo is open. Auto-lock is frontend-driven
  (`useAutoLock`), so the frontend turns it off in the demo; a lock that happens anyway ends the
  demo instead of leaving the app pointed at its folder.
- **Real profile comes to exist → the demo goes**, through `discard_beside_real_profile`: after
  `confirm_setup` (which first ends a demo still open; the wizard's sync-join path creates the
  profile here too and joins afterwards), after `restore_backup`, and at every start.

### Call-site audit (2026-11-23)

| Call site | Folder |
|---|---|
| `commands/attachments.rs` `data_dir` (export, open elsewhere, conversion, status, leftovers, phone save copy) | open profile |
| `commands/real_estate.rs` `data_dir` (property, photo and document deletes) | open profile |
| `commands/insurance.rs` `delete_insurance`, `delete_insurance_document` | open profile |
| `lib.rs` attachment URI scheme (`attachment_protocol::respond`) | open profile |
| `commands/backup.rs` `data_dir` (create, phone staging, `get_data_location`, `open_data_folder`) | open profile (create is gated) |
| `commands/backup.rs` `restore_backup` | real |
| `commands/auth.rs` `get_db_path` (`check_setup`, setup, unlock, recovery, password, deletion) | real |
| `commands/biometric.rs` (via `get_db_path`), keychain plugin's `moony.master-key.bin` | real |
| `commands/sync.rs` `data_dir`, engine `data_dir`, `verify_password`, `kit_owner`; `sync_pairing`, `sync_kit`, `sync_devices`, `sync_backups` | real (sync is gated) |
| `commands/device_backup.rs` `data_dir` (phone backup exclusion of the whole folder) | real |
| `lib.rs` legacy temp cleanup marker, phone staged-export sweep | real |
| `lib.rs` startup demo sweep | real (and its `demo/`) |

The decrypted copies opened in other apps live in the app cache folder, shared by both profiles and
wiped at every close of either.

## Consequences

- Synthetic data cannot reach a backup, the sync server or an MCP client, and the real profile's
  key files are never read or written by the demo.
- The fixed key is not a secret: anyone with the folder can read the demo. That is acceptable
  because it holds only generated data; nothing a person types in the demo is protected either,
  which the UI says.
- A new `app_data_dir()` call site must choose: `profile_dir::open_profile_dir` for what a running
  session keeps next to its database, the real folder for everything about the real profile.
- The flag is process-wide in the app and per test thread in tests, so the gates' tests run in
  parallel with everything else.
- Generating takes a second or two on first open; an interrupted generation costs a regeneration,
  never a half demo.
