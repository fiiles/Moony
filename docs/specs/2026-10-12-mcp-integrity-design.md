# MCP integrity remediation

Authorized scope: autonomously resolve all actionable findings in the 2026-10-12 MCP audit, including ambiguous duplicate fills, insurance retry identity and Stock Monitor partial writes. Historical personal data is not silently rewritten or assigned guessed import ownership.

All entry points enforce the same service invariants. Investment histories must never have negative chronological holdings, including backdated inserts; numeric inputs must be finite; calendar dates must be valid UTC days. Asset totals are derived atomically from transactions. Other-asset costs use the asset currency and dated EUR-reference FX; missing conversion fails atomically instead of inventing a rate. Existing enum inputs are normalized where case-insensitive compatibility was promised; real-estate type is restricted to personal/investment.

Imports use numeric and currency-aware identity. Stock MCP imports create the same undoable batches as UI imports. Explicit duplicate override allows genuine identical fills. Crypto identity comes from committed holdings, with conflicting identifiers rejected. Crypto transactions retain the UI's multiple-currency capability.

MCP post-write bookkeeping runs in the backend, independent of a mounted renderer. Durable pending work survives lock/restart and is retried on unlock or a later mutation. Live snapshots, FX coverage and historical recalculation use canonical UTC dates; frontend events invalidate query caches. Failed follow-up work remains pending rather than reporting the committed import as failed and encouraging duplicate retries.

Regression tests cover each audit case, rollback and retry behavior. Existing databases with nullable savings rates or malformed display dates remain readable. No backfill guesses how old transactions were imported.
