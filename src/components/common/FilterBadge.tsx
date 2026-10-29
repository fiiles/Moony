import { useTranslation } from 'react-i18next';
import { ListFilter, X } from 'lucide-react';
import { Badge } from '@/components/ui/badge';

/**
 * Marks a widget a filter narrows, in the dark of an active chip: "Filtr: Růst · USA". Nothing
 * without a filter. `title` says on hover what the filter does to the widget; with `onClear` an ×
 * at the end clears the filters the badge names.
 */
export function FilterBadge({
  labels,
  onClear,
  title,
}: {
  /** What the filter keeps, in the order the user reads the filters. */
  labels: readonly string[];
  onClear?: () => void;
  title?: string;
}) {
  const { t } = useTranslation('common');
  if (labels.length === 0) return null;
  return (
    <Badge variant="dark" className="max-w-[260px] shrink-0" title={title}>
      <ListFilter aria-hidden />
      <span className="truncate">{t('filterBadge.label', { labels: labels.join(' · ') })}</span>
      {onClear && (
        <button
          type="button"
          aria-label={t('filterBadge.clear')}
          onClick={onClear}
          className="-mr-[3px] grid size-4 shrink-0 place-items-center rounded-[3px] text-ink-inverse-2 transition-colors duration-fast hover:text-ink-inverse focus-visible:outline-none focus-visible:shadow-focus"
        >
          <X aria-hidden />
        </button>
      )}
    </Badge>
  );
}
