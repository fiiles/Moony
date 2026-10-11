# 0013. Tag groups partition the portfolio

Date: 2026-10-11

## Status

Accepted

## Context

A stock position could carry several tags of one tag group, for example "Dlouhodobě" and
"Spekulativně" from the group "Strategie". The stocks analysis then showed one tag with three
different values: the allocation ring split the position between its tags, the tag table counted
it in full under each of them, and the time-weighted return (TWR) line of a tag counted it in full
as well. Nothing on the page could be made to add up, and the page could not honestly answer
"which part of my return came from which group".

The redesigned analysis splits the whole portfolio by one group into lines, table rows and
contributions to the return, and these must add up: the shares to 100 %, the contributions to the
return of the portfolio. That is only possible when every position belongs to exactly one part of
the group. The alternatives were to warn about conflicting tags on the page (the user would have
to fix them by hand before the numbers made sense) and to split a position's weight between its
tags (the parts then no longer describe real positions, and the TWR of a part has no meaning).

## Decision

- **A tag group is one partition.** A position has at most one tag per group. Tags without a
  group stay free, overlapping labels: they filter the analysis and appear in the tag table, but
  the portfolio is never split by them.
- **The automatic groups Sektor and Měna are partitions by construction.** A position has one
  sector (or none) and one quote currency.
- **Enforcement on every write.** The tag service (`services/stock_tags.rs`) owns the rule:
  `set_investment_tags` rejects two tags of one group with `validation.oneTagPerGroup`, and
  moving a tag into a group (`update_tag` with a changed group) is rejected with
  `validation.tagGroupConflict` when a position carrying it would then have two tags of that
  group. The assign dialog offers one choice per group and the choice replaces the position's
  current tag of that group, so the rule is visible before the service has to refuse.
- **Legacy duplicates are removed by migration `015_one_tag_per_group`.** For each position and
  group the earliest assignment stays (by its `created_at`, then by tag name); the other
  assignments of that group are deleted. Tags themselves, ungrouped tags and assignments in other
  groups are untouched. The migration changes no schema.

## Consequences

- Shares of a group add to 100 % and the contributions of its parts sum to the TWR of the
  portfolio, so the board, the contribution chart and the lines can be read against each other.
- A user who wants overlapping labels uses tags without a group, or puts the labels into two
  groups, one per dimension.
- The migration deletes assignments without asking. A profile that tagged one position twice in a
  group keeps the tag assigned first and loses the other; the pre-migration database copy made by
  the opener is the rollback, and the user can assign the lost tag again where it still fits
  (another group, or no group).
- Every write path to `stock_investment_tags` must go through the tag service. The database
  itself does not enforce the rule (the join table has no group column and the migration changes
  no schema), so a direct SQL write could bring the duplicates back.
