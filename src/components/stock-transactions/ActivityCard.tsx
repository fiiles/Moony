import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockReportBucket } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';

/**
 * "Nákupy a prodeje v čase" (spec D10): the volume bought (s1) and sold (s3) per bucket, a month
 * each or a year each for a period over 24 months. Hover shows the bucket's buys, sells and the
 * result of its sales; a click selects the bucket (the page narrows the table card to it and
 * the other bars dim), a second click on it clears the selection.
 */
export function ActivityCard({
  buckets,
  currency,
  bucket,
  onBucketChange,
  stale,
}: {
  buckets: StockReportBucket[];
  currency: string;
  bucket: StockReportBucket | null;
  onBucketChange: (next: StockReportBucket | null) => void;
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { money, signedMoney } = useReportMoney(currency);

  const yearly = buckets.length > 0 && buckets[0].key.length === 4;
  const bars = useMemo(
    () =>
      buckets.map((b) => {
        const date = new Date(b.fromDay * 1000);
        return {
          // Unique per bar; the axis and the tooltip read their own texts
          label: b.key,
          axisLabel: yearly ? b.key : fmt.month(date, { month: 'short', year: '2-digit' }),
          title: yearly ? b.key : fmt.month(date, { month: 'long', year: 'numeric' }),
          a: Number(b.buys),
          b: Number(b.sells),
          muted: bucket !== null && bucket.key !== b.key,
        };
      }),
    [buckets, bucket, yearly, fmt]
  );
  const resultByKey = useMemo(() => new Map(buckets.map((b) => [b.key, b])), [buckets]);

  const totalBuys = buckets.reduce((sum, b) => sum + Number(b.buys), 0);
  const totalSells = buckets.reduce((sum, b) => sum + Number(b.sells), 0);

  return (
    <Card className="flex min-w-0 flex-col">
      <CardHeader>
        <div className="min-w-0">
          <CardTitle>{t('activity.title')}</CardTitle>
          <CardDescription>
            {t(yearly ? 'activity.subYear' : 'activity.subMonth', { currency })}
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent
        className={cn(
          'flex flex-1 flex-col transition-opacity duration-base',
          stale && 'opacity-50'
        )}
      >
        <MoonyBarChart
          className="-mx-1 flex-1"
          bars={bars}
          names={[t('activity.buys'), t('activity.sells')]}
          height={200}
          fill
          labelSpacing={44}
          formatValue={money}
          renderExtra={(bar) => {
            const source = resultByKey.get(bar.label);
            return source && bar.b > 0
              ? `${t('activity.result')} ${signedMoney(Number(source.result))}`
              : null;
          }}
          onBarClick={(index) => {
            const clicked = buckets[index];
            if (clicked) onBucketChange(bucket?.key === clicked.key ? null : clicked);
          }}
        />
        <ChartLegend
          items={[
            {
              label: (
                <>
                  {t('activity.buys')} <b className="text-ink">{money(totalBuys)}</b>
                </>
              ),
              swatch: { kind: 'block', color: 'var(--s1)' },
            },
            {
              label: (
                <>
                  {t('activity.sells')} <b className="text-ink">{money(totalSells)}</b>
                </>
              ),
              swatch: { kind: 'block', color: 'var(--s3)' },
            },
          ]}
        />
      </CardContent>
    </Card>
  );
}
