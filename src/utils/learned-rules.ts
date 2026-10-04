import type { LearnedPayeeEntry } from '@shared/schema';

/** Rows per page offered on the learned-rules table (RUL-01). */
export const PAGE_SIZES = [50, 100, 200] as const;
export const DEFAULT_PAGE_SIZE = PAGE_SIZES[0];

/** Value of the category filter that lets every category through. */
export const ALL_CATEGORIES = 'all';

/**
 * Learned rules matching the free-text search (payee, normalized payee, IBAN or category name,
 * case-insensitive) and the category filter (`ALL_CATEGORIES` or one category id).
 */
export function filterLearnedRules(
  rules: readonly LearnedPayeeEntry[],
  filter: { query: string; categoryId: string },
  categoryNameOf: (categoryId: string) => string | undefined
): LearnedPayeeEntry[] {
  const query = filter.query.trim().toLowerCase();
  return rules.filter((rule) => {
    if (filter.categoryId !== ALL_CATEGORIES && rule.categoryId !== filter.categoryId) {
      return false;
    }
    if (!query) return true;
    return (
      rule.originalPayee?.toLowerCase().includes(query) ||
      rule.normalizedPayee?.toLowerCase().includes(query) ||
      rule.counterpartyIban?.toLowerCase().includes(query) ||
      categoryNameOf(rule.categoryId)?.toLowerCase().includes(query) ||
      false
    );
  });
}

/** How many learned rules each category has (categories without any are absent). */
export function countRulesByCategory(rules: readonly LearnedPayeeEntry[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const rule of rules) {
    counts.set(rule.categoryId, (counts.get(rule.categoryId) ?? 0) + 1);
  }
  return counts;
}

export interface Page<T> {
  items: T[];
  /** Zero-based page actually shown (the requested one, clamped into range). */
  page: number;
  pageCount: number;
  total: number;
  /** 1-based position of the first / last item on the page; 0 / 0 when there are none. */
  from: number;
  to: number;
}

/** One page of `items`; an out-of-range `page` (filters shrank the list) is clamped. */
export function paginate<T>(items: readonly T[], page: number, pageSize: number): Page<T> {
  const total = items.length;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const current = Math.min(Math.max(0, page), pageCount - 1);
  const start = current * pageSize;
  const slice = items.slice(start, start + pageSize);
  return {
    items: slice,
    page: current,
    pageCount,
    total,
    from: slice.length === 0 ? 0 : start + 1,
    to: start + slice.length,
  };
}

/** Whether none, some or all of `ids` are in `selected` (for a tri-state header checkbox). */
export function selectionState(
  ids: readonly string[],
  selected: ReadonlySet<string>
): 'none' | 'some' | 'all' {
  if (ids.length === 0) return 'none';
  const chosen = ids.filter((id) => selected.has(id)).length;
  if (chosen === 0) return 'none';
  return chosen === ids.length ? 'all' : 'some';
}
