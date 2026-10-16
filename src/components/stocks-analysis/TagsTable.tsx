import { Fragment, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag } from 'lucide-react';
import type { PerformanceSeries, StockTag } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { ChipLabel } from '@/components/ui/chip';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { FilterBadge } from './FilterBadge';
import { GainChange } from './GainChange';
import { amountOf } from './performance-model';
import { tagGroupTotals, tagRows, type TagFigures, type TagGroupRows } from './tables-model';

const HEAD_WRAP = '[&_th]:whitespace-normal [&_th]:leading-[1.2]';

interface TagsTableProps {
  /** The tag groups in order, then the tags without a group (`free`). */
  groups: readonly TagGroupRows[];
  /** The positions the chips select, and how many positions the portfolio has. */
  selected: readonly AnalysisPosition[];
  total: number;
  /** Value of a position as of the end of the period; of the whole portfolio. */
  endValueOf: (positionId: string) => number | null;
  endTotal: number | null;
  /**
   * The series of the performance response on screen: `tag:<id>` carries a tag's return,
   * `tag-group:<id>` the return of a tag group's subtotal.
   */
  series: readonly PerformanceSeries[] | undefined;
  filterActive: boolean;
  /** The tags the filter chose, for the badge in the head. */
  filterTags: readonly StockTag[];
  /** The period ends today: "Hodnota", else "K {date}" and the unrealized gain says "dnes". */
  endsToday: boolean;
  endDate: number;
  /** Report currency of every amount. */
  currency: string;
  /** The figures are the previous period's: the next response is loading. */
  stale: boolean;
  onManage: () => void;
}

/**
 * The tags table: for every tag the positions that carry it, their value at the end of the period
 * and its share of the whole portfolio, their return over the period, the unrealized gain and the
 * estimated dividend a year (both as of today). Shares within a tag group add up to 100 %; tags
 * without a group can overlap. A group with at least two tags holding a position ends with its
 * subtotal row, so its rows visibly add up. With a filter only tags that have a selected position
 * are listed.
 */
