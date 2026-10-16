import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, Tag } from 'lucide-react';
import type { PositionPerformance } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { positionCurrency } from '@/utils/stock-performance';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import { AssetLogo } from '@/components/common/AssetLogo';
import { EmptyState } from '@/components/common/EmptyState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { GainChange } from './GainChange';
import { amountOf, sectorLabel, sumKnown } from './performance-model';
import { holdingRows, portfolioConcentration } from './tables-model';

const HEAD_WRAP = '[&_th]:whitespace-normal [&_th]:leading-[1.2]';

interface HoldingsTableProps {
  /** Every held position (the concentration is of the whole portfolio) and the selected ones. */
  stocks: readonly AnalysisPosition[];
  selected: readonly AnalysisPosition[];
  /** Value of a position as of the end of the period; of the whole portfolio. */
  endValueOf: (positionId: string) => number | null;
  endTotal: number | null;
  /** Return of each position over the period (from the performance response on screen). */
  performance: ReadonlyMap<string, PositionPerformance>;
  /** The period ends today: "Hodnota", else "K {date}" and the unrealized gain says "dnes". */
  endsToday: boolean;
  endDate: number;
  /** Report currency of every amount. */
  currency: string;
  /** The figures are the previous period's: the next response is loading. */
  stale: boolean;
  /** Open the assign dialog for these positions. */
  onAssign: (positionIds: string[]) => void;
}

/**
 * The positions with their tags: value at the end of the period, return over the period and the
 * unrealized gain (as of today), largest first; a search box narrows by company and ticker. The
 * subtitle carries the concentration of the whole portfolio.
 */
export function HoldingsTable({
  stocks,
  selected,
  endValueOf,
  endTotal,
  performance,
  endsToday,
  endDate,
  currency,
  stale,
  onAssign,
}: HoldingsTableProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const [query, setQuery] = useState('');

  const rows = useMemo(
    () => holdingRows(selected, endValueOf, query),
    [selected, endValueOf, query]
  );
  const rowsValue = useMemo(
    () => sumKnown(rows.map((p) => endValueOf(p.id))) ?? 0,
    [rows, endValueOf]
  );
  const concentration = useMemo(
    () => portfolioConcentration(stocks, endValueOf),
    [stocks, endValueOf]
  );

  const money = (value: number | null) =>
    value === null ? '—' : fmt.money(value, currency, { decimals: 0 });
  const share = endTotal !== null && endTotal > 0 ? rowsValue / endTotal : 0;

  return (
    <Card variant="table" className={cn('transition-opacity duration-base', stale && 'opacity-50')}>
      <CardHeader>
        <div>
          <CardTitle>{t('stocksAnalysis.holdings.title')}</CardTitle>
          <CardDescription>
            {t('stocksAnalysis.holdings.sub', {
              shown: rows.length,
              total: stocks.length,
              amount: money(rowsValue),
              percent: fmt.percent(share, 0),
            })}
            {concentration && (
              <>
                {' · '}
                <span
                  className="cursor-help border-b border-dotted border-ink-5"
                  title={t('stocksAnalysis.holdings.concentrationTitle')}
                >
                  {t('stocksAnalysis.holdings.concentration', {
                    share: fmt.percent(concentration.top5Share, 0),
                    count: fmt.number(concentration.effective, {
                      minimumFractionDigits: 1,
                      maximumFractionDigits: 1,
                    }),
                  })}
                </span>
              </>
            )}
          </CardDescription>
        </div>
        <InputWrap icon={<Search />} className="w-[220px]">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('stocksAnalysis.holdings.search')}
            aria-label={t('stocksAnalysis.holdings.search')}
          />
        </InputWrap>
      </CardHeader>
      {rows.length === 0 ? (
        <EmptyState
          bare
          icon={<Tag />}
          title={t('stocksAnalysis.holdings.emptyFilter.title')}
          description={t('stocksAnalysis.holdings.emptyFilter.description')}
        />
      ) : (
        <Table>
          {/* Heads may wrap: the columns have to fit the 720 px of the 1080 px minimum */}
          <TableHeader className={HEAD_WRAP}>
            <TableRow>
              <TableHead className="w-[30%]">{t('stocksAnalysis.holdings.stock')}</TableHead>
              <TableHead>{t('stocksAnalysis.holdings.tags')}</TableHead>
              <TableHead className="text-right">
                {endsToday
                  ? t('stocksAnalysis.holdings.value')
                  : t('stocksAnalysis.board.valueAt', { date: fmt.day(endDate) })}
              </TableHead>
              <TableHead className="text-right">
                {t('stocksAnalysis.holdings.periodReturn')}
              </TableHead>
              <TableHead className="text-right">
                {endsToday
                  ? t('stocksAnalysis.holdings.gain')
                  : t('stocksAnalysis.tagsTable.gainToday')}
              </TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((s) => {
              const period = performance.get(s.id);
              return (
                <TableRow key={s.id} className="h-[58px]">
                  {/* max-w-0: a long company name truncates within the 30 % instead of widening it */}
                  <TableCell className="max-w-0">
                    <div className="flex min-w-0 items-center gap-[10px] text-ink">
                      <AssetLogo ticker={s.ticker} type="stock" />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{s.companyName}</b>
                        <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                          {[
                            s.ticker,
                            positionCurrency(s),
                            s.sector ? sectorLabel(s.sector, t) : t('stocksAnalysis.holdings.etf'),
                          ].join(' · ')}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-wrap items-center gap-1.5">
                      {s.tags.length > 0 ? (
                        s.tags.map((x) => <Badge key={x.id}>{x.name}</Badge>)
                      ) : (
                        <>
                          <Badge variant="outline">{t('stocksAnalysis.holdings.noTag')}</Badge>
                          <button
                            type="button"
                            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                            onClick={() => onAssign([s.id])}
                          >
                            {t('stocksAnalysis.holdings.assign')}
                          </button>
                        </>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {money(endValueOf(s.id))}
                  </TableCell>
                  <TableCell className="text-right num">
                    <GainChange
                      amount={amountOf(period?.gain)}
                      percent={period?.twr ?? null}
                      currency={currency}
                    />
                  </TableCell>
                  <TableCell className="text-right num">
                    <GainChange
                      amount={s.gainLoss}
                      percent={s.gainLossPercent}
                      currency={currency}
                    />
                  </TableCell>
                  <TableCell className="pr-3 text-right">
                    <div data-row-actions>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={t('stocksAnalysis.holdings.editTags')}
                        onClick={() => onAssign([s.id])}
                      >
                        <Tag />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </Card>
  );
}
