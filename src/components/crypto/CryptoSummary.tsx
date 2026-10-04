import { useTranslation } from 'react-i18next';
import { Stat, Stats } from '@/components/common/Stat';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { cn } from '@/lib/utils';

interface CryptoSummaryProps {
  metrics: {
    totalValue: number;
    totalCost: number;
    overallGainLoss: number;
    overallGainLossPercent: number;
  };
  positionCount: number;
  realizedGain: number;
  sellCount: number;
  largest: { name: string; share: number; gainLossPercent: number } | null;
  latestFetchedAt?: Date;
  isLoading?: boolean;
}

/**
 * Four stats of the crypto list (design system §7 List, prototype crypto.html):
 * value with position count and freshness, unrealized return against the
 * invested amount, realized return (always shown, with the number of sells),
 * largest position with its share of the crypto total.
 */
export function CryptoSummary({
  metrics,
  positionCount,
  realizedGain,
  sellCount,
  largest,
  latestFetchedAt,
  isLoading,
}: CryptoSummaryProps) {
  const { formatCurrencyRaw, currencyCode } = useCurrency();
  const fmt = useFormat();
  const { t } = useTranslation('crypto');

  const tone = (v: number) => (v > 0 ? 'gain' : v < 0 ? 'loss' : 'neutral');
  const fetched = latestFetchedAt ? Math.floor(latestFetchedAt.getTime() / 1000) : null;
  const freshness =
    fetched === null
      ? t('summary.notUpdated')
      : utcDayFloor(fetched) === utcDayFloor(Date.now() / 1000)
        ? t('summary.updatedToday')
        : t('summary.updatedOn', { date: fmt.day(fetched) });

  return (
    <Stats className={cn('transition-opacity duration-base', isLoading && 'opacity-50')}>
      <Stat
        label={t('summary.value')}
        value={formatCurrencyRaw(metrics.totalValue)}
        note={`${t('summary.positions', { count: positionCount })} · ${freshness}`}
      />
      <Stat
        label={
          metrics.overallGainLoss >= 0 ? t('summary.unrealizedGain') : t('summary.unrealizedLoss')
        }
        value={fmt.money(metrics.overallGainLoss, currencyCode, { signed: true, decimals: 0 })}
        tone={tone(metrics.overallGainLoss)}
        note={`${fmt.percent(metrics.overallGainLossPercent / 100, 1, { signed: true })} ${t(
          'summary.vsInvested',
          { amount: formatCurrencyRaw(metrics.totalCost) }
        )}`}
      />
      <Stat
        label={t('summary.realized')}
        value={fmt.money(realizedGain, currencyCode, { signed: true, decimals: 0 })}
        tone={tone(realizedGain)}
        noteTone="neutral"
        note={sellCount > 0 ? t('summary.sells', { count: sellCount }) : t('summary.noSells')}
      />
      <Stat
        label={t('summary.largest')}
        value={largest?.name ?? '—'}
        note={
          largest
            ? t('summary.largestNote', {
                share: fmt.percent(largest.share, 0),
                change: fmt.percent(largest.gainLossPercent / 100, 1, { signed: true }),
              })
            : t('summary.noPositions')
        }
        noteTone={largest ? tone(largest.gainLossPercent) : 'neutral'}
      />
    </Stats>
  );
}