export function TagsTable({
  groups,
  selected,
  total,
  endValueOf,
  endTotal,
  series,
  filterActive,
  filterTags,
  endsToday,
  endDate,
  currency,
  stale,
  onManage,
}: TagsTableProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();

  const { rows, totals } = useMemo(() => {
    const byKey = new Map((series ?? []).map((s) => [s.key, s]));
    const input = {
      groups,
      selected,
      endValueOf,
      endTotal,
      periodOf: (key: string) => {
        const found = byKey.get(key);
        return found
          ? { periodReturn: found.twr.at(-1) ?? null, periodGain: amountOf(found.gain) }
          : null;
      },
      filterActive,
    };
    return {
      rows: tagRows(input),
      totals: new Map(tagGroupTotals(input).map((total) => [total.groupKey, total])),
    };
  }, [groups, selected, endValueOf, endTotal, series, filterActive]);

  const money = (value: number | null) =>
    value === null ? '—' : fmt.money(value, currency, { decimals: 0 });
  const dash = <span className="text-ink-5">—</span>;
  const hasTags = groups.some((group) => group.tags.length > 0);

  /** The figures of a tag row or a subtotal, from "Pozice" to "Dividenda ročně". */
  const figureCells = (row: TagFigures) => {
    const has = row.count > 0;
    return (
      <>
        <TableCell className="text-right num">{fmt.number(row.count)}</TableCell>
        <TableCell className="text-right font-650 text-ink num">
          {has ? money(row.value) : dash}
        </TableCell>
        <TableCell className="text-right num">
          {has && row.share !== null ? fmt.percent(row.share, 0) : '—'}
        </TableCell>
        <TableCell className="text-right num">
          {has ? (
            <GainChange amount={row.periodGain} percent={row.periodReturn} currency={currency} />
          ) : (
            dash
          )}
        </TableCell>
        <TableCell className="text-right num">
          {has ? (
            <GainChange amount={row.gain} percent={row.gainPercent} currency={currency} />
          ) : (
            '—'
          )}
        </TableCell>
        <TableCell className="text-right num">
          {row.dividend > 0 ? money(row.dividend) : dash}
        </TableCell>
      </>
    );
  };

  const subtitle = filterActive
    ? `${t('stocksAnalysis.selection', { shown: selected.length, total })} · ${t('stocksAnalysis.tagsTable.selectedNote')}`
    : t('stocksAnalysis.tagsTable.sub');

  return (
    <Card
      variant="table"
      className={cn('mb-[18px] transition-opacity duration-base', stale && 'opacity-50')}
    >
      <CardHeader>
        <div>
          <CardTitle className="flex items-center gap-2.5">
            {t('stocksAnalysis.tagsTable.title')}
            <FilterBadge tags={filterTags} />
          </CardTitle>
          <CardDescription>{subtitle}</CardDescription>
        </div>
      </CardHeader>
      {!hasTags ? (
        <EmptyState
          bare
          icon={<Tag />}
          title={t('stocksAnalysis.tagsTable.empty.title')}
          description={t('stocksAnalysis.tagsTable.empty.description')}
          action={
            <Button variant="outline" onClick={onManage}>
              <Tag />
              {t('stocksAnalysis.actions.manage')}
            </Button>
          }
        />
      ) : rows.length === 0 ? (
        <EmptyState
          bare
          icon={<Tag />}
          title={t('stocksAnalysis.holdings.emptyFilter.title')}
          description={t('stocksAnalysis.holdings.emptyFilter.description')}
        />
      ) : (
        <Table>
          {/* Heads may wrap: seven columns have to fit the 720 px of the 1080 px minimum */}
          <TableHeader className={HEAD_WRAP}>
            <TableRow>
              <TableHead>{t('stocksAnalysis.tagsTable.tag')}</TableHead>
              <TableHead className="text-right">
                {t('stocksAnalysis.tagsTable.positions')}
              </TableHead>
              <TableHead className="text-right">
                {endsToday
                  ? t('stocksAnalysis.tagsTable.value')
                  : t('stocksAnalysis.tagsTable.valueAt', { date: fmt.day(endDate) })}
              </TableHead>
              <TableHead className="text-right">{t('stocksAnalysis.tagsTable.share')}</TableHead>
              <TableHead className="text-right">
                {t('stocksAnalysis.tagsTable.periodReturn')}
              </TableHead>
              <TableHead className="text-right">
                {endsToday
                  ? t('stocksAnalysis.tagsTable.gain')
                  : t('stocksAnalysis.tagsTable.gainToday')}
              </TableHead>
              <TableHead className="text-right">{t('stocksAnalysis.tagsTable.dividend')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row, i) => {
              // A group's subtotal follows its last row
              const total =
                rows[i + 1]?.groupKey !== row.groupKey ? totals.get(row.groupKey) : undefined;
              return (
                <Fragment key={row.tag.id}>
                  <TableRow className="h-12">
                    <TableCell>
                      <span className="inline-flex items-center gap-1.5">
                        <ChipLabel>{row.tag.name}</ChipLabel>
                        <span className="whitespace-nowrap text-[10px] font-500 text-ink-4">
                          {row.groupName ?? t('stocksAnalysis.tagsTable.noGroup')}
                        </span>
                      </span>
                    </TableCell>
                    {figureCells(row)}
                  </TableRow>
                  {total && (
                    <TableRow className="h-12 border-line bg-well hover:bg-well">
                      <TableCell>
                        <span className="inline-flex items-baseline gap-1.5 whitespace-nowrap">
                          <b className="font-700 text-ink">{t('stocksAnalysis.tagsTable.total')}</b>
                          <span className="text-[10px] font-500 text-ink-4">{total.label}</span>
                        </span>
                      </TableCell>
                      {figureCells(total)}
                    </TableRow>
                  )}
                </Fragment>
              );
            })}
          </TableBody>
        </Table>
      )}
    </Card>
  );
}
