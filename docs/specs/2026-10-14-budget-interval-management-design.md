# Budget interval selection — clarified scope

Supersedes the deletion and wire-contract constraints of
`2026-10-14-budgeting-audit-design.md` following the user's clarification:
the settings table supports selecting one or multiple intervals, with one batch
delete action. Other budgeting audit requirements remain applicable.

- Settings include all stored limit intervals, including archived goals that
  were ended through the previous goal deletion flow.
- Only positive limit versions are selectable; no-limit intervals remain visible
  as gaps and can be edited to set a limit again.
- `get_budget_goal_history` returns the existing `BudgetGoal[]` contract, including
  archived version timelines. Archived IDs only identify table rows; editing and
  deleting use category, timeframe and effective month.
- `delete_budget_goal_intervals` takes typed `BudgetGoalInterval[]` selections.
  Validate every selection, reject empty batches, deduplicate identical selections
  and apply the entire batch in one database transaction. A stale selection rolls
  back the whole batch.
- Cancellation writes NULL amounts at the existing boundaries. This removes
  exactly the selected months through the next version; it never extends the
  preceding limit. Update/remove the mirrored current goal as appropriate.
- Historical selections explicitly change historical reports. Confirmation copy
  states this. Bank transactions and unselected limit intervals remain unchanged.
- Both active-goal and full-history caches are invalidated after writes. Clear
  table selection when a refreshed history response arrives.
- Backdating an edited interval preserves its selected amount and native currency.
  Later explicit versions remain intact. Cached opening dates are used immediately
  only when no refresh is running; an invalidated cache waits for its new result.

No migration is needed. Follow all seven wire-contract steps for the new interval
type and commands, including regeneration and all ten translations.
