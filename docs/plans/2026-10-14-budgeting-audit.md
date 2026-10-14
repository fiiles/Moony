# Budgeting audit implementation plan

Spec: `docs/specs/2026-10-14-budgeting-audit-design.md`.
Implement inline in the current clean checkout, without changing wire types.

1. Reproduce cached latest-date initialization using a real QueryObserver.
   Add failing regression coverage for refreshing that query and invalid month
   values, then fix the initialization gate, refresh keys and month parsing.
2. Add tested version interval derivation (inclusive end of the month before
   the next version). Add a settings table using existing goals/versions, with
   native amount, recurrence, validity and direct edit/delete actions.
3. Separate period evaluation from current settings. Label retained historical
   limits; remove unusable delete actions. Keep settings accessible through
   report errors and empty periods, and render dialogs during loading/errors.
4. Pass the selected effective month into the editor, prefill its own currency,
   and move budget writes into a domain mutation hook. Preserve subsequent
   versions and stored money text. Cover backdating, empty periods, cancellation
   and recreation in service regression tests using an in-memory Connection.
5. Update all ten locales and README. Review the diff for cache invalidation,
   selection before the first version, scheduled changes, historical deletion,
   version currency changes and empty/error states. Run frontend lint/typecheck/
   tests/build and Rust format/clippy/tests, reporting any blocked verification.
