# 0007. One write path for UI and MCP

Date: 2026-08-12

## Status

Accepted

## Context

MCP write tools let an LLM import parsed exports and categorize payments.
The first write tool (insurance_create, ADR 0006) duplicated the UI
command's SQL and diverged within days. Several create commands carried
inline SQL with no service layer; bank transactions had no validation at
all; nothing in the app detected duplicate imports; multi-row writes had
no transactional guarantees.

## Decision

- Every domain has exactly one creation function in services/<domain>.rs,
  taking &Connection. Input validation runs at the trust boundary: in the
  creation function itself where the Insert struct is its argument, or in
  both callers for the stock/crypto import functions (whose per-row
  validation precedes the batch). Tauri commands (UI) and MCP tools are
  both thin wrappers.
- MCP writes are create-only, with two narrow exceptions:
  bank_transactions_categorize may set the category field (only) of existing
  transactions, to existing categories only; bank_transactions_assign_import
  (2026-10-09, see Consequences) may set the import ownership (only) of
  unowned transactions of one earlier MCP import call.
- Bulk tools are two-phase and atomic: validate everything up front (any
  failure rejects the batch, nothing written), then insert inside one SQL
  transaction. Duplicates (services/dedup.rs: bank-side transaction id, else
  strict composite match) are skipped and reported, never errors.
- Stock/crypto holdings are created implicitly by their first buy — the same
  behavior as the UI CSV import. New coins require a coingeckoId.
- After successful writes the server emits mcp-data-changed; the frontend
  performs the same invalidation/snapshot/price-refresh ritual as UI
  mutations (AGENTS.md rule 7). Tools stay sync and network-free (ADR 0006).

## Consequences

- A validation fix lands once and covers both callers; the UI inherited
  validation (bank transactions) and existence checks it never had.
- The dedup module makes re-importing overlapping exports safe by default;
  the UI CSV import can adopt it later.
- ~~Known holdout: `commands/bank_accounts.rs::import_csv_transactions` still
  raw-INSERTs bank transactions~~ — resolved 2026-08-19: the CSV import now
  lives in `services/csv_import.rs::import_transactions`, writing through
  `create_transaction_with_origin` (validate(), lowercase normalization,
  account-currency fallback) with dedup skipping inside one SQL transaction.
  It stays row-tolerant (bad rows are reported, good rows import) rather than
  all-or-nothing, matching the interactive UI contract.
- Write tools multiply: every new one must be listed in the README (rule
  carried from ADR 0006) and must go through a service function — a tool
  with inline SQL is a review-blocking defect.
- Accepted risk: an LLM with the bearer token can create records,
  re-categorize payments and group one earlier MCP import call into an
  import record. It cannot modify or delete financial facts.
- 2026-08-19: imports are now rules-first, and the rule applies identically to both
  callers of the shared write path. `bank_transactions_create` and the UI
  CSV import run the app's own categorization waterfall on every row that
  arrives without an explicit `categoryId`, before the INSERT, and
  auto-apply based on the *result variant* the engine returns:
  `CategorizationResult::Match` (custom rules, learned-payee exact match,
  own-account IBAN, default system rules) is applied; `Suggestion` is not.
  The ML classifier's `CategorizationResult::Suggestion` is deliberately
  excluded from auto-apply — it is "for user to confirm", not fact — so a
  row it would only guess at stays uncategorized for a human or an LLM to
  decide. The learned-payee layer can itself return `Suggestion`, not just
  `Match`: a partial-IBAN hit is a 0.70-confidence guess, same shape as the
  ML classifier's answer. Pre-existing engine behavior, surfaced (not
  introduced) by this change: because that layer returns as soon as it has
  any answer, a partial-IBAN `Suggestion` short-circuits the waterfall
  before the default system rules run, so such a row can import
  uncategorized even when a later default rule would have matched it.
