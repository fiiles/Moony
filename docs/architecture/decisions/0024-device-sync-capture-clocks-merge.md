# 0024. Device sync: trigger capture, hybrid clocks and a per-column merge

Date: 2026-11-07

## Status

Accepted. Records phase 1 of `docs/specs/2026-11-06-device-sync-client-design.md` as built
(`src-tauri/src/services/sync/`); the spec's §20 lists where the build departs from its text.
Amended by [ADR 0027](0027-device-sync-audit-fixes.md) (2026-11-12): capture or guard, the
unlock sweep, far-future clocks and the `sync_devices.public_key` rule.

## Context

Device sync must merge offline edits from several devices of one person without the relay seeing
any data, and every device must end with identical synced tables whatever order changes arrive in,
including orders a dishonest relay picks. Moony writes through ~900 SQL statements in the services,
MCP and imports, and keeps derived values (mirrors, positions, split units, names a repair shows)
that a plain last-writer-wins merge would make path-dependent. A convergence harness (three
devices, random operations through the real services, random and adversarial delivery orders)
found the cases below; every rule here exists because a seed diverged without it.

## Decision

- **D4 Capture by triggers.** `TEMP` triggers generated at unlock from the registry
  (`services/sync/registry/`) and `PRAGMA table_info`, installed only when `sync_state.enabled = 1`
  (a profile without sync has none). Per synced table: insert (row clock `'*'`, the generation),
  update (a clock per changed column, every member of a changed column group), primary-key change
  (tombstone of the old key + insert of the new), and a BEFORE/AFTER delete pair. A mode in
  `temp.sync_mode` steers them: `local` clocks and queues; `merge` queues nothing and every delete
  needs a cascade clock (the parent's tombstone) or RAISEs; `repair` records nothing. A local delete
  ticks once and every row its FK cascades reach is tombstoned with that same clock, as a merge
  stamps a cascade. Clocks come from the connection functions `sync_hlc_tick`, `sync_hlc_now`,
  `sync_hlc_observe`. `INSERT OR REPLACE` is banned by a lint.
- **D9 Hybrid logical clock.** `(ms << 16) | counter` as `i64`, compared with the 16-byte device ID.
  The ms part saturates at `2^47 − 1` (6429-10-17) so the packed value stays a positive SQLite
  INTEGER; a decoded clock above `i64::MAX` is refused; a remote clock over 24 h ahead is applied
  and warned about. A row that never had a clock reads as `Clock::ZERO` with the all-zero device.
- **Full rows always (R1).** The outbox holds keys; the journal sends every synced column of the
  current row with its clock (column clock, else `'*'`, else zero), `"g"` and `"ins"`; a deleted key
  sends `"del"`, and `"del"` + `"ins"` when it was re-created. `"p"` carries each referenced
  parent's generation, `"barrier"` the author's barrier.
- **D12–D13 Delete epochs and generations.** A key's epoch is its newest tombstone `X`. An insert
  whose generation is older than `X` (or than its own `"del"`) is dropped; inside one epoch two
  creations of the same row merge per column and the stored generation becomes the larger one. A
  carried tombstone newer than the local `'*'` deletes the row whatever later column edits it had.
  Column groups merge as one unit. `stock_splits.status`: a `pending` written by an insert is weak
  and loses to any other status; otherwise the newer clock wins. `sync_devices.public_key` is
  write-once.
- **D14 Foreign keys.** For a live row the column merge runs first and the parent rules run only
  on the FK values that won (plus key parts); a non-live row checks every carried value. A child
  referencing a deleted parent epoch follows the key's `ON DELETE`: a dropped CASCADE (or NO ACTION
  drop-child) insert is recorded as a tombstone at the parent's tombstone clock (raised, never
  lowered), so its own children follow in turn; SET NULL inserts with NULL. A parent never seen
  stops the merge.
