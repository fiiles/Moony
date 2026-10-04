# Playbook: Add (or Change) a Tauri Command

Checklist form of the 7-step procedure in `docs/standards/type-contract.md`, plus the
TDD service step. Rationale lives in the standards
(`docs/standards/rust-backend.md`, `docs/standards/type-contract.md`,
`docs/standards/testing.md`) — this file only tells you what to do, in order.

## Checklist

1. **Write the service function first (TDD).** In
   `src-tauri/src/services/<domain>.rs` (create the file and add `pub mod <domain>;`
   to `src-tauri/src/services/mod.rs` if the domain has no service yet):
   1. Add a **failing test** in the `#[cfg(test)]` module at the bottom of the file,
      using the in-memory `rusqlite::Connection` pattern from
      `docs/standards/testing.md` (never the `Database` struct).
   2. Implement the service function — it takes a plain `&Connection` — until the
      test passes.
2. **Write the command wrapper.** `#[tauri::command] pub async fn` in
   `src-tauri/src/commands/<domain>.rs`: call `data.validate()?` on any `Insert*`
   input, delegate via `db.with_conn(|conn| ...)`, return
   `Result<_, crate::error::AppError>` — never `Result<_, String>`
   (`docs/standards/rust-backend.md`).
3. **Declare the module.** New command file only: add `pub mod <domain>;` to
   `src-tauri/src/commands/mod.rs`.
4. **Register the handler.** Add the function to `tauri::generate_handler![]` in
   `src-tauri/src/lib.rs`, grouped under the domain's comment. (A missing entry
   compiles fine and fails only at runtime.)
5. **Model the types.** New/changed structs go in `src-tauri/src/models/<domain>.rs`:
   derive `Serialize, Deserialize, specta::Type`; **per-field**
   `#[serde(rename = "camelCase")]` annotations (not `rename_all`); `Insert*` structs
   implement `validate() -> Result<(), AppError>` returning `AppError::Validation`
   with an i18n key.
6. **Regenerate the drift detector.** Register new/changed types in `collect_types()`
   in `src-tauri/src/bindings.rs`, then:

   ```bash
   cd src-tauri && cargo test generate_bindings -- --ignored
   ```

   Commit the regenerated `shared/generated-types.ts` — CI diffs it.
7. **Hand-mirror into `shared/schema.ts`** — the file the frontend actually imports.
   Field names camelCase; money fields are `string`, never `number` (ADR 0001,
   `docs/architecture/decisions/0001-czk-base-currency-and-money-as-text.md`).
8. **Add the typed wrapper.** In `src/lib/tauri-api.ts`, under the domain's `*Api`
   namespace: snake_case command-name string, camelCase args object, return type
   imported from `@shared/schema`.

## Verify

```bash
cd src-tauri && cargo test && cargo clippy -- -D warnings
cd src-tauri && cargo test generate_bindings -- --ignored
git diff --exit-code shared/generated-types.ts   # clean once the regenerated file is staged/committed
npm run typecheck
```
