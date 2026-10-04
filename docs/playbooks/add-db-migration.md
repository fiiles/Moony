# Playbook: Add a Database Migration

Mechanics and rationale are owned by `docs/architecture/database.md` ("How Migrations
Work" + "Storage Conventions") and `docs/standards/rust-backend.md` rule 9. This file
is the execution checklist only.

## Checklist

1. **Find the next number.** Open `src-tauri/src/db/migrations.rs` and read the
   entries returned by `all_migrations()`. Yours is the next number after the last
   one — zero-padded, sequential, never reused (the chain starts with the baseline
   `001_initial_schema`). Use the same number in the const name and the name string.
2. **Define the const.** Add `const MIGRATION_0NN: &str = r#"..."#;` with inline SQL
   (there are no `.sql` files). Follow the storage conventions in
   `docs/architecture/database.md`: TEXT UUID primary keys, INTEGER unix-epoch
   timestamps (`DEFAULT (unixepoch())`), money/decimals as TEXT strings, CZK-base
   aggregates, per-currency breakdowns as JSON-in-TEXT.
3. **Append the Vec entry.** Add `("0NN_short_name", MIGRATION_0NN)` to the `Vec` in
   `all_migrations()` in the same file.
4. **Never edit or rename an applied migration** (including `001_initial_schema`) —
   databases track migrations by name, so the fix is always a new migration. If you
   need to alter existing columns (SQLite has no `ALTER COLUMN`), use
   **create-new → copy → drop → rename**: create `table_new` with the desired
   shape, `INSERT INTO ... SELECT` the data across, drop the old table and its
   indexes, rename `table_new` back, recreate indexes.
5. **Do not add out-of-band repair checks** outside the numbered migration list.
6. **Regenerate the schema snapshot.** The `baseline_matches_schema_snapshot` test
   fails on any schema change until you run
   `cargo test regenerate_schema_snapshot -- --ignored` (in `src-tauri/`) and commit
   the updated `src-tauri/src/db/schema.snapshot` with the migration. Read the diff:
   it must show exactly the change you intended.
7. **Update the docs in the same commit.** Add/amend the table's row in the catalog
   in `docs/architecture/database.md` (new table → new row; changed columns → amend
   "Notable columns / constraints").

## Verify

```bash
cd src-tauri && cargo test
```

If the migration affects schema used by the app (any new/changed table or column),
also launch the app once — `npm run tauri dev` — and confirm the `[MIGRATION]` log
lines show the new migration applying without error.
