# Testing Standards

## When to run tests

- Before claiming any task complete, run `npm test` (TypeScript) and
  `cd src-tauri && cargo test` (Rust) and verify they pass.
- When modifying a file that has a corresponding test file, run the relevant suite
  before and after the change.
- When adding new business logic to a service, add at least one test covering the
  happy path before the task is done.

Do NOT say "I'll add tests later" — tests are part of the definition of done for new
logic (see `docs/standards/workflow.md`).

## When to write new tests

- Any new pure function in `src/utils/`, `src/lib/`, or `shared/` must have tests
  alongside it in a co-located `.test.ts` file.
- Any new Rust service function with non-trivial logic (calculations, parsing, state
  transitions) must have at least a happy-path test and one error/edge case in a
  `#[cfg(test)]` module at the bottom of the service file.
- Bug fixes in tested code must include a regression test that **fails before the fix
  and passes after**.

## What to test vs. skip

| Test | Skip |
|---|---|
| Pure utility functions (`src/utils/`, `shared/`) | Tauri command handlers in `src-tauri/src/commands/` — *see the thin-command amendment below* |
| Rust service business logic in `src-tauri/src/services/` | React components and hooks (out of scope) |
| | `shared/generated-types.ts` and `shared/schema.ts` (types only) |
| | `src/lib/tauri-api.ts` (thin invoke wrappers) |
| | `src/lib/analytics.ts` (side-effectful, requires jsdom+mocks) |

### The thin-command amendment

> Command handlers are exempt from testing **only while they are thin** (validate →
> delegate → map). Business logic living in a command file (calculations, aggregation,
> multi-step SQL transactions) counts as service logic: when you touch it, either move
> it to `src-tauri/src/services/` (with tests) or add `#[cfg(test)]` tests in place.
> "It's in commands/" is not an exemption for logic.

## Vitest environment

Vitest runs with `environment: "node"` (see `vite.config.ts`) — **no jsdom, no
component/hook tests; do not change the test environment.** Frontend tests cover pure
functions only.

## Rust test DB pattern

Each Rust service test module uses `Connection::open_in_memory()` with a hand-written
minimal schema — only the tables needed by that service. Follow the pattern in the
`#[cfg(test)]` module of `src-tauri/src/services/budgeting.rs`:

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

Do NOT use the `Database` struct in tests — it requires SQLCipher key setup. Use raw
`rusqlite::Connection` directly.

## Coverage thresholds

Coverage thresholds are a **planned** gate, not yet enforced. Enabling them is blocked
on the untested modules in `shared/calculations/` — turning them
on today would fail the build on pre-existing gaps rather than on new code.
