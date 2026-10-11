import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, X } from 'lucide-react';
import type { StockTag } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { FILTER_ALL } from '@/utils/stock-analysis';
import { slot } from '@/utils/rich-text';
import { Card } from '@/components/ui/card';
import { Chip } from '@/components/ui/chip';
import { Skeleton } from '@/components/ui/skeleton';
import { Rich } from './Rich';

/** One row of chips: the tags of a tag group, or the tags without a group. */
export interface ScopeGroup {
  key: string;
  label: string;
  tags: StockTag[];
  /** The tags without a group: they can overlap. */
  free?: boolean;
}

/** Rows shown before the rest hide behind "Další filtry". */
const VISIBLE_ROWS = 3;

const linkClass =
  'text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus';

interface ScopeCardProps {
  groups: ScopeGroup[];
  /** The chosen tag of each filtering group. */
  chosenIds: readonly string[];
  /** Choose a tag of a group, or `FILTER_ALL` to stop filtering by it. */
  onChoose: (groupKey: string, tagId: string) => void;
  onClear: () => void;
  /** Positions in the selection and in the portfolio. */
  shown: number;
  total: number;
  /** Their values as of today in `currency`; `null` = unknown. */
  shownValue: number | null;
  totalValue: number | null;
  currency: string;
  /** Positions without any tag. */
  untagged: number;
  onAssignUntagged: () => void;
}

/**
 * "Které pozice analyzovat": the chip filters per tag group with a live summary and the rule in a
 * sentence. One tag or "Vše" per group, AND across groups. From the fourth group on the rows hide
 * behind "Další filtry"; an active filter of a hidden row stays visible as a removable chip.
 */
export function ScopeCard({
  groups,
  chosenIds,
  onChoose,
  onClear,
  shown,
  total,
  shownValue,
  totalValue,
  currency,
  untagged,
  onAssignUntagged,
}: ScopeCardProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const chipsRef = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);

  const filterActive = chosenIds.length > 0;
  const chosenOf = (group: ScopeGroup) => group.tags.find((tag) => chosenIds.includes(tag.id));
  const visible = expanded ? groups : groups.slice(0, VISIBLE_ROWS);
  const hidden = groups.slice(VISIBLE_ROWS);
  // Hidden rows with an active filter: shown as chips next to the toggle
  const hiddenActive = expanded
    ? []
    : hidden.flatMap((group) => {
        const tag = chosenOf(group);
        return tag ? [{ group, tag }] : [];
      });

  const money = (value: number | null) =>
    value === null ? '—' : fmt.money(value, currency, { decimals: 0 });

  const chosenTags = groups.flatMap((group) => {
    const tag = chosenOf(group);
    return tag ? [tag] : [];
  });
  const hintActive = t('stocksAnalysis.scope.hintActive', { tags: slot('tags') });
  const tagNames = chosenTags.map((tag, i) => (
    <span key={tag.id}>
      {i > 0 && t('stocksAnalysis.scope.tagsJoin')}
      <b className="font-650 text-ink-2">{tag.name}</b>
    </span>
  ));

  return (
    <Card variant="flat" className="mb-[18px] px-[18px] pb-3 pt-[15px]">
      <div className="mb-[11px] flex items-baseline justify-between gap-4">
        <h2 className="m-0 text-h3 text-ink">{t('stocksAnalysis.scope.title')}</h2>
        <span className="whitespace-nowrap text-caption font-500 text-ink-3">
          <b className="font-650 text-ink">
            {filterActive
              ? t('stocksAnalysis.scope.selected', {
                  shown: fmt.number(shown),
                  total: fmt.number(total),
                })
              : t('stocksAnalysis.scope.all', { count: total })}
          </b>
          {' · '}
          <span className="num">{money(filterActive ? shownValue : totalValue)}</span>
          {filterActive && (
            <button
              type="button"
              className={cn(linkClass, 'ml-[10px]')}
              onClick={() => {
                onClear();
                // The link goes away with the filter: keep keyboard focus among the chips
                chipsRef.current?.querySelector('button')?.focus();
              }}
            >
              {t('stocksAnalysis.scope.clear')}
            </button>
          )}
        </span>
      </div>

      <div ref={chipsRef} className="flex flex-wrap items-center gap-x-[22px] gap-y-[10px]">
        {visible.map((group) => {
          const chosen = chosenOf(group)?.id ?? FILTER_ALL;
          return (
            <div key={group.key} className="flex flex-wrap items-center gap-1.5">
              <span
                className="mr-1 text-eyebrow uppercase text-ink-4"
                title={group.free ? t('stocksAnalysis.scope.ungroupedTitle') : undefined}
              >
                {group.label}
              </span>
              <Chip active={chosen === FILTER_ALL} onClick={() => onChoose(group.key, FILTER_ALL)}>
                {t('stocksAnalysis.scope.all')}
              </Chip>
              {group.tags.map((tag) => (
                <Chip
                  key={tag.id}
                  active={chosen === tag.id}
                  onClick={() => onChoose(group.key, tag.id)}
                >
                  {tag.name}
                </Chip>
              ))}
            </div>
          );
        })}
        {hidden.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              className={cn(linkClass, 'inline-flex items-center gap-1')}
              aria-expanded={expanded}
              onClick={() => setExpanded((open) => !open)}
            >
              {expanded ? t('stocksAnalysis.scope.fewer') : t('stocksAnalysis.scope.more')}
              <ChevronDown
                aria-hidden
                className={cn(
                  'size-3 transition-transform duration-fast',
                  expanded && 'rotate-180'
                )}
              />
            </button>
            {hiddenActive.map(({ group, tag }) => (
              <Chip
                key={group.key}
                active
                role="button"
                aria-label={t('stocksAnalysis.scope.removeFilter', {
                  name: `${group.label}: ${tag.name}`,
                })}
                onClick={() => onChoose(group.key, FILTER_ALL)}
              >
                {group.label}: {tag.name}
                <X aria-hidden />
              </Chip>
            ))}
          </div>
        )}
      </div>

      <div className="mt-3 flex items-center justify-between gap-4 border-t border-line-soft pt-[10px]">
        <span className="text-micro font-500 leading-[1.55] text-ink-4">
          {filterActive ? (
            <Rich text={hintActive} slots={{ tags: tagNames }} />
          ) : (
            t('stocksAnalysis.scope.hint')
          )}
        </span>
        {untagged > 0 && (
          <span className="whitespace-nowrap text-caption font-500 text-ink-3">
            {t('stocksAnalysis.scope.untagged', { count: untagged })}
            <button type="button" className={cn(linkClass, 'ml-1.5')} onClick={onAssignUntagged}>
              {t('stocksAnalysis.scope.assignUntagged')}
            </button>
          </span>
        )}
      </div>
    </Card>
  );
}

/** Skeleton in the shape of the scope card. */
export function ScopeSkeleton() {
  return (
    <Card variant="flat" aria-hidden className="mb-[18px] px-[18px] pb-3 pt-[15px]">
      <div className="mb-[11px] flex items-baseline justify-between gap-4">
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-3 w-40" />
      </div>
      <div className="flex gap-6">
        <Skeleton className="h-[26px] w-72" />
        <Skeleton className="h-[26px] w-72" />
      </div>
      <div className="mt-3 border-t border-line-soft pt-[10px]">
        <Skeleton className="h-3 w-2/3" />
      </div>
    </Card>
  );
}
