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
