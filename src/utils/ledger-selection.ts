/**
 * Row selection of the account ledger. A selection belongs to one scope (account, page, period,
 * filters, sort): once the scope changes it reads as empty, so a hidden row is never tagged by
 * surprise.
 */
export interface ScopedSelection {
  scope: string;
  ids: ReadonlySet<string>;
}

const NONE: ReadonlySet<string> = new Set();

/** The selected ids while `scope` is the selection's scope; nothing otherwise. */
export function idsInScope(selection: ScopedSelection, scope: string): ReadonlySet<string> {
  return selection.scope === scope ? selection.ids : NONE;
}

/** A copy of `ids` with `id` added (`on`) or removed. */
export function withToggled(ids: ReadonlySet<string>, id: string, on: boolean): Set<string> {
  const next = new Set(ids);
  if (on) next.add(id);
  else next.delete(id);
  return next;
}

/** The header checkbox of a page: checked, partly checked or unchecked. */
export function pageCheckState(
  pageIds: readonly string[],
  ids: ReadonlySet<string>
): boolean | 'indeterminate' {
  const count = pageIds.filter((id) => ids.has(id)).length;
  if (count === 0) return false;
  return count === pageIds.length ? true : 'indeterminate';
}

/** A copy of `ids` with every row of the page selected (`on`) or cleared. */
export function withPageSelected(
  ids: ReadonlySet<string>,
  pageIds: readonly string[],
  on: boolean
): Set<string> {
  const next = new Set(ids);
  for (const id of pageIds) {
    if (on) next.add(id);
    else next.delete(id);
  }
  return next;
}

/** The selected rows of the page, in the page's order. */
export function selectedRows<T extends { id: string }>(
  rows: readonly T[],
  ids: ReadonlySet<string>
): T[] {
  return rows.filter((row) => ids.has(row.id));
}