- **D15 Unique keys.** *SameId*: the creating services derive the ID from the natural key
  (`natural_id`, UUIDv5), including `bank_balance_records (account, as_of)` and
  `budget_goal_versions (category, timeframe, effective month)`, whose key edit is a delete plus an
  insert under the new key's ID; a different ID on one key (legacy random IDs) falls back to
  keep-newest by the key clock `max('*', key column clocks)` with a warning. *Rename*
  (`stock_tags`, `stock_tag_groups`, spending plans per year, category names) and *Unlink*
  (`cashflow_items.replacement_source_id`) are derived: the typed value lives in the Local
  `sync_typed (tbl, pk, col)` and the stored column is recomputed from the typed values (lowest ID
  keeps the name or link). The wire carries typed values. A collision inside one changeset is
  deferred, with the rows that reference the deferred row, and retried after the rest while
  progress is made; only what still collides gets the rule.
- **Tombstone invariant (narrowed).** Live rows and their clocks, and every `'-'`, are identical
  across devices, except that a cascade may raise a key's `'-'` above its author's own earlier
  tombstone; no decision reads that window, and a dead row's `'*'` is never read.
- **Recomputed-or-typed columns** (bank balance, mirrored prices, position quantity/currency,
  other-asset average): computed from source rows while any exist, typed otherwise. The typed value
  is the last-writer-wins one, kept in `sync_typed` while a computed value hides it, sent on the
  wire, and put back when the last source row goes. With capture installed, the value at sync start
  counts as typed.
- **Split units derive from entry values.** `entry_*`, `entry_basis` (split IDs included when
  entered) and, on the three side tables, `entry_at` are synced; current units and `split_basis`
  are Recomputed by `stock_splits::reconcile_ticker` in one scaling with one rounding. Deleting a
  reverted manual split or recording it again at another ratio first re-bases the rows whose
  `entry_basis` names it (scaled back by the old ratio, the ID removed, a local entry write).
- **Repairs** run in `repair` mode after every changeset, through the same service code a local
  write runs, as pure functions of the merged rows (ordered by primary key); a failing
  recalculation falls back deterministically instead of failing the merge.
- **Snapshots** dump through the journal reader, only with an empty `sync_outbox` and
  `sync_unsent` (`validation.syncSnapshotPending`); restore replaces (join) or merges (after `410`).
- **Chains.** Each pulled envelope is judged `Apply`, `SkipReplay`, `OwnAcknowledged`, `Clone`,
  `Quarantine`, `Held`, `Gap`, `NewerSchema`, `BelowBarrier` or `Invalid`. A re-join after a clone
  takes the confirmed own prefix below the triggering seq as the old ID's head and re-queues every
  own clock at or above `W = min(prefix max_hlc, min_hlc of every own envelope outside it)`
  (`sync_own_log.min_hlc`).
- **D17 Envelope (phase 2, decided now).** XChaCha20-Poly1305 with HKDF-SHA256 keys, deflate and
  1 KiB padding, STREAM for snapshots with a per-stream key, no signatures.

## Consequences

- Every write path syncs, present and future, with no service change; a profile without sync pays
  one `SELECT` at unlock.
- A feature that rewrites rows relative to their values must state its sync behaviour
  (`docs/standards/rust-backend.md` §11); a recomputed column must be registered with its class;
  migrations after 029 that seed synced rows use deterministic IDs and fixed values.
- Known limits, accepted:
  - insurance `(provider, policy_number)` is unique only in the service: duplicates from two
    devices are kept, no repair;
  - capture costs about 350% on a 5,000-row import (≈32 ms → ≈144 ms, release, in memory; the
    spec's target was under 20%); phase 2 profiles it on a phone;
  - a legacy random-ID row deleted and its natural-ID replacement created, split across envelopes
    by the size cap, hit the warned SameId fallback; data converges, that tombstone's clock may
    differ;
  - a split ID in a basis with no `stock_splits` row stays and scales nothing;
  - concurrent rate changes leave the loan's current terms of the later clock (plain LWW);
  - an other asset whose average needs FX history the device lacks keeps its stored average;
  - legacy plain files of a property deleted on another device stay on disk until phase 4.
