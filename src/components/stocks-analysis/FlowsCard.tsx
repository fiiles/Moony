import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockPerformance } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  DAY,
  bucketKind,
  dayFromIso,
  flowBuckets,
  isoDay,
  type BucketKind,
} from '@/utils/stock-performance';
import { toneOf } from '@/utils/performance-format';
import { MoonyBarChart, type BarGroup } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { flowBridge, flowLabel, type FlowLabel } from './flows-model';
import { amountOf } from './performance-model';

interface FlowsCardProps {
  /** The response on screen; the previous one while the next loads (`stale`). */
  data: StockPerformance | undefined;
  loading: boolean;
  stale: boolean;
  /** The period ends today: "Dnes", else "K {date}". */
  endsToday: boolean;
}

/** Room one axis label needs (px), by bucket kind: a longer span is thinned instead of squeezed. */
const LABEL_WIDTH: Record<BucketKind, number> = { week: 32, month: 42, quarter: 46, year: 34 };

/** Skeleton in the shape of the card: the head, the four cells and the bars. */
export function FlowsSkeleton() {
  return (
    <Card className="flex flex-col pb-[18px]">
      <CardHeader className="items-start">
        <div>
          <Skeleton className="h-5 w-48" />
          <Skeleton className="mt-2 h-3 w-60" />
        </div>
      </CardHeader>
      <Skeleton className="mx-[21px] h-[58px]" />
      <Skeleton className="mx-[21px] mt-[18px] h-[168px]" />
    </Card>
  );
}

/**
 * "Vklady a zhodnocení": what moved the value of the whole portfolio, split into net deposits and
 * the market. A bridge from the value at the start to the value at the end, and bars per week,
 * month, quarter or year (by the length of the period): market `s1`, deposits `s3`, both below
 * the zero line when negative. The bars fill the height the neighbouring card sets.
 */
export function FlowsCard({ data, loading, stale, endsToday }: FlowsCardProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();

  const dates = useMemo(() => (data?.dates ?? []).map((iso) => dayFromIso(iso) ?? 0), [data]);
  const values = useMemo(() => (data?.values ?? []).map(amountOf), [data]);
  const netFlows = useMemo(() => (data?.netFlows ?? []).map((v) => amountOf(v) ?? 0), [data]);
  const last = dates.length - 1;
  const kind = bucketKind(last > 0 ? (dates[last] - dates[0]) / DAY : 0);
  const buckets = useMemo(
    () => flowBuckets(dates, values, netFlows, kind),
    [dates, values, netFlows, kind]
  );
  const bridge = useMemo(() => flowBridge(values, netFlows), [values, netFlows]);

  if (!data) return loading ? <FlowsSkeleton /> : null;

  const currency = data.currency;
  const money = (value: number | null, signed = false) =>
    value === null ? '—' : fmt.money(value, currency, { decimals: 0, signed });
  const signedMoney = (value: number) => money(value, true);

  const axisText = (label: FlowLabel): string => {
    switch (label.kind) {
      case 'week':
        return fmt.day(label.ts, { day: 'numeric', month: 'numeric' });
      case 'month':
        return fmt.day(
          label.ts,
          label.withYear ? { month: 'short', year: '2-digit' } : { month: 'short' }
        );
      case 'quarter':
        return t('stocksAnalysis.flows.quarter', {
          quarter: label.quarter,
          year: String(label.year).slice(2),
        });
      case 'year':
        return String(label.year);
    }
  };
  const bars: BarGroup[] = buckets.map((bucket, i) => ({
    label: isoDay(bucket.start),
    axisLabel: axisText(flowLabel(bucket, kind, i === 0)),
    title: `${fmt.day(bucket.start)} – ${fmt.day(bucket.end)}`,
    a: bucket.market,
    b: bucket.deposits,
  }));

  const from = fmt.day(dates[0]);
  const unit = t(`stocksAnalysis.flows.units.${kind}`);
  const subtitle = endsToday
    ? t('stocksAnalysis.flows.sub', { unit, from })
    : t('stocksAnalysis.flows.subTo', { unit, from, to: fmt.day(dates[last]) });

  const marketTone = toneOf(bridge?.market, 0);
  const cells = [
    { label: t('stocksAnalysis.flows.start'), value: money(bridge?.start ?? null), tone: '' },
    {
      label: t('stocksAnalysis.flows.deposits'),
      value: bridge?.deposits == null ? '—' : signedMoney(bridge.deposits),
      tone: '',
    },
    {
      label: t('stocksAnalysis.flows.market'),
      value: bridge?.market == null ? '—' : signedMoney(bridge.market),
      tone: marketTone === 'gain' ? 'text-gain' : marketTone === 'loss' ? 'text-loss' : '',
    },
    {
      label: endsToday
        ? t('stocksAnalysis.flows.end')
        : t('stocksAnalysis.flows.endAt', { date: fmt.day(dates[last]) }),
      value: money(bridge?.end ?? null),
      tone: '',
    },
  ];

  return (
    <Card className={cn('flex flex-col transition-opacity duration-base', stale && 'opacity-50')}>
      <CardHeader className="items-start">
        <div className="min-w-0">
          <CardTitle className="text-[16px]">{t('stocksAnalysis.flows.title')}</CardTitle>
          <CardDescription>{subtitle}</CardDescription>
        </div>
      </CardHeader>
      {/* Two by two below 1500 px (the card is 351 px wide at the 1080 px minimum), four across from there */}
      <div className="mx-[21px] grid grid-cols-2 overflow-hidden rounded-r2 border border-line-soft bg-paper min-[1500px]:grid-cols-4">
        {cells.map((cell, i) => (
          <div
            key={cell.label}
            className={cn(
              'min-w-0 border-line-soft px-3 pb-[11px] pt-[10px]',
              i % 2 === 1 && 'border-l',
              i >= 2 && 'border-t min-[1500px]:border-t-0',
              i === 2 && 'min-[1500px]:border-l'
            )}
          >
            <small className="block truncate text-micro font-600 text-ink-4">{cell.label}</small>
            <b
              className={cn(
                'mt-1 block truncate text-[14px] font-650 num',
                cell.tone || 'text-ink'
              )}
            >
              {cell.value}
            </b>
          </div>
        ))}
      </div>
      {bars.length === 0 ? (
        <p className="m-0 grid min-h-[168px] flex-1 place-items-center px-[21px] text-table text-ink-3">
          {t('stocksAnalysis.performance.empty')}
        </p>
      ) : (
        <MoonyBarChart
          className="mx-[17px] mt-[18px] flex-1"
          bars={bars}
          names={[t('stocksAnalysis.flows.market'), t('stocksAnalysis.flows.deposits')]}
          height={168}
          fill
          labelSpacing={LABEL_WIDTH[kind]}
          formatValue={signedMoney}
          renderExtra={(bar) => `${t('stocksAnalysis.flows.change')} ${signedMoney(bar.a + bar.b)}`}
        />
      )}
      <ChartLegend
        className="mx-[21px] mb-[18px] mt-[10px]"
        items={[
          {
            label: t('stocksAnalysis.flows.legendDeposits'),
            swatch: { kind: 'block', color: 'var(--s3)' },
          },
          {
            label: t('stocksAnalysis.flows.market'),
            swatch: { kind: 'block', color: 'var(--s1)' },
          },
        ]}
      />
    </Card>
  );
}
