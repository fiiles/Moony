/**
 * The newest import of each account, from the import histories the phone reads per account
 * (`['import-batches', accountId]`): unix seconds, or `null` for an account that has never
 * imported or whose history has not answered yet. `batches[i]` is the history of `accountIds[i]`.
 */
export function newestImportPerAccount(
  accountIds: readonly string[],
  batches: ReadonlyArray<ReadonlyArray<{ importedAt: number }> | undefined>
): Record<string, number | null> {
  const newest: Record<string, number | null> = {};
  accountIds.forEach((id, i) => {
    const history = batches[i] ?? [];
    newest[id] = history.length ? Math.max(...history.map((b) => b.importedAt)) : null;
  });
  return newest;
}
