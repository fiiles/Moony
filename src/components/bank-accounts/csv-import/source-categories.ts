import type { SourceCategoryMapping } from '@shared/schema';

/** What `matchSourceCategories` needs of a Moony category (a `TransactionCategory` fits). */
export interface MatchableCategory {
  id: string;
  name: string;
  parentId: string | null;
  isSystem: boolean;
}

/** A name as the matching compares it: lowercase, without diacritics, single spaces. */
export function foldName(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/\s+/g, ' ').trim();
}

/**
 * Prefill for the category panel (spec §8.2): source path → category id, or null.
 *
 * A category answers to its stored name and to its name in the UI language
 * (`displayName`, so the seeded "Groceries" is also "Potraviny"). A path
 * `Parent:Child` picks the one category named like the child whose parent is named
 * like the parent; otherwise the last segment picks the one category of that name.
 * Two candidates or none leave the source to the rules (null).
 */
export function matchSourceCategories(
  sources: readonly string[],
  categories: readonly MatchableCategory[],
  displayName: (category: MatchableCategory) => string
): Record<string, string | null> {
  const names = new Map(
    categories.map((category) => [
      category.id,
      new Set([foldName(category.name), foldName(displayName(category))]),
    ])
  );
  const named = (category: MatchableCategory, name: string) =>
    names.get(category.id)?.has(name) ?? false;
  const only = (candidates: readonly MatchableCategory[]) =>
    candidates.length === 1 ? candidates[0].id : null;
  const byId = new Map(categories.map((category) => [category.id, category]));

  const match = (source: string): string | null => {
    const segments = source.split(':').map(foldName).filter(Boolean);
    const leaf = segments.at(-1);
    if (leaf === undefined) return null;
    const leafMatches = categories.filter((category) => named(category, leaf));
    if (segments.length > 1) {
      const parentName = segments[segments.length - 2];
      const underParent = leafMatches.filter((category) => {
        const parent = category.parentId != null ? byId.get(category.parentId) : undefined;
        return parent != null && named(parent, parentName);
      });
      const found = only(underParent);
      if (found) return found;
    }
    return only(leafMatches);
  };

  return Object.fromEntries(sources.map((source) => [source, match(source)]));
}

/**
 * The panel's choices as `options.sourceCategoryMap`: the given sources (those of the
 * statement being imported) that have a category, in their order; a source left to the rules
 * (null) or not chosen at all is not sent.
 */
export function sourceCategoryMapping(
  sources: readonly string[],
  chosen: Readonly<Record<string, string | null>>
): SourceCategoryMapping[] {
  return sources.flatMap((source) => {
    const categoryId = chosen[source];
    return categoryId ? [{ source, categoryId }] : [];
  });
}
