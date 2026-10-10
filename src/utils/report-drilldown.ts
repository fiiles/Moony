/**
 * The report drilldown keeps the rows it opened with while the user
 * recategorizes them. A changed row that no longer belongs to the dialog's
 * target (a category id, or `uncategorized`) is shown as moved, not removed.
 */
export function isMovedOut(
  targetCategoryId: string | undefined,
  currentCategoryId: string | null
): boolean {
  if (targetCategoryId === undefined) return false;
  return (currentCategoryId ?? 'uncategorized') !== targetCategoryId;
}

/** Rows whose category changed in the dialog and that now sit outside its target. */
export function movedCount(
  rows: readonly { id: string; categoryId: string | null }[],
  changes: ReadonlyMap<string, string | null>,
  targetCategoryId: string | undefined
): number {
  return rows.filter(
    (row) => changes.has(row.id) && isMovedOut(targetCategoryId, changes.get(row.id) ?? null)
  ).length;
}

/** A payment a bulk recategorization changed, as the picker reports it (`ChangedRow`). */
interface BulkChange {
  id: string;
  previousCategoryId: string | null;
}

/**
 * The picker gave `categoryId` to the other payments a remembered pick covers: mark the ones this
 * dialog holds so their chips follow, leaving rows the dialog never loaded alone.
 */
export function applyBulkChanges(
  changes: ReadonlyMap<string, string | null>,
  rows: readonly { id: string; categoryId: string | null }[],
  changed: readonly BulkChange[],
  categoryId: string
): ReadonlyMap<string, string | null> {
  const held = new Set(rows.map((row) => row.id));
  const next = new Map(changes);
  for (const change of changed) {
    if (held.has(change.id)) next.set(change.id, categoryId);
  }
  return next;
}

/**
 * The bulk change was undone: put each held row back to what it had before, which drops the mark
 * when that is the category the dialog opened with.
 */
export function restoreBulkChanges(
  changes: ReadonlyMap<string, string | null>,
  rows: readonly { id: string; categoryId: string | null }[],
  changed: readonly BulkChange[]
): ReadonlyMap<string, string | null> {
  const original = new Map(rows.map((row) => [row.id, row.categoryId ?? null]));
  const next = new Map(changes);
  for (const change of changed) {
    if (!original.has(change.id)) continue;
    const previous = change.previousCategoryId ?? null;
    if (previous === original.get(change.id)) next.delete(change.id);
    else next.set(change.id, previous);
  }
  return next;
}