- `bank_transactions.categorization_source` is written for the first time:
  `rule` / `exact_match` / `own_account` for an auto-applied import match,
  `mcp` when the categorize tool sets a category. NULL still means "no
  recorded provenance" — which today covers three cases, not two: a
  genuinely uncategorized row, the pre-existing manual UI path
  (`update_transaction_category` passing `None`), and the UI's
  "Auto-categorize" button (`BankAccountDetail.tsx`), which also persists
  engine-derived matches through that same `None`-source call. That button
  can stamp an identical rule-derived category with NULL where an import
  would have stamped `rule`/`exact_match`/`own_account`; giving it its own
  provenance label is a known follow-up, not yet done.
- `bank_transactions_categorize` is non-destructive by default: a
  transaction that already carries a category (manual or rule-derived) is
  skipped and reported rather than overwritten, unless the caller passes
  `overwrite: true`. This is the MCP-only exception ADR 0007 already
  carves out for re-categorizing existing rows — it now defaults to
  protecting prior work instead of blindly replacing it. It reuses
  `BulkWriteReport.skipped_duplicates` for this — that field now carries
  "already categorized, left alone" rows in addition to genuine dedup
  duplicates, so a reader of that type should not assume every entry in it
  is a duplicate.
- The engine is deliberately NOT trained on LLM assignments: the MCP
  categorize path never calls `learn_categorization`, so the exact-match
  layer only ever learns from a human correcting a category in the UI. An
  LLM's guesses cannot teach the engine to repeat themselves.
- 2026-08-20: every bulk write's array argument (`transactions` on
  `bank_transactions_create`, `stock_transactions_create`,
  `crypto_transactions_create`, `other_asset_transactions_create`;
  `assignments` on `bank_transactions_categorize`) is capped at
  `MAX_BULK_ROWS = 200` (`services/mcp/mod.rs`), enforced as the first
  check in each function, before any other validation or DB work — an
  oversized call gets `AppError::Validation` and nothing is written. The
  cap is a discoverability fix, not a performance one: a 432-row bank
  import had no `maxItems` anywhere in the advertised tool schema and no
  size guidance in the tool description, so an LLM handed a large export
  had no signal to chunk it and instead emitted the whole batch (~173KB of
  arguments) in one call, which the client failed to send reliably. The
  server itself processes batches like that in tens of milliseconds, so
  the fix is entirely about making the limit visible to the caller: the
  cap is advertised as `maxItems` in each tool's JSON schema (via
  `#[schemars(length(max = MAX_BULK_ROWS))]` on the Vec field) and spelled
  out in the tool description, which recommends batches of about 100 rows
  and says splitting a larger export across calls is safe because
  duplicate/already-handled rows are skipped and reported. No minimum was
  added — an empty batch stays a harmless no-op.
- 2026-10-09: MCP bank imports record a `csv_import_batches` entry per
  validated nonempty call, like CSV uploads, so they can be deleted from the
  account's import history. Rows imported before that have no owner and
  their origin cannot be inferred, so `bank_transactions_assign_import` is a
  second, narrow exception to create-only: it creates one history record and
  sets `import_batch_id` (only) on explicitly listed transactions of one
  account that belong to no import. Because an LLM thereby decides what a
  later human deletion removes, eligibility is limited to what one earlier
  MCP call looks like: `manual` data source (never CSV uploads), no owner,
  `created_at` within one minute. The record is always named `MCP` /
  `MCP · <name>` so it cannot pose as a CSV upload, and is dated at the
  original write. Amounts, dates, categories and existing ownership are never
  changed; any ineligible id rejects the whole call. Deleting the record stays
  a human action in the app, whose confirmation states how many transactions
  it removes. Rows typed in the app are also `manual` and unowned, so any of
  them written within one minute (even a single row, with no MCP import
  nearby) remain eligible; the `MCP` name and the human confirmation are the
  remaining guards — the residual risk accepted here.
- New MCP bank rows use the existing `api_sync` data source to avoid a wire
  change. A future real bank-sync feature must not assume `api_sync` means
  bank API rows only.
- Batch rows are written only through `csv_import::insert_batch` and
  `finish_batch`, shared by the CSV and MCP writers.
