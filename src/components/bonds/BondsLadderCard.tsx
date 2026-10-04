import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { Bond } from '@shared/schema';
import { bondLadder, nearestMaturity } from '@/utils/bonds';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Badge } from '@/components/ui/badge';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { TREND_CHART_HEIGHT, TrendCard } from '@/components/charts/TrendCard';

interface BondsLadderCardProps {
  bonds: Bond[];
  today: number;
}

/**
 * Maturity-and-coupon ladder (prototype bonds.html, the page's time view):
 * stacked bars per calendar year with the face value coming back and the
 * coupons still due, the nearest maturity marked.
 */
export function BondsLadderCard({ bonds, today }: BondsLadderCardProps) {
  const { formatCurrency } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('bonds');

  const ladder = useMemo(() => bondLadder(bonds, today), [bonds, today]);
  const nearest = nearestMaturity(bonds, today);
  const nearestYear = nearest?.maturityDate
    ? new Date(nearest.maturityDate * 1000).getUTCFullYear()
    : null;
  const totals = ladder.reduce(
    (acc, y) => ({ principal: acc.principal + y.principal, coupons: acc.coupons + y.coupons }),
    { principal: 0, coupons: 0 }
  );

  if (ladder.length === 0) return null;

  return (
    <TrendCard
      title={t('ladder.title')}
      subtitle={t('ladder.subtitle')}
      aside={<Badge variant="outline">{t('ladder.badge')}</Badge>}
      legend={
        <ChartLegend
          items={[
            { label: t('ladder.principal'), swatch: { kind: 'block', color: 'var(--s1)' } },
            { label: t('ladder.coupons'), swatch: { kind: 'block', color: 'var(--s3)' } },
          ]}
          note={t('ladder.note', {
            year: ladder[ladder.length - 1].year,
            principal: formatCurrency(totals.principal),
            coupons: formatCurrency(totals.coupons),
          })}
        />
      }
    >
      <MoonyBarChart
        bars={ladder.map((y) => ({ label: String(y.year), a: y.principal, b: y.coupons }))}
        stacked
        height={TREND_CHART_HEIGHT}
        names={[t('ladder.principal'), t('ladder.coupons')]}
        formatValue={(v) => formatCurrency(v)}
        marks={
          nearest?.maturityDate && nearestYear !== null
            ? [
                {
                  index: ladder.findIndex((y) => y.year === nearestYear),
                  title: t('ladder.nearest'),
                  lines: [
                    `${fmt.day(nearest.maturityDate)} · ${nearest.name}`,
                    formatCurrency(ladder.find((y) => y.year === nearestYear)?.principal ?? 0),
                  ],
                },
              ].filter((m) => m.index >= 0)
            : []
        }
      />
    </TrendCard>
  );
}
