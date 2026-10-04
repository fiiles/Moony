import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { Bond } from '@shared/schema';
import { bondLadder, nearestMaturity } from '@/utils/bonds';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Badge } from '@/components/ui/badge';
import { Card } from '@/components/ui/card';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';

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
    <Card className='relative mb-7 overflow-hidden px-[21px] pb-[14px] pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-[""]'>
      <div className="relative z-[1] flex items-center justify-between gap-6">
        <div>
          <h3 className="m-0 text-h3 text-ink">{t('ladder.title')}</h3>
          <p className="mt-[5px] text-micro font-500 text-ink-4">{t('ladder.subtitle')}</p>
        </div>
        <Badge variant="outline">{t('ladder.badge')}</Badge>
      </div>
      <MoonyBarChart
        className="relative z-[1] -mx-1 mt-[14px]"
        bars={ladder.map((y) => ({ label: String(y.year), a: y.principal, b: y.coupons }))}
        stacked
        height={170}
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
      <div className="relative z-[1]">
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
      </div>
    </Card>
  );
}
