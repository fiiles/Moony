# 0002. schema.ts is the wire contract

Date: 2026-07-07

## Status

Accepted

## Context

As of this date, four type sources described the same Rust↔TypeScript wire
contract: hand-written `shared/schema.ts` (imported by ~70 files — the
contract in practice), `shared/generated-types.ts` (specta output, imported
by nothing), `shared/types/extended-types.ts` (redefines two schema.ts types
with conflicting field types), and ~20 inline interfaces in
`src/lib/tauri-api.ts`. Agent rule files contradicted each other about which
one was canonical — one even mandated tauri-specta, which is not installed —
and live drift already existed.

## Decision

The wire contract is set by these rules:

- **`shared/schema.ts` is the canonical wire contract.** Hand-written,
  hand-mirrored from the Rust structs; the frontend imports types only from
  it.
- **`shared/generated-types.ts` is a drift detector only** — never imported
  by app code. Regenerate with
  `cd src-tauri && cargo test generate_bindings -- --ignored`; CI regenerates
  and diffs it, so an out-of-sync committed file fails the build.
- **`shared/types/extended-types.ts` is deprecated** (do not extend; see
  `docs/DEPRECATED.md`), as are the inline interfaces in `tauri-api.ts`.
- **Full codegen migration (tauri-specta) is explicitly deferred** — it is
  source work, out of scope for the documentation and tooling.

## Consequences

- Adding or changing a command means following the 7-step procedure in
  `docs/standards/type-contract.md`, including the hand-mirror into
  schema.ts. Human error can still drift schema.ts from Rust; the generated
  file is the tripwire, not a guarantee.
- If schema.ts and Rust disagree, **Rust is the truth and schema.ts is the
  bug**.
- **Reconsider when:** schema.ts↔Rust drift incidents keep recurring despite
  the detector, or a feature forces bulk type churn — then evaluate migrating
  to end-to-end codegen (tauri-specta) and record it in a new ADR
  superseding this one.
