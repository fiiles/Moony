import { useTranslation } from 'react-i18next';
import { Stat, Stats } from '@/components/common/Stat';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { cn } from '@/lib/utils';

interface InvestmentsSummaryProps {
  /** The report currency of every amount below. */
  currency: string;
  /** Totals of the response; `null` when unknown (held, none converted), shown as "—". */
  metrics: {
    totalValue: number | null;
    totalCost: number | null;
    overallGainLoss: number | null;
    overallGainLossPercent: number;
    /** Yearly dividend estimate in `currency`. */
    estimatedDividendYield: number | null;
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
  currency,
  metrics,
  positionCount,
  realizedGain = 0,
  latestFetchedAt,
  isLoading,
}: InvestmentsSummaryProps) {
  const fmt = useFormat();
  const { t } = useTranslation('stocks');

  const tone = (v: number | null) =>
    v === null ? 'neutral' : v > 0 ? 'gain' : v < 0 ? 'loss' : 'neutral';
  const money = (v: number | null, signed = false) =>
    v === null ? '—' : fmt.money(v, currency, { signed, decimals: 0 });
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
          amount: fmt.money(realizedGain, currency, { signed: true, decimals: 0 }),
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
        value={money(metrics.totalValue)}
        note={`${t('summary.positions', { count: positionCount })} · ${freshness}`}
      />
      <Stat
        label={
          (metrics.overallGainLoss ?? 0) >= 0
            ? t('summary.unrealizedGain')
            : t('summary.unrealizedLoss')
        }
        value={money(metrics.overallGainLoss, true)}
        tone={tone(metrics.overallGainLoss)}
        note={unrealizedNote}
      />
      <Stat
        label={t('summary.estimatedDividend')}
        value={money(metrics.estimatedDividendYield)}
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
