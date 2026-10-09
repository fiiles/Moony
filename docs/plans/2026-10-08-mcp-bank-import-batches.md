# MCP bank import batches implementation plan

**Goal:** Make MCP bank statement imports visible and deletable in account import history.

**Spec:** `docs/specs/2026-10-08-mcp-bank-import-batches-design.md`

**Architecture:** Reuse `csv_import_batches` and the existing CSV history/deletion
services. Keep batch creation, row insertion and counters inside the MCP writer's
single SQL transaction. Preserve original ownership during pending promotions.

**Tech stack:** Rust, rusqlite, rmcp/schemars; existing React account history.

## Implementation

- Add a minimal batch table to `services/mcp/bank_accounts.rs`'s in-memory test
  schema. Add regression tests that import rows, read their history and delete only
  those owned rows; repeat the import to verify duplicate-only history. Assert
  that input validation and a rejecting SQLite trigger leave no partial batch.
- Run `cargo test services::mcp::bank_accounts::tests` and observe the missing-batch
  failures before changing production code.
- Add optional `fileName` to `BankTransactionsCreateArgs`; update Rust test
  literals to omit it explicitly. Insert the batch before writes, link new rows,
  store the input date range and final counters. Use `api_sync` for new MCP rows.
- Extend the pending-promotion test to check both batches' linked counts and
  prove deleting the newer record preserves the older row.
- Document the behavior in the MCP tool description, README and living database
  documentation. No shared types, migration, user-facing copy or UI are added.
- Run the focused tests, then `npm run lint`, `npm run typecheck`, `npm test`,
  `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test`. Review the
  final diff for ownership, atomicity and backward compatibility.

## Addendum 2026-10-09: explicit legacy assignment

- RED service tests: named legacy rows are linked and deleted as one record
  while an unnamed row survives; repeated, unknown, other-account and
  already-owned ids are rejected without a record or partial link; unknown
  account errors and an empty request is a no-op.
- Implement `bank_transactions_assign_import` in `services/mcp/bank_accounts.rs`,
  register the tool with a confirmation-first description and a `maxItems` test,
  and document it in README, the database document and ADR 0007.
- Review follow-up: restrict eligibility to one earlier MCP call (`manual`,
  one-minute `created_at` cohort), force the `MCP` name prefix, date the record
  at the original write, guard the link UPDATE, and route both writers through
  shared `csv_import::insert_batch`/`finish_batch`.
