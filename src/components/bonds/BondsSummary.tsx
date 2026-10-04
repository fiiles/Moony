import { useTranslation } from 'react-i18next';
import type { CurrencyCode } from '@shared/currencies';
import type { BondsMetrics } from '@/hooks/use-bonds';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Stat, Stats } from '@/components/common/Stat';
import { untilText } from '@/components/bonds/bond-until';

interface BondsSummaryProps {
  metrics: BondsMetrics;
  today: number;
}

/**
 * Four stats of the bonds list (prototype bonds.html): face value, yearly
 * coupon with the weighted rate, the nearest maturity and how much it
 * returns, the face-value-weighted time to maturity.
 */
export function BondsSummary({ metrics, today }: BondsSummaryProps) {
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('bonds');

  const income = convert(metrics.projectedYearlyIncome, 'CZK', currencyCode as CurrencyCode);
  const issuesNote = [
    t('summary.issues', { count: metrics.liveCount }),
    metrics.currencies.length > 1
      ? t('summary.currencies', { count: metrics.currencies.length })
      : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <Stats>
      <Stat
        label={t('summary.nominal')}
        value={formatCurrency(metrics.totalValue)}
        note={issuesNote}
      />
      <Stat
        label={t('summary.yearlyCoupon')}
        value={fmt.money(income, currencyCode, { signed: true, decimals: 0 })}
        tone={income > 0 ? 'gain' : 'neutral'}
        noteTone="neutral"
        note={t('summary.weightedRate', { rate: fmt.percent(metrics.averageYield / 100, 2) })}
      />
      <Stat
        label={t('summary.nearestMaturity')}
        value={metrics.nearest?.maturityDate ? fmt.day(metrics.nearest.maturityDate) : '—'}
        note={
          metrics.nearest?.maturityDate
            ? `${untilText(t, metrics.nearest.maturityDate, today, fmt.day)} · ${t(
                'summary.returns',
                {
                  amount: formatCurrency(metrics.nearestValueCzk),
                }
              )}`
            : t('summary.noMaturity')
        }
      />
      <Stat
        label={t('summary.avgToMaturity')}
        value={
          metrics.weightedYears !== null
            ? Number.isInteger(Math.round(metrics.weightedYears * 10) / 10)
              ? t('duration.years', { count: Math.round(metrics.weightedYears) })
              : t('summary.years', {
                  value: fmt.number(metrics.weightedYears, { maximumFractionDigits: 1 }),
                })
            : '—'
        }
        note={
          metrics.lastMaturity !== null
            ? t('summary.lastMaturity', { date: fmt.day(metrics.lastMaturity) })
            : metrics.liveCount > 0
              ? t('summary.noMaturity')
              : t('summary.noneLive')
        }
      />
    </Stats>
  );
}
