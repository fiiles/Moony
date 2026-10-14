# Budget interval management implementation plan

Spec: `docs/specs/2026-10-14-budget-interval-management-design.md`.
Continues the budgeting audit inline after the user's interval-selection clarification.

1. Add service regressions for noncontiguous cancellation, transaction rollback,
   archived history discoverability, and deleting the final remaining interval.
   Run them before implementation, then add the minimal typed selection model,
   history query and transactional cancellation service.
2. Add/register thin commands, register the model in bindings, regenerate types,
   mirror the selection contract in schema.ts and add typed API wrappers.
3. Add row and select-all checkboxes to the settings table. Derive the batch from
   visible positive versions; confirm the count and historical impact, then submit
   one batch. Invalidate current goals and all history/report caches afterward.
4. Cover selected edit source with pure function tests. Preserve its native text
   and currency while moving the date; wait for invalidated latest-date refetches.
5. Translate the selection/confirmation/validation copy into all ten locales,
   update README, review changes and run frontend/Rust gates plus binding drift.
   Smoke-test cached remounts, empty periods, editing and batch selection in a
   browser using synthetic data, without writing to the user's financial database.
