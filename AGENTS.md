# AGENTS.md

Guidance for AI coding agents working in this repository (Claude Code reads it through
`CLAUDE.md`; other agents read it directly). It is a thin router: the ten rules below are
non-negotiable; everything else lives under `docs/` (see the map below).

## Project overview

Moony is a privacy-focused personal finance desktop app built with Tauri 2 (Rust backend) and
React 19 (TypeScript frontend). All data is stored locally in an AES-256 encrypted SQLite
database (SQLCipher). No cloud sync.

## Commands

```bash
# Development
npm run tauri dev          # Start the full Tauri app (Rust + React with HMR)
npm run dev                # Frontend only (Vite dev server, no Tauri)

# Build
npm run tauri build        # Production build (Rust + React, creates installers)
npm run build              # Frontend only

# Code quality (run before committing)
npm run lint               # ESLint check
npm run lint:fix           # ESLint auto-fix
npm run format             # Prettier format
npm run format:check       # Prettier validation
npm run typecheck          # TypeScript type check (no emit)
npm test                   # TypeScript tests (Vitest)
npm run test:watch         # Watch mode
npm run test:coverage      # Coverage report

# Rust backend
cd src-tauri && cargo check        # Check Rust compilation
cd src-tauri && cargo clippy       # Rust linting
cd src-tauri && cargo test         # Rust tests
```

## The ten rules (non-negotiable)

1. Never call `invoke()` outside `src/lib/tauri-api.ts` → `docs/standards/typescript-frontend.md`
2. `shared/schema.ts` is the wire contract — follow the 7-step procedure whenever you touch it → `docs/standards/type-contract.md`
3. New Rust types: per-field `#[serde(rename = "camelCase")]`, register in `bindings.rs`, regenerate `generated-types.ts` → `docs/standards/type-contract.md`
4. Thin commands: business logic goes in `src-tauri/src/services/`, taking `&Connection`; use `AppError`, never `Result<_, String>` → `docs/standards/rust-backend.md`
5. `Insert*` structs get `validate()`; commands call it before DB work; no `.unwrap()` outside tests → `docs/standards/rust-backend.md`
6. Migrations are append-only in `migrations.rs`: next number + Vec entry; never edit the baseline or an applied migration → `docs/playbooks/add-db-migration.md`
7. Mutations invalidate domain keys + `["portfolio-metrics"]` + `["cashflow-report"]`, then `recordSnapshot()` + invalidate `["portfolio-history"]` (reference: `use-bank-account-mutations.ts`) → `docs/standards/typescript-frontend.md`
8. Every user-facing string lands in BOTH `en/` and `cs/` locale files → `docs/standards/i18n.md`
9. Check `docs/DEPRECATED.md` before copying any pattern; never resurrect removed code from history → `docs/DEPRECATED.md`
10. Money is TEXT, timestamps are unix epoch INTEGER, PKs are TEXT UUIDs, CZK is base → `docs/architecture/decisions/0001-czk-base-currency-and-money-as-text.md`

## Where the docs live

| Path | Contents |
|---|---|
| `docs/architecture/overview.md` | System map, data flow, canonical examples to copy |
| `docs/architecture/database.md` | Schema and migrations |
| `docs/architecture/decisions/` | ADRs (index in `README.md`) |
| `docs/standards/` | Per-layer rules: rust-backend, typescript-frontend, type-contract, testing, i18n, workflow |
| `docs/playbooks/` | Step-by-step checklists: command, migration, domain, page, release |
| `docs/DEPRECATED.md` | **Check before copying any pattern** |
| `docs/design-system/` | UI design reference: tokens, component contract, style guide |
| `docs/specs/` + `docs/plans/` | Frozen snapshots of past designs — never current truth |

## Workflow

- Tiered process per `docs/standards/workflow.md`: feature → spec + plan; small fix → TDD + verify.
- Specs go in `docs/specs/`, plans in `docs/plans/`.
- Gates: fast pre-commit / heavy pre-push / authoritative CI.

## Testing policy

Full policy including the thin-command amendment: `docs/standards/testing.md`.

### What to test vs. skip

| Test | Skip |
|---|---|
| Pure utility functions (`src/utils/`, `shared/`) | Tauri command handlers in `src-tauri/src/commands/` |
| Rust service business logic in `src-tauri/src/services/` | React components and hooks (out of scope) |
| | `shared/generated-types.ts` and `shared/schema.ts` (types only) |
| | `src/lib/tauri-api.ts` (thin invoke wrappers) |

### Rust test DB pattern

Each Rust service test module uses `Connection::open_in_memory()` with a hand-written minimal
schema — only the tables needed by that service. Follow the pattern in `services/budgeting.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(r#"
            CREATE TABLE ... ;
        "#).expect("schema");
        conn
    }

    #[test]
    fn test_something() {
        let conn = setup_test_db();
        // ...
    }
}
```

Do NOT use the `Database` struct in tests — it requires SQLCipher key setup. Use a raw
`rusqlite::Connection` directly.

## Type regeneration

```bash
cd src-tauri && cargo test generate_bindings -- --ignored
```

Regenerates `shared/generated-types.ts` from the Rust types registered in `bindings.rs`. It is a
drift detector only — `shared/schema.ts` stays the canonical wire contract (ADR 0002). CI fails
if the checked-in file is out of sync.

## Key path aliases

- `@` → `src/`
- `@shared` → `shared/`

## Environment variables

Moony needs none (see `.env.example`). The `npm run tauri` script still uses dotenv to load a
`.env` before running the Tauri CLI.
