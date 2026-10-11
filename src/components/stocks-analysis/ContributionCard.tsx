import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockPerformance } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { CompareGroup } from '@/utils/stock-performance';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import { formatPoints, toneOf } from '@/utils/performance-format';
import { AssetLogo } from '@/components/common/AssetLogo';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import {
  barLayout,
  contributionRows,
  partItems,
  positionItems,
  type ContributionItem,
} from './contribution-model';
import { groupName, partName, type RankedPart } from './performance-model';
import { useHeldWhile } from './use-held-while';

type Mode = 'positions' | 'group';

interface ContributionCardProps {
  /** The response on screen; the previous one while the next loads (`stale`). */
  data: StockPerformance | undefined;
  loading: boolean;
  stale: boolean;
  /** The positions the chips select. */
  selected: readonly AnalysisPosition[];
  /** The parts of the chosen group in the selection, by value. */
  parts: readonly RankedPart[];
  /** The chosen way to split (`null`: nothing to split by). */
  group: CompareGroup | null;
  filterActive: boolean;
}

/**
 * Name, bar, value. Compact below 1360 px (the card is 351 px wide at the 1080 px minimum), the
 * design's widths from there.
 */
const GRID =
  'grid grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_72px] items-center gap-x-3 px-[21px] min-[1360px]:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)_84px] min-[1360px]:gap-x-[14px]';

/** Skeleton in the shape of the card: the head and five rows. */
export function ContributionSkeleton() {
  return (
    <Card className="flex flex-col pb-4">
      <CardHeader className="items-start">
        <div>
          <Skeleton className="h-5 w-44" />
          <Skeleton className="mt-2 h-3 w-64" />
        </div>
        <Skeleton className="h-[32px] w-36" />
      </CardHeader>
      {[0, 1, 2, 3, 4].map((i) => (
        <Skeleton key={i} className="mx-[21px] mb-3 h-5" />
      ))}
    </Card>
  );
}

/**
 * "Příspěvek k výnosu": how many percentage points of the portfolio's return each position (or
 * each part of the chosen group) brought. A diverging bar per row from a shared zero line; the
 * rows add up to the total in the footer. With a selection the footer is the selection's share of
 * the portfolio's return.
 */
