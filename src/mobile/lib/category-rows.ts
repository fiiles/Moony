/**
 * Nastavení → Kategorie, pure parts: the categories grouped by kind with their usage, who a
 * deleted category's payments can move to, and when the delete may be confirmed. The in-use rule
 * and the reassign requirement are the desktop `CategoryManager`'s.
 */
import type { CategoryUsage, FlowKind, TransactionCategory } from '@shared/schema';
import type { TextRef } from '@/mobile/lib/settings-summary';

export interface CategoryRow {
  category: TransactionCategory;
  usage: CategoryUsage | null;
  inUse: boolean;
}

/** Výdaje, Příjmy, then the remaining kinds. */
const KIND_ORDER: readonly FlowKind[] = ['expense', 'income', 'transfer'];

/** Everything that references a category: what a delete would have to move. */
export function usageTotal(usage: Omit<CategoryUsage, 'categoryId'>): number {
  return usage.transactions + usage.rules + usage.learnedPayees + usage.budgetGoals;
}

/**
 * The usage line of a category row, as parts to join with " · ": only the counts above zero, each
 * with its plural noun ("16 transakcí", "1 rozpočet"); no parts for an unused category.
 */
export function usageParts(usage: Omit<CategoryUsage, 'categoryId'>): TextRef[] {
  const parts: TextRef[] = [];
  const add = (key: string, count: number) => {
    if (count > 0) parts.push({ key: `settings.categories.${key}`, params: { count } });
  };
  add('usageTransactions', usage.transactions);
  add('usageRules', usage.rules);
  add('usagePayees', usage.learnedPayees);
  add('usageBudgets', usage.budgetGoals);
  return parts;
}

/** One group per kind that has categories, in the kind order; in a group the categories keep their order. */
export function groupCategories(
  categories: readonly TransactionCategory[],
  usage: readonly CategoryUsage[]
): { kind: string; rows: CategoryRow[] }[] {
  const usageById = new Map(usage.map((u) => [u.categoryId, u]));
  const kinds = [...KIND_ORDER, ...new Set(categories.map((c) => c.kind))].filter(
    (kind, i, all) => all.indexOf(kind) === i
  );
  return kinds
    .map((kind) => ({
      kind: kind as string,
      rows: categories
        .filter((c) => c.kind === kind)
        .map((category) => {
          const u = usageById.get(category.id) ?? null;
          return { category, usage: u, inUse: u !== null && usageTotal(u) > 0 };
        }),
    }))
    .filter((group) => group.rows.length > 0);
}

/** The categories a deleted one can move to: its own kind first, then the others; never itself. */
export function reassignTargets(
  categories: readonly TransactionCategory[],
  deleting: TransactionCategory
): TransactionCategory[] {
  const others = categories.filter((c) => c.id !== deleting.id);
  return [
    ...others.filter((c) => c.kind === deleting.kind),
    ...others.filter((c) => c.kind !== deleting.kind),
  ];
}

/** A category in use cannot go until a target is picked. */
export function canConfirmDelete(row: CategoryRow, reassignTo: string | null): boolean {
  return !row.inUse || reassignTo !== null;
}

/** Every category can be edited; a system one cannot be deleted. */
export function categoryRowActions(row: CategoryRow): { edit: true; delete: boolean } {
  return { edit: true, delete: !row.category.isSystem };
}
