import { useTranslation } from 'react-i18next';
import type { StockReportStats } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { ExplainText, ExplainTip, ExplainTitle } from './ExplainTip';

/**
 * The four stats under the hero (spec D9): sale proceeds (every sale of the period, uncovered
 * parts included), the volume bought, the fees of every trade with their share of the traded
 * volume (not the fee figure of the summary table), and the gross dividends with an ex-date in
 * the period.
 */
export function ReportStats({
  stats,
  currency,
  stale,
}: {
  stats: StockReportStats;
  currency: string;
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { money } = useReportMoney(currency);

  const volume = Number(stats.tradedVolume);
  const fees = Number(stats.feesPaid);
  const trades = stats.salesCount + stats.buysCount;
  const counted = (count: string, tickers: number) =>
    `${count} · ${t('stats.tickers', { count: tickers })}`;

  return (
    <Stats className={cn('transition-opacity duration-base', stale && 'opacity-50')}>
      <Stat
        label={t('stats.proceeds')}
        value={money(Number(stats.proceeds))}
        note={
          stats.salesCount > 0
            ? counted(t('stats.sales', { count: stats.salesCount }), stats.soldTickers)
            : t('stats.noSales')
        }
      />
      <Stat
        label={t('stats.bought')}
        value={money(Number(stats.bought))}
        note={
          stats.buysCount > 0
            ? counted(t('stats.buys', { count: stats.buysCount }), stats.boughtTickers)
            : t('stats.noBuys')
        }
      />
      <Stat
        label={t('stats.fees')}
        value={money(fees)}
        note={
          trades === 0
            ? t('stats.noTrades')
            : volume > 0
              ? t('stats.feesNote', { percent: fmt.percent(fees / volume, 2) })
              : t('stats.feesNoteBare')
        }
      />
      <Stat
        label={
          <span className="inline-flex items-center gap-1">
            {t('stats.dividends')}
            <ExplainTip label={t('tips.dividends.label')}>
              <ExplainTitle>{t('tips.dividends.title')}</ExplainTitle>
              <ExplainText>{t('tips.dividends.body', { currency })}</ExplainText>
            </ExplainTip>
          </span>
        }
        value={money(Number(stats.dividends))}
        note={
          stats.dividendsCount > 0
            ? `${t('stats.payouts', { count: stats.dividendsCount })} · ${t('stats.outsideResult')}`
            : t('stats.noPayouts')
        }
      />
    </Stats>
  );
}

/** Four stat skeletons while the first response loads. */
export function ReportStatsSkeleton() {
  return (
    <Stats>
      {[0, 1, 2, 3].map((i) => (
        <StatSkeleton key={i} />
      ))}
    </Stats>
  );
}
