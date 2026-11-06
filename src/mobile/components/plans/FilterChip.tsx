import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';

/**
 * A removable filter of the plan detail (spec M9), above the section it narrows: "Březen ×",
 * "Ubytování ×". The desktop `FilterBadge`'s dark material as a 32 pt pill (12 / 600, the name
 * ending in an ellipsis); the whole pill is the button that clears the filter, its hit area 44 pt.
 */
export function FilterChip({ label, onClear }: { label: string; onClear: () => void }) {
  const { t } = useTranslation('common');
  return (
    <button
      type="button"
      onClick={onClear}
      aria-label={`${t('filterBadge.clear')}: ${label}`}
      className="relative inline-flex min-h-8 min-w-0 max-w-full items-center gap-1.5 rounded-full bg-dark py-1 pl-3 pr-2 text-m-sub font-semibold text-ink-inverse [-webkit-touch-callout:none] after:absolute after:inset-x-0 after:-inset-y-1.5 after:content-[''] active:opacity-80 focus-visible:outline-none focus-visible:shadow-focus"
    >
      <span className="min-w-0 truncate">{label}</span>
      <X aria-hidden className="size-3.5 flex-none text-ink-inverse-2" strokeWidth={2} />
    </button>
  );
}

/** The chips above one section, in reading order; nothing without a filter. */
export function FilterChips({
  chips,
}: {
  chips: readonly ({ key: string; label: string; onClear: () => void } | null)[];
}) {
  const shown = chips.filter((chip) => chip !== null);
  if (shown.length === 0) return null;
  return (
    <div className="mx-0.5 mb-2 flex flex-wrap gap-2">
      {shown.map((chip) => (
        <FilterChip key={chip.key} label={chip.label} onClear={chip.onClear} />
      ))}
    </div>
  );
}
