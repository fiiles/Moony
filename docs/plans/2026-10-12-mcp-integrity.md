# MCP integrity implementation plan

1. Securities: chronological sell guards, persisted crypto identity, undoable stock import batches, numeric/currency deduplication, explicit duplicate override and crypto currency parity. Add regression tests before fixes.
2. Assets: initial-transaction totals, finite values, dated other-asset currency conversion, canonical property types and insurance status/identity, atomic Stock Monitor validation. Add service regressions.
3. Trust boundaries: shared finite/date validation, bank rate defaults and enum normalization, legacy NULL-rate reads, safe display of malformed legacy dates. Normalize financial calendar days at shared writers.
4. Backend effects: durable pending MCP bookkeeping, snapshot and FX/history refresh without renderer, unlock recovery, query invalidation and authoritative crypto history identity. Add queue/retry regressions.
5. Integrate domains, update architecture and contract docs, regenerate bindings if necessary, run Rust/TypeScript gates and review changes independently. Resolve review findings and rerun affected checks.

No commit, push or production-data migration beyond append-only schema migrations is needed to deliver reviewable changes.
