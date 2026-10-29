import type { TFunction } from 'i18next';
import type { StockTag } from '@shared/schema';

/**
 * The hover of a widget's filter badge: which tags it counts. `partial`: only part of the widget
 * follows the filter (the performance card), so it says what stays whole-portfolio.
 */
export function tagFilterTitle(
  t: TFunction<'reports'>,
  tags: readonly Pick<StockTag, 'name'>[],
  partial = false
): string {
  const joined = tags.map((tag) => tag.name).join(t('stocksAnalysis.scope.tagsJoin'));
  return partial
    ? t('stocksAnalysis.scope.hintActive', { tags: joined })
    : t('stocksAnalysis.filterBadge.title', { tags: joined });
}
