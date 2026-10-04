import { useTranslation } from 'react-i18next';
import { Stat, Stats } from '@/components/common/Stat';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { cn } from '@/lib/utils';

interface InvestmentsSummaryProps {
  metrics: {
    totalValue: number;
    totalCost: number;
    overallGainLoss: number;
    overallGainLossPercent: number;
    estimatedDividendYield: number;
    topPerformer: { ticker: string; companyName?: string; gainLossPercent: number } | null;
  };
  positionCount: number;
  realizedGain?: number;
  latestFetchedAt?: Date;
  isLoading?: boolean;
}

/**
 * Four stats of the stocks list (design system §7 List): portfolio value with
 * the position count and freshness, unrealized return against invested (with
 * the realized return as a note when there is one), dividend estimate for the
 * next twelve months, best position.
 */
export function InvestmentsSummary({
  metrics,
  positionCount,
  realizedGain = 0,
  latestFetchedAt,
  isLoading,
}: InvestmentsSummaryProps) {
  const { formatCurrencyRaw, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('stocks');

  const tone = (v: number) => (v > 0 ? 'gain' : v < 0 ? 'loss' : 'neutral');
  const fetched = latestFetchedAt ? Math.floor(latestFetchedAt.getTime() / 1000) : null;
  const freshness =
    fetched === null
      ? t('summary.notUpdated')
      : utcDayFloor(fetched) === utcDayFloor(Date.now() / 1000)
        ? t('summary.updatedToday')
        : t('summary.updatedOn', { date: fmt.day(fetched) });

  const unrealizedNote = [
    `${fmt.percent(metrics.overallGainLossPercent / 100, 1, { signed: true })} ${t('summary.vsInvested')}`,
    realizedGain !== 0
      ? t('summary.realizedNote', {
          amount: fmt.money(realizedGain, currencyCode, { signed: true, decimals: 0 }),
        })
      : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <Stats
      className={cn(
        'grid-cols-[1.2fr_0.9fr_0.9fr_0.9fr] transition-opacity duration-base',
        isLoading && 'opacity-50'
      )}
    >
      <Stat
        label={t('summary.totalValue')}
        value={formatCurrencyRaw(metrics.totalValue)}
        note={`${t('summary.positions', { count: positionCount })} · ${freshness}`}
      />
      <Stat
        label={
          metrics.overallGainLoss >= 0 ? t('summary.unrealizedGain') : t('summary.unrealizedLoss')
        }
        value={fmt.money(metrics.overallGainLoss, currencyCode, { signed: true, decimals: 0 })}
        tone={tone(metrics.overallGainLoss)}
        note={unrealizedNote}
      />
      <Stat
        label={t('summary.estimatedDividend')}
        value={formatCurrencyRaw(metrics.estimatedDividendYield)}
        note={t('summary.next12Months')}
      />
      <Stat
        label={t('summary.topPosition')}
        value={metrics.topPerformer?.companyName || metrics.topPerformer?.ticker || '—'}
        note={
          metrics.topPerformer
            ? `${fmt.percent(metrics.topPerformer.gainLossPercent / 100, 1, { signed: true })} ${t('summary.overall')}`
            : t('summary.noPositions')
        }
        noteTone={metrics.topPerformer ? tone(metrics.topPerformer.gainLossPercent) : 'neutral'}
      />
    </Stats>
  );
}
