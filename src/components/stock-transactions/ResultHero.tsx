import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { StockTransactionsReport, StockTransactionsReportSettings } from '@shared/schema';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { ChangeBadge } from '@/components/common/ChangeBadge';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Skeleton } from '@/components/ui/skeleton';
import { ExplainText, ExplainTip, ExplainTitle } from './ExplainTip';
import { ReportSettings } from './ReportSettings';
import { SummaryTable } from './SummaryTable';

/**
 * The hero of the report (spec D3, D6–D8): the result after fees of the period's sales with its
 * change badge against cost plus fees and the count of sales in profit and in loss; with the
 * holding test on and a sale in the period, the taxable and the exempt result in the aside.
 * Under it the settings strip, the summary table, the loss alert and the note line.
 */
export function ResultHero({
  report,
  settings,
  onSettingsChange,
  periodLabel,
  stale,
}: {
  report: StockTransactionsReport;
  settings: StockTransactionsReportSettings;
  onSettingsChange: (next: StockTransactionsReportSettings) => void;
  periodLabel: string;
  /** The previous period's response stays on screen while the next one loads. */
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const { currency, summary } = report;
  const { signedMoney } = useReportMoney(currency);

  const total = summary.total;
  const result = Number(total.resultAfterFees);
  const spent = Number(total.cost) + Number(total.fees);
  const testOn = settings.holdingTestEnabled && summary.taxable !== null;
  const taxableResult = summary.taxable ? Number(summary.taxable.resultAfterFees) : 0;
  const exemptResult = summary.exempt ? Number(summary.exempt.resultAfterFees) : 0;
  // Sales the sums hold; "nothing sold" only when the period has no sale at all (an uncounted
  // sale is still listed in the table)
  const hasSales = summary.salesCount > 0;
  const soldNothing = report.sales.length === 0;

  const delta = hasSales ? (
    <span className="inline-flex flex-wrap items-center gap-x-[5px] gap-y-1">
      {spent > 0 && <ChangeBadge ratio={result / spent} />}
      <span className="font-450 text-ink-4">
        {spent > 0 && `${t('hero.ofCosts')} · `}
        {t('hero.sales', {
          count: summary.salesCount,
          profit: summary.salesInProfit,
          loss: summary.salesInLoss,
        })}
      </span>
    </span>
  ) : (
    <span className="font-450 text-ink-4">
      {t(soldNothing ? 'hero.noSales' : 'hero.notCounted')}
    </span>
  );

  const aside =
    testOn && hasSales ? (
      <div className={cn('text-right transition-opacity duration-base', stale && 'opacity-50')}>
        <div className="text-table font-600 text-ink-3">{t('summary.taxable')}</div>
        <b
          className={cn(
            'my-1.5 block text-stat num',
            taxableResult > 0 && 'text-gain',
            taxableResult < 0 && 'text-loss',
            taxableResult === 0 && 'text-ink'
          )}
        >
          {signedMoney(taxableResult)}
        </b>
        <small className="block text-micro font-500 text-ink-4">
          {t('hero.exempt', { amount: signedMoney(exemptResult) })}
        </small>
      </div>
    ) : undefined;

  return (
    <HeroCard
      className="mb-[17px] pb-[18px]"
      aside={aside}
      chart={
        <div className="mt-[22px] border-t border-line-soft pt-4">
          <h3 className="mb-[11px] mt-0 text-h3 text-ink">{t('summary.title')}</h3>
          <ReportSettings settings={settings} onChange={onSettingsChange} />
          <div className={cn('transition-opacity duration-base', stale && 'opacity-50')}>
            <SummaryTable
              summary={summary}
              months={testOn ? settings.holdingTestMonths : null}
              currency={currency}
            />
            {testOn && taxableResult < 0 && (
              <Alert className="mt-3">
                <Info />
                <AlertTitle>{t('summary.lossTitle')}</AlertTitle>
                <AlertDescription>{t('summary.lossBody')}</AlertDescription>
              </Alert>
            )}
          </div>
          {/* A div, not a p: the tip renders its popper in place */}
          <div className="mt-3 text-micro font-500 leading-[1.6] text-ink-4">
            {t('summary.note', { currency })}{' '}
            <ExplainTip label={t('tips.fx.label')}>
              <ExplainTitle>{t('tips.fx.title', { currency })}</ExplainTitle>
              <ExplainText>{t('tips.fx.rate')}</ExplainText>
              <ExplainText>{t('tips.fx.return')}</ExplainText>
            </ExplainTip>{' '}
            · {t(settings.costMethod === 'fifo' ? 'summary.noteFifo' : 'summary.noteAverage')} ·{' '}
            {t('summary.noteDividends')}
          </div>
        </div>
      }
    >
      <HeroValue
        className={cn('transition-opacity duration-base', stale && 'opacity-50')}
        label={t('hero.label', { period: periodLabel })}
        value={signedMoney(result)}
        delta={delta}
      />
    </HeroCard>
  );
}

/** Skeleton in the shape of the hero: the result, the settings strip and the summary rows. */
export function ResultHeroSkeleton() {
  return (
    <HeroCard
      className="mb-[17px] pb-[18px]"
      chart={
        <div aria-hidden className="mt-[22px] border-t border-line-soft pt-4">
          <Skeleton className="mb-[11px] h-4 w-64" />
          <Skeleton className="mb-[10px] h-[46px] w-full" />
          {[0, 1, 2].map((i) => (
            <div key={i} className="flex h-[46px] items-center justify-between gap-6">
              <Skeleton className="h-3 w-40" />
              <Skeleton className="h-3 w-[55%]" />
            </div>
          ))}
        </div>
      }
    >
      <div aria-hidden className="min-w-0">
        <Skeleton className="h-3 w-56" />
        <Skeleton className="mb-2 mt-3 h-10 w-52" />
        <Skeleton className="h-3 w-72" />
      </div>
    </HeroCard>
  );
}
