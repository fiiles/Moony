/**
 * The categorize queue's logic (spec §8.5): what a page that works through the queue shows while
 * the answer changes under it. The rows are a snapshot taken when the page opens, so a row never
 * jumps while the person works; later answers only say which of those rows are still open. Pure.
 */
import type {
  CategorizeQueue,
  CategorizeQueueRow,
  ChangedRow,
  SuggestionSource,
} from '@shared/schema';
import { routes } from '@/mobile/shell/routes';

/** How long "Nechat" holds a payment back from the queue (spec §8.5). */
export const SNOOZE_DAYS = 30;

/** What this page did to a row of the snapshot; the answer of the backend may still lag behind it. */
export type LocalOutcome =
  /** Categorized here (accepted, picked, or through the details sheet); `batch` is what that pick took along. */
  | { kind: 'categorized'; categoryId: string; batch: ChangedRow[] }
  /** Put off with "Nechat". */
  | { kind: 'left' };

export type RowState = 'open' | 'done' | 'left';

/** The rows of the snapshot that left the open set, of all of them ("3 z 12"). */
export function queueProgress(
  snapshotIds: readonly string[],
  openIds: ReadonlySet<string>
): { handled: number; total: number } {
  return {
    handled: snapshotIds.filter((id) => !openIds.has(id)).length,
    total: snapshotIds.length,
  };
}

/**
 * A row is `left` when it was put off here, `done` when it was categorized here or has left the
 * open set some other way (another row's batch took it along), else `open`. What this page did
 * counts before what the answer says: the answer is the one before the write's refresh until it
 * settles.
 */
export function rowState(
  id: string,
  openIds: ReadonlySet<string>,
  local: ReadonlyMap<string, LocalOutcome>
): RowState {
  const outcome = local.get(id);
  if (outcome) return outcome.kind === 'left' ? 'left' : 'done';
  return openIds.has(id) ? 'open' : 'done';
}

const SOURCE_KEYS: Record<SuggestionSource, string> = {
  mcp: 'queue.source.mcp',
  learned: 'queue.source.learned',
  rule: 'queue.source.rule',
  pack: 'queue.source.pack',
};

/** The `mobile` key of the small label on a suggestion pill ("podle pravidla"). */
export function suggestionLabelKey(source: SuggestionSource): string {
  return SOURCE_KEYS[source];
}

/**
 * `local` with `ids` let go of by every batch but the one of `owner`: a newer write reached those
 * rows, so an earlier batch's undo could no longer restore them truthfully.
 */
function releaseRows(
  local: ReadonlyMap<string, LocalOutcome>,
  ids: readonly string[],
  owner: string
): Map<string, LocalOutcome> {
  const next = new Map(local);
  const gone = new Set(ids);
  for (const [id, outcome] of local) {
    if (id === owner || outcome.kind !== 'categorized') continue;
    if (!outcome.batch.some((row) => gone.has(row.id))) continue;
    next.set(id, { ...outcome, batch: outcome.batch.filter((row) => !gone.has(row.id)) });
  }
  return next;
}

/**
 * The write of row `id`'s own category has landed (an accepted suggestion): every earlier batch
 * lets go of the row, so its undo can no longer restore it over that pick.
 */
export function releaseRow(
  local: ReadonlyMap<string, LocalOutcome>,
  id: string
): Map<string, LocalOutcome> {
  return releaseRows(local, [id], id);
}

/**
 * Row `id` was picked as `categoryId` (the picker, once the category is saved): the row's outcome
 * is that pick with nothing taken along yet, and every earlier batch lets go of the row.
 */
export function withPick(
  local: ReadonlyMap<string, LocalOutcome>,
  id: string,
  categoryId: string
): Map<string, LocalOutcome> {
  const next = releaseRow(local, id);
  next.set(id, { kind: 'categorized', categoryId, batch: [] });
  return next;
}

/**
 * The learning of row `id`'s pick took `changed` along: that pick's batch owns them now — every
 * earlier batch lets go of them, and one of them that was picked here earlier shows this batch's
 * category until the batch is restored (`doneCategories`). Nothing changes for an empty batch or
 * a pick that is no longer the row's outcome.
 */
export function withBatch(
  local: ReadonlyMap<string, LocalOutcome>,
  id: string,
  changed: readonly ChangedRow[]
): ReadonlyMap<string, LocalOutcome> {
  const outcome = local.get(id);
  if (changed.length === 0 || outcome?.kind !== 'categorized') return local;
  const next = releaseRows(
    local,
    changed.map((row) => row.id),
    id
  );
  next.set(id, { ...outcome, batch: [...changed] });
  return next;
}

