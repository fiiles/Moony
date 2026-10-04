# Type Contract Standards

**The ruling (ADR 0002,
`docs/architecture/decisions/0002-schema-ts-is-the-wire-contract.md`):**
`shared/schema.ts` is the canonical wire contract — it is hand-written and it is what
the frontend imports. `shared/generated-types.ts` (specta output) is a **drift detector
only**: nothing imports it; CI diffs it to catch Rust-side type changes that were not
propagated. If schema.ts and Rust disagree, Rust is the truth and schema.ts is the bug.

## The 7-step procedure for adding or changing a command

Numbered, no step optional. Skipping a step ships silent drift or a runtime failure.

1. **Write the command.** `#[tauri::command]` in `src-tauri/src/commands/<domain>.rs`,
   thin per `docs/standards/rust-backend.md`.
2. **Declare the module.** If it's a new file, add the module to
   `src-tauri/src/commands/mod.rs`.
3. **Register the handler.** Add the function to `tauri::generate_handler![]` in
   `src-tauri/src/lib.rs` (a missing entry fails only at runtime).
4. **Model the types.** New/changed structs go in `src-tauri/src/models/<domain>.rs`:
   derive `specta::Type`, per-field `#[serde(rename = "camelCase")]` annotations.
5. **Regenerate the drift detector.** Register the type in `collect_types()` in
   `src-tauri/src/bindings.rs`, then run:

   ```bash
   cd src-tauri && cargo test generate_bindings -- --ignored
   ```

   Commit the regenerated `shared/generated-types.ts` — CI diffs it, and an
   out-of-sync file fails the build.
6. **Hand-mirror into `shared/schema.ts`.** This is what the frontend actually
   imports. Field names camelCase; money fields are `string` (they are TEXT on the
   wire — e.g. `currentPrice: string`, never `number`; see ADR 0001).
7. **Add the typed wrapper.** In `src/lib/tauri-api.ts`, under the domain's `*Api`
   namespace: snake_case command-name string, camelCase args object, return type
   imported from `@shared/schema`.

## Drift rules

- Never add a field on one side only — every field exists in the Rust struct, in
  `shared/schema.ts`, and (via regeneration) in `shared/generated-types.ts`.
- Never widen a type in `shared/schema.ts` beyond what Rust actually serializes
  (no optimistic `| null`, no extra optional fields "for later").
- `shared/types/extended-types.ts` and the inline interfaces in
  `src/lib/tauri-api.ts` are deprecated (see `docs/DEPRECATED.md`) — do not extend
  them; migrate types you touch into `shared/schema.ts` opportunistically.
- If `shared/schema.ts` and Rust disagree, Rust is the truth and schema.ts is the bug:
  fix schema.ts (and the frontend code relying on the wrong shape), never bend Rust to
  match a schema.ts mistake.