export function ContributionCard({
  data,
  loading,
  stale,
  selected,
  parts,
  group,
  filterActive,
}: ContributionCardProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const pp = t('stocksAnalysis.pp');
  const [chosen, setChosen] = useState<Mode>('positions');

  const live = useMemo(
    () => ({ selected, parts, group, filterActive }),
    [selected, parts, group, filterActive]
  );
  const view = useHeldWhile(live, stale);
  const mode: Mode = chosen === 'group' && view.group ? 'group' : 'positions';

  const byPosition = useMemo(
    () => new Map((data?.positions ?? []).map((p) => [p.investmentId, p])),
    [data]
  );
  const bySeries = useMemo(() => new Map((data?.series ?? []).map((s) => [s.key, s])), [data]);
  const kind = view.group?.kind;
  const noneTag = t('stocksAnalysis.board.noneTag');
  const noneSector = t('stocksAnalysis.board.noneSector');
  const other = t('stocksAnalysis.contribution.other');

  const rows = useMemo(
    () =>
      mode === 'positions'
        ? contributionRows(positionItems(view.selected, byPosition), 'magnitude', other)
        : contributionRows(
            partItems(view.parts, bySeries, (part) =>
              partName(part, kind, { tag: noneTag, sector: noneSector })
            ),
            'value',
            other
          ),
    [mode, view.selected, view.parts, byPosition, bySeries, kind, noneTag, noneSector, other]
  );
  const layout = useMemo(() => barLayout(rows.map((row) => row.points)), [rows]);

  if (!data) return loading ? <ContributionSkeleton /> : null;

  const portfolioReturn = data.portfolio.at(-1) ?? 0;
  const selectionPoints = view.selected.reduce(
    (sum, position) => sum + (byPosition.get(position.id)?.contribution ?? 0),
    0
  );
  const footPoints = view.filterActive ? selectionPoints : portfolioReturn;

  const percent = (value: number) => fmt.percent(value / 100, 1, { signed: true });
  const points = (value: number) => formatPoints(fmt.number, value, pp);
  const label = view.group
    ? groupName(view.group, (auto) => t(`stocksAnalysis.groups.${auto}`))
    : '';

  const subtitle = (): string => {
    const value = percent(portfolioReturn);
    if (view.filterActive) {
      return t(
        mode === 'group'
          ? 'stocksAnalysis.contribution.subSelectionGroup'
          : 'stocksAnalysis.contribution.subSelection',
        { part: points(selectionPoints), value, group: label }
      );
    }
    if (mode === 'positions') return t('stocksAnalysis.contribution.subPositions', { value });
    const key = { tags: 'subTags', sector: 'subSectors', currency: 'subCurrencies' }[
      kind ?? 'tags'
    ];
    return t(`stocksAnalysis.contribution.${key}`, { value, group: label });
  };

  const toneClass = (value: number) => {
    const tone = toneOf(value);
    return tone === 'gain' ? 'text-gain' : tone === 'loss' ? 'text-loss' : 'text-ink-2';
  };

  const renderRow = (row: ContributionItem, i: number) => (
    <li key={row.id} className={cn(GRID, 'min-h-[40px] border-t border-line-soft text-table')}>
      <span className="flex min-w-0 items-center gap-[10px]">
        {row.ticker !== null && (
          <AssetLogo ticker={row.ticker} type="stock" className="size-6 text-[8.5px]" />
        )}
        <span className="min-w-0 py-[7px]">
          <b
            className={cn(
              'block truncate text-table',
              row.muted ? 'font-600 text-ink-3' : 'font-650 text-ink'
            )}
          >
            {row.name}
          </b>
          {row.positions !== null && (
            <small className="mt-0.5 block truncate text-micro font-500 text-ink-4">
              {t('stocksAnalysis.board.positions', { count: row.positions })}
            </small>
          )}
        </span>
      </span>
      <span aria-hidden className="relative block h-4">
        <i
          className="absolute -bottom-3 -top-3 w-px bg-line-strong"
          style={{ left: `${layout.zero}%` }}
        />
        <i
          className={cn('absolute top-1 h-2 rounded-[2px]', row.points < 0 ? 'bg-s3' : 'bg-s2')}
          style={{ left: `${layout.bars[i].left}%`, width: `${layout.bars[i].width}%` }}
        />
      </span>
      <span
        className={cn('text-right font-650 num whitespace-nowrap', toneClass(row.points))}
        title={
          row.periodReturn === null
            ? undefined
            : t('stocksAnalysis.contribution.periodReturn', { value: percent(row.periodReturn) })
        }
      >
        {points(row.points)}
      </span>
    </li>
  );

  return (
    <Card className={cn('flex flex-col transition-opacity duration-base', stale && 'opacity-50')}>
      <CardHeader className="flex-wrap items-start">
        <div className="min-w-0 flex-1 basis-[190px]">
          <CardTitle className="text-[16px]">{t('stocksAnalysis.contribution.title')}</CardTitle>
          <CardDescription>{subtitle()}</CardDescription>
        </div>
        <Segmented
          value={mode}
          onValueChange={setChosen}
          aria-label={t('stocksAnalysis.contribution.title')}
          options={[
            { value: 'positions', label: t('stocksAnalysis.contribution.byPosition') },
            {
              value: 'group',
              label: (
                <span title={t('stocksAnalysis.contribution.byGroupTitle')}>
                  {t('stocksAnalysis.contribution.byGroup')}
                </span>
              ),
              disabled: view.group === null,
            },
          ]}
        />
      </CardHeader>
      <ul className="m-0 flex flex-1 list-none flex-col p-0">
        {rows.length === 0 ? (
          <li className="border-t border-line-soft px-[21px] py-6 text-center text-table text-ink-3">
            {t('stocksAnalysis.performance.empty')}
          </li>
        ) : (
          rows.map(renderRow)
        )}
        <li
          className={cn(
            GRID,
            'mt-auto min-h-[40px] rounded-b-r4 border-t border-line bg-well text-table'
          )}
        >
          <b className="truncate font-700 text-ink">
            {t(
              view.filterActive
                ? 'stocksAnalysis.contribution.selection'
                : 'stocksAnalysis.contribution.portfolio'
            )}
          </b>
          <span />
          <span className={cn('text-right font-650 num whitespace-nowrap', toneClass(footPoints))}>
            {points(footPoints)}
          </span>
        </li>
      </ul>
    </Card>
  );
}
