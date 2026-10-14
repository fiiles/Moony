# Budgeting audit and settings overview

The budgets page must recover after navigation, evaluate the selected period from
fresh saved limits, and make the effective dates of configured limits editable.
This is a bounded extension of the existing budgeting flow; the wire contract,
database schema and historical-limit retention policy stay unchanged.

- Initialize the period from either a cached or newly fetched latest transaction.
  A failed lookup falls back to the current month rather than blocking reports.
- Evaluate only categories with a limit in the selected report period. Current
  and scheduled goals belong in a separate settings view, including their full
  version timeline, native currency, recurrence and inclusive validity interval.
- Open editing on the selected interval/month and use that version's currency.
  Saving an earlier effective month extends the limit backwards; later explicit
  versions remain intact. Validate month inputs before submission.
- Deleting a goal ends it from the current month and cancels future versions.
  Older report limits remain visible with an explicit historical label and an
  action to set a limit again, rather than a disabled delete control.
- Refresh every report period, goal, drilldown and the latest-transaction lookup
  after money-flow changes. Keep mutations in domain hooks.
- The settings view remains accessible even if the spending report fails or the
  selected period has no transactions. Use existing design-system primitives and
  translate every added string into all ten locales.

Verification covers cached query initialization, inactive/active cache refresh,
month parsing and version intervals, backend backdating and deletion semantics,
and the repository's frontend and Rust quality gates. Component/hook tests are
excluded by repository policy; verify their query boundary directly and inspect
the rendered page where the available UI environment permits.
