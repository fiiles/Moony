# ADR 0014: Durable backend bookkeeping after MCP writes

Status: Accepted (2026-10-12).

Supersedes the renderer-dependent refresh behavior described in ADR 0007.

## Context

MCP clients can write while the renderer is unavailable. An emitted event cannot guarantee a live snapshot, FX coverage or historical valuation refresh. Closing or locking the app can interrupt asynchronous work after a successful import.

## Decision

Persist a coalescing follow-up intent before every financial MCP mutation while holding the database connection lock. An unsuccessful mutation may leave a harmless refresh of unchanged facts. A successful mutation cannot commit without a previously durable intent. A generation token prevents completion of an older job from clearing a concurrent write.

The backend drains pending work after successful writes and after unlock/recovery. It records a live snapshot before network operations, extends FX coverage, refreshes market data and historical valuations, then records the fresh snapshot. Failures remain pending for the next mutation or unlock. MCP reports the result of the financial transaction; a network failure after commit must not encourage a client to repeat an already committed import.

UI events invalidate query caches and do not perform financial bookkeeping. Calendar comparisons use UTC days in the backend. Crypto identifiers used for valuations are resolved from committed holdings. Stock MCP imports use the UI's import-batch ownership and undo service. No migration guesses ownership or repairs ambiguous financial facts in existing data.

## Consequences

The schema gains a small queue. Offline imports remain usable and follow-up work is retried. Domains are coalesced, so consecutive imports do not launch independent rebuilds. Renderer absence cannot suppress bookkeeping. Network errors can delay derived valuations; they cannot roll back or duplicate committed transactions.
