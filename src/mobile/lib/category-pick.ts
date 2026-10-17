/**
 * The category picker's logic (spec §8.4): which categories it lists first, how its search folds
 * accents, who a remembered pick is learned for, and what a pick does besides setting the row's
 * category (the desktop's `CategorySelector.handleSelect`). Pure.
 */
import type { RecategorizationPreview } from '@shared/schema';

/**
 * The categories of the newest rows, each once, newest first, at most `max` (the picker's
 * "Naposledy použité"). Rows without a category are skipped.
 */
export function recentCategoryIds(
  rows: readonly { categoryId: string | null; bookingDate: number }[],
  max: number
): string[] {
  const ids = new Set<string>();
  for (const row of [...rows].sort((a, b) => b.bookingDate - a.bookingDate)) {
    if (ids.size === max) break;
    if (row.categoryId) ids.add(row.categoryId);
  }
  return [...ids];
}

/** A search text or a name without case and accents ("Říjen" and "RIJEN" meet). */
const fold = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();

/**
 * The top-level categories in their sort order whose displayed name holds the query, ignoring
 * case and accents (the desktop's list shows no sub-categories either); every one for an empty query.
 */
export function filterCategories<
  T extends { id: string; name: string; parentId: string | null; sortOrder: number },
>(categories: readonly T[], query: string, nameOf: (category: T) => string): T[] {
  const needle = fold(query.trim());
  return categories
    .filter((category) => !category.parentId && fold(nameOf(category)).includes(needle))
    .sort((a, b) => a.sortOrder - b.sortOrder);
}

/**
 * Who a remembered pick is learned for: the payee (trimmed, longer than two characters) and/or
 * the counterparty account (trimmed, longer than five) — shorter texts are noise. `null` when
 * neither is reliable, so there is nothing to learn.
 */
export function learnIdentity(tx: {
  counterpartyName: string | null;
  counterpartyIban: string | null;
}): { payee: string | null; iban: string | null } | null {
  const name = tx.counterpartyName?.trim() ?? '';
  const account = tx.counterpartyIban?.trim() ?? '';
  const payee = name.length > 2 ? name : null;
  const iban = account.length > 5 ? account : null;
  return payee || iban ? { payee, iban } : null;
}

/** What happens after the row's category is saved: nothing, a custom rule is re-pointed, or the pick is learned. */
export type PickOutcome = 'skip' | 'retarget' | 'learn';

/**
 * Own-account transfers categorize themselves and payments without an identity cannot be
 * remembered (a custom rule can still be retargeted either way); a pick made with "remember" off
 * is one-time. Without a preview (it could not be loaded in time) the pick is learned.
 */
export function pickOutcome(
  preview: RecategorizationPreview | undefined,
  remember: boolean,
  hasIdentity: boolean
): PickOutcome {
  if (!remember) return 'skip';
  if (preview) {
    if (preview.governing === 'ownAccount') return 'skip';
    if (preview.governing === 'custom' && preview.rule) return 'retarget';
    if (!preview.learnable && preview.governing !== 'custom') return 'skip';
  }
  return hasIdentity ? 'learn' : 'skip';
}
