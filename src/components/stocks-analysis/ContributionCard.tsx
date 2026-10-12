import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockPerformance } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { CompareGroup } from '@/utils/stock-performance';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import { formatPoints } from '@/utils/performance-format';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import { barLayout, contributionRows, partItems, positionItems } from './contribution-model';
import { ContributionOtherDialog } from './ContributionOtherDialog';
import { CARD_GRID, ContributionRow, ContributionTotalRow } from './ContributionRow';
import { groupName, partName, sectorLabel, type RankedPart } from './performance-model';
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
  /** The chosen way to split (`null`: nothing to split by; "don't split" counts as none). */
  group: CompareGroup | null;
  filterActive: boolean;
}

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
  const [otherOpen, setOtherOpen] = useState(false);

  const live = useMemo(
    () => ({ selected, parts, group, filterActive }),
    [selected, parts, group, filterActive]
  );
  const view = useHeldWhile(live, stale);
  // "Don't split" is no group: there are only the positions, and no switch
  const grouped = view.group !== null && view.group.kind !== 'none';
  const mode: Mode = chosen === 'group' && grouped ? 'group' : 'positions';

  const byPosition = useMemo(
    () => new Map((data?.positions ?? []).map((p) => [p.investmentId, p])),
    [data]
  );
  const bySeries = useMemo(() => new Map((data?.series ?? []).map((s) => [s.key, s])), [data]);
  const kind = view.group?.kind;
  const noneTag = t('stocksAnalysis.board.noneTag');
  const noneSector = t('stocksAnalysis.board.noneSector');
  const selectionName = t('stocksAnalysis.board.selection');
  const other = t('stocksAnalysis.contribution.other');

  const rows = useMemo(
    () =>
      mode === 'positions'
        ? contributionRows(positionItems(view.selected, byPosition), 'magnitude', other)
        : contributionRows(
            partItems(view.parts, bySeries, (part) =>
              partName(
                part,
                kind,
                { tag: noneTag, sector: noneSector, selection: selectionName },
                (name) => sectorLabel(name, t)
              )
            ),
            'value',
            other
          ),
    [
      mode,
      view.selected,
      view.parts,
      byPosition,
      bySeries,
      kind,
      noneTag,
      noneSector,
      selectionName,
      other,
      t,
    ]
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
  const label =
    grouped && view.group
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
    // The group mode is only there for a real group, never for "don't split"
    const key = { tags: 'subTags', sector: 'subSectors', currency: 'subCurrencies' }[
      kind === 'sector' || kind === 'currency' ? kind : 'tags'
    ];
    return t(`stocksAnalysis.contribution.${key}`, { value, group: label });
  };

  // The "rest" row folds the smallest effects; it opens a list of them
  const last = rows.at(-1);
  const otherRow = last?.folded ? last : undefined;
  const otherSubtitle =
    mode === 'positions'
      ? t('stocksAnalysis.contribution.otherSubPositions', { value: percent(portfolioReturn) })
      : t('stocksAnalysis.contribution.otherSubParts', {
          value: percent(portfolioReturn),
          group: label,
        });

  return (
    <>
      <Card className={cn('flex flex-col transition-opacity duration-base', stale && 'opacity-50')}>
        <CardHeader className="flex-wrap items-start">
          <div className="min-w-0 flex-1 basis-[190px]">
            <CardTitle className="text-[16px]">{t('stocksAnalysis.contribution.title')}</CardTitle>
            <CardDescription>{subtitle()}</CardDescription>
          </div>
          {grouped && (
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
                },
              ]}
            />
          )}
        </CardHeader>
        <ul className="m-0 flex flex-1 list-none flex-col p-0">
          {rows.length === 0 ? (
            <li className="border-t border-line-soft px-[21px] py-6 text-center text-table text-ink-3">
              {t('stocksAnalysis.performance.empty')}
            </li>
          ) : (
            rows.map((row, i) => (
              <ContributionRow
                key={row.id}
                row={row}
                zero={layout.zero}
                bar={layout.bars[i]}
                grid={CARD_GRID}
                onOpen={row.folded ? () => setOtherOpen(true) : undefined}
                openLabel={t('stocksAnalysis.contribution.otherOpen')}
              />
            ))
          )}
          <ContributionTotalRow
            label={t(
              view.filterActive
                ? 'stocksAnalysis.contribution.selection'
                : 'stocksAnalysis.contribution.portfolio'
            )}
            points={footPoints}
            grid={CARD_GRID}
            className="mt-auto rounded-b-r4"
          />
        </ul>
      </Card>
      {otherRow?.folded && (
        <ContributionOtherDialog
          open={otherOpen}
          onOpenChange={setOtherOpen}
          items={otherRow.folded}
          total={otherRow.points}
          description={otherSubtitle}
        />
      )}
    </>
  );
}
