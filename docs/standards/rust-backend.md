# Rust Backend Standards

Rules for all code under `src-tauri/src/`. Each rule states the why and points to a real
example in this repo. Copy the canonical examples — never the anti-examples (see also
`docs/DEPRECATED.md`).

## 1. Commands are thin

`#[tauri::command] pub async fn` validates input, delegates to a function in
`src-tauri/src/services/<domain>.rs` that takes `&Connection`, and maps the result.
**Why:** logic in command files is untestable without the full Tauri runtime; services
taking a plain connection are unit-testable in memory.

- Canonical: `src-tauri/src/commands/budgeting.rs` (thin wrappers) +
  `src-tauri/src/services/budgeting.rs` (all logic + tests).
- Anti-example (do not copy): `src-tauri/src/commands/portfolio.rs` — logic embedded in
  the command file.

## 2. Errors: always `AppError`

Return `crate::error::AppError` (it implements `Serialize` and crosses IPC as a string
message). Return `Err(AppError::NotFound(...))` when an UPDATE/DELETE affects 0 rows.
**Never** `Result<_, String>`.
**Why:** one error type keeps frontend error translation (`translate-api-error.ts`)
working; stringly-typed errors bypass it.

- Canonical: `src-tauri/src/error.rs` + any command in `commands/budgeting.rs`.
- Known violation, not a pattern: `commands/categorization.rs` uses `Result<_, String>`.

## 3. Validate at the trust boundary

Every `Insert*` struct implements `validate() -> Result<(), AppError>` returning
`AppError::Validation` with an i18n key (e.g. `"validation.bondNameRequired"`).
Commands call `data.validate()?` before any DB work. Never trust frontend data.
**Why:** the frontend is one client of many (local API is another); the backend is the
only enforcement point, and i18n keys let the UI show translated messages.

- Canonical: `src-tauri/src/models/bonds.rs` (`InsertBond::validate()`).

## 4. No `.unwrap()` outside tests

Use `?` or `.expect("reason")` — `.unwrap()` is allowed only inside `#[cfg(test)]`.
Beware `.parse().unwrap_or(0.0)` on money strings: it silently zero-coerces bad data
into wrong balances; prefer propagating a parse error.
**Why:** a panic in a command crashes the whole app; silent zero-coercion corrupts
money math without any error (see ADR 0001).

## 5. DB access pattern

Commands take `db: State<'_, Database>` and call `db.with_conn(|conn| ...)`. Service
functions take a plain `&Connection`.
**Why:** services stay testable with `rusqlite::Connection::open_in_memory()` — no
SQLCipher key setup needed (see `docs/standards/testing.md`).

- Canonical: `src-tauri/src/commands/budgeting.rs` +
  `src-tauri/src/services/budgeting.rs`.

## 6. Command registration — 3 manual points, all required

1. The function in `commands/<domain>.rs`.
2. The module declaration in `commands/mod.rs` (for new files).
3. An entry in `tauri::generate_handler![]` in `src-tauri/src/lib.rs` (grouped under
   the domain's comment).

**Why:** a missed `lib.rs` entry compiles fine and fails only at runtime when the
frontend invokes the command.

## 7. Serde conventions

Derive `Serialize, Deserialize, specta::Type` and use **per-field**
`#[serde(rename = "camelCase")]` annotations. This is the project style — 334 existing
per-field renames; do not switch to `rename_all`.
**Why:** a forgotten rename silently ships snake_case to the frontend and breaks the
wire contract without a compile error (see `docs/standards/type-contract.md`).

- Canonical: any struct in `src-tauri/src/models/`.

## 8. State, blocking work, logging

- Shared state: Tauri managed state + `Arc` — example: `CategorizationState` in
  `src-tauri/src/commands/categorization.rs`.
- CPU-heavy work: `tauri::async_runtime::spawn_blocking` (same file, `categorize_transaction`).
- Logging: the `log` macros (`log::info!`, `warn!`, `error!`, `debug!`) with bracketed
  prefixes (`[MIGRATION]`, `[DB]`) — never `println!`: only `log` output reaches the
  app log file (`services/logging.rs`). Release builds write `info` and above,
  so anything that reveals the portfolio or personal data (tickers, payee names, IBANs,
  search queries, amounts, file contents) belongs in `log::debug!`, never above it.

**Why:** consistency — agents grep for these patterns; ad-hoc alternatives fragment
the codebase.

## 9. Migrations

Append-only, per `docs/architecture/database.md`: add the next-numbered
`const MIGRATION_0NN` plus its entry in `all_migrations()`; never edit an applied
migration.
**Why:** migrations already applied to users' encrypted databases cannot be re-run;
editing one desyncs schemas across installs.
