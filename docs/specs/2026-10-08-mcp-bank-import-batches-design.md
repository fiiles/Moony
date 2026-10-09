# MCP bank import batches

Bank statement imports through `bank_transactions_create` currently store rows as
manual transactions without import ownership. Deleting CSV imports therefore
cannot remove those rows. MCP imports should appear in the existing account import
history and support the same deletion behavior as CSV uploads.

Each successful, validated nonempty MCP request creates one `csv_import_batches` record in
the same SQL transaction as its writes. An optional `fileName` identifies the
original statement; omitted or blank names use `MCP`. The record retains the input
booking-date range and counts of inserted, skipped duplicate and promoted rows.
New rows use the existing `api_sync` data source and reference that batch. Same-ID
pending promotions retain the stored row's original ownership. Duplicate-only
requests create history records with zero owned rows, as CSV imports do. Invalid
requests and failed writes leave no batch or partial transactions. Empty requests
remain a no-op.

No database migration or shared wire-contract change is required. The MCP input
schema adds only an optional field, so existing clients remain compatible. Existing
unbatched rows cannot be safely assigned a historical source automatically and are
left untouched. This applies to bank transactions, not broker trade imports.

Verification uses raw in-memory SQLite service tests for history, deletion scope,
repeat imports, promotion ownership, validation and SQL rollback, followed by the
repository's frontend and Rust gates.

## Addendum 2026-10-09: explicit assignment of earlier rows

Rows imported through MCP before batching remain unowned and cannot be deleted
in one step. Their origin still cannot be inferred, so nothing is assigned
automatically. A separate tool, `bank_transactions_assign_import`, takes an
account id, an optional record name and up to 200 explicit transaction ids
that the user confirmed. Eligible rows look like one earlier MCP call: unowned,
`manual` data source, same account, `created_at` within one minute. Repeated,
unknown, other-account, already-owned, CSV-sourced or out-of-cohort ids reject
the whole call. Otherwise one SQL transaction creates a history record named
`MCP` or `MCP · <name>`, dated at the original write, with its imported count
and booking-date range, and sets only `import_batch_id` on those rows. Deleting the record in the app then removes exactly them. ADR 0007
records this as a narrow exception to create-only MCP writes.
