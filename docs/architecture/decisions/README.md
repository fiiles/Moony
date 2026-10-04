# Architecture Decision Records

One ADR records one decision: the context that forced it, what was decided,
and what it costs. ADRs are dated point-in-time records — facts in them
(file names, counts) are true as of the ADR's date and are not kept fresh.

## Index

| # | Title | Date | Status |
|---|-------|------|--------|
| [0001](0001-czk-base-currency-and-money-as-text.md) | CZK base currency and money as TEXT | 2026-07-07 | Accepted |
| [0002](0002-schema-ts-is-the-wire-contract.md) | schema.ts is the wire contract | 2026-07-07 | Accepted |
| [0003](0003-tiered-ai-workflow.md) | Tiered AI workflow | 2026-07-07 | Accepted |
| [0004](0004-local-api-token-gated-writes.md) | Local API token-gated writes | 2026-07-07 | Superseded by 0006 |
| [0005](0005-yahoo-finance-for-prices.md) | Yahoo Finance for stock prices | 2026-07-07 | Accepted |
| [0006](0006-embedded-mcp-server.md) | Embedded MCP server over Streamable HTTP | 2026-08-12 | Accepted |
| [0007](0007-one-write-path-ui-and-mcp.md) | One write path for UI and MCP | 2026-08-12 | Accepted |
| [0008](0008-utc-calendar-days.md) | Calendar days are UTC-midnight epochs; periods bounded in UTC | 2026-10-02 | Accepted |
| [0009](0009-key-derivation-parameters.md) | Versioned scrypt parameters for the password key file | 2026-10-02 | Accepted |
| [0010](0010-new-identifier-and-single-schema-baseline.md) | New app identifier and a single schema baseline | 2026-10-04 | Accepted |

## Rules

- **ADRs are immutable.** An accepted ADR is never rewritten to say something
  different. To change a decision, write a **new ADR** with the next number
  that links back to the one it supersedes. The only edit ever made to an old
  ADR is flipping its `## Status` to `Superseded by [NNNN](NNNN-....md)`.
- Numbering is sequential and zero-padded; the next free number is `0011`.
- Filename: `NNNN-short-kebab-title.md`. Add every new ADR to the index table
  above.
- Keep each ADR to one page or less.

## Template

````markdown
# NNNN. Title

Date: YYYY-MM-DD

## Status

Accepted

## Context

The situation that forced a decision: facts, constraints, and the
alternatives that were on the table.

## Decision

What was decided, stated actively ("We use X", "Writes are Y").

## Consequences

What becomes easier, what becomes harder, what must now always be done,
and which risks are knowingly accepted.
````