/**
 * The category each done row ended with, as far as this page knows: the pick of a row, and the
 * same category for the rows its remembered pick took along. A batch still holds a row picked
 * here only when it came after that pick (`withPick` and `withBatch` let go of the rows a newer
 * write reached), so the batch's category wins. A row that left the open set some other way is
 * not in it, and neither are the rows of a batch that was `restored` (the switch of its
 * expansion turned off): they hold what they held before — their own pick, else nothing, and the
 * queue lists them again.
 */
export function doneCategories(
  local: ReadonlyMap<string, LocalOutcome>,
  restored: ReadonlySet<string> = new Set()
): Map<string, string> {
  const categories = new Map<string, string>();
  for (const [id, outcome] of local) {
    if (outcome.kind === 'categorized') categories.set(id, outcome.categoryId);
  }
  for (const [id, outcome] of local) {
    if (outcome.kind !== 'categorized' || restored.has(id)) continue;
    for (const taken of outcome.batch) categories.set(taken.id, outcome.categoryId);
  }
  return categories;
}

/**
 * Whether a pick that took other payments along can still be given back: some done row has its
 * expansion and its switch (`restored`) is not turned off yet.
 */
export function batchOffered(
  local: ReadonlyMap<string, LocalOutcome>,
  restored: ReadonlySet<string>
): boolean {
  for (const [id, outcome] of local) {
    if (outcome.kind === 'categorized' && outcome.batch.length > 0 && !restored.has(id)) {
      return true;
    }
  }
  return false;
}

/**
 * The ids the answer lists as open, less what this page has handled itself: the answer is stale
 * between a write and its refresh, and the progress should not wait for it.
 */
export function effectiveOpenIds(
  live: CategorizeQueue | undefined,
  local: ReadonlyMap<string, LocalOutcome>
): Set<string> {
  const open = new Set<string>();
  for (const { transaction } of live?.rows ?? []) {
    if (!local.has(transaction.id)) open.add(transaction.id);
  }
  return open;
}

/**
 * The rows the page lists. The first answer that is not a stale one being refreshed (`settled`)
 * is the snapshot and stays so; only once none of its rows is open while the answer still lists
 * payments of its own — the ones beyond the 500 the backend sends at a time, or newly arrived —
 * the page takes those as its next snapshot. An emptied queue keeps its rows: that is the
 * "all done" state.
 */
export function nextSnapshot(
  snapshot: readonly CategorizeQueueRow[] | null,
  live: CategorizeQueue | undefined,
  settled: boolean,
  local: ReadonlyMap<string, LocalOutcome>
): readonly CategorizeQueueRow[] | null {
  if (!live || !settled) return snapshot;
  if (snapshot === null) return live.rows;

  const open = effectiveOpenIds(live, local);
  if (snapshot.some((row) => open.has(row.transaction.id))) return snapshot;
  const held = new Set(snapshot.map((row) => row.transaction.id));
  const waiting = live.rows.some(
    ({ transaction }) => !held.has(transaction.id) && !local.has(transaction.id)
  );
  return waiting ? live.rows : snapshot;
}

/**
 * "N plateb bez kategorie": the payments waiting in the answer, less what this page has handled
 * itself and the answer still lists (it is the one before the write's refresh until it settles).
 * The answer's rows are the newest 500 of the waiting payments and move on as payments are
 * handled, so they are not a subset of the page's snapshot: the count never goes by them, only
 * by `total`, which counts all.
 */
export function openCount(
  live: CategorizeQueue | undefined,
  local: ReadonlyMap<string, LocalOutcome>
): number {
  if (!live) return 0;
  const handled = live.rows.filter(({ transaction }) => local.has(transaction.id)).length;
  return Math.max(0, live.total - handled);
}

/** What Údržba and the Finance row show of the queue: the count, and the accounts that hold the listed payments. */
export function queueSummary(queue: CategorizeQueue | undefined): {
  count: number;
  accountIds: string[];
} {
  if (!queue) return { count: 0, accountIds: [] };
  return {
    count: queue.total,
    accountIds: [...new Set(queue.rows.map((row) => row.transaction.bankAccountId))],
  };
}

/** The queue's path: one account's when exactly one holds the payments, else every account's. */
export function queuePath(accountIds: readonly string[]): string {
  return accountIds.length === 1
    ? `${routes.uncategorized}?account=${encodeURIComponent(accountIds[0])}`
    : routes.uncategorized;
}
