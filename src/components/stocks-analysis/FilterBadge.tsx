import { useTranslation } from 'react-i18next';
import { ListFilter } from 'lucide-react';
import type { StockTag } from '@shared/schema';
import { Badge } from '@/components/ui/badge';

/**
 * Marks a widget the tag filter narrows, in the dark of an active chip: "Filtr: Růst · USA".
 * Nothing without a filter. `partial`: only part of the widget follows the filter (the
 * performance card), so the hover says what stays whole-portfolio.
 */
export function FilterBadge({
  tags,
  partial = false,
}: {
  /** The chosen tags of the filter, in the order of their rows of chips. */
  tags: readonly Pick<StockTag, 'id' | 'name'>[];
  partial?: boolean;
}) {
  const { t } = useTranslation('reports');
  if (tags.length === 0) return null;
  const names = tags.map((tag) => tag.name);
  const joined = names.join(t('stocksAnalysis.scope.tagsJoin'));
  return (
    <Badge
      variant="dark"
      className="max-w-[260px]"
      title={
        partial
          ? t('stocksAnalysis.scope.hintActive', { tags: joined })
          : t('stocksAnalysis.filterBadge.title', { tags: joined })
      }
    >
      <ListFilter aria-hidden />
      <span className="truncate">
        {t('stocksAnalysis.filterBadge.label', { tags: names.join(' · ') })}
      </span>
    </Badge>
  );
}
