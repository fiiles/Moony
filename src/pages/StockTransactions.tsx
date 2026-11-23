import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Receipt } from 'lucide-react';
import type { StockReportBucket, StockTransactionsReportRequest } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useStockTransactionsReport } from '@/hooks/use-stock-transactions-report';
import { useStockTransactionsSettings } from '@/hooks/use-stock-transactions-settings';
import { yearPeriod } from '@/utils/stock-transactions-report';
import { utcDay } from '@/utils/stock-performance';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ConversionQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { PeriodControls, type PeriodMode } from '@/components/stock-transactions/PeriodControls';
import { ResultHero, ResultHeroSkeleton } from '@/components/stock-transactions/ResultHero';
import { ReportStats, ReportStatsSkeleton } from '@/components/stock-transactions/ReportStats';
import { ActivityCard } from '@/components/stock-transactions/ActivityCard';
import { TickerResultsCard } from '@/components/stock-transactions/TickerResultsCard';
import { HoldingTestCard } from '@/components/stock-transactions/HoldingTestCard';
import { TransactionsCard } from '@/components/stock-transactions/TransactionsCard';

const yearOf = (day: number) => new Date(day * 1000).getUTCFullYear();

/**
 * Akciové transakce (spec 2026-11-23, D1–D3): the sales, buys and dividends of a period for a
 * tax return. The period is page state (a calendar year or a custom range) and is not stored;
 * the cost method and the holding test are stored settings. This component holds the state and
 * composes the cards; the report is requested only once the settings have loaded.
 */
export default function StockTransactions() {
  const { t } = useTranslation('stockTransactions');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const { settings, isLoading: settingsLoading, save } = useStockTransactionsSettings();

  const today = utcDay(Math.floor(Date.now() / 1000));
  const currentYear = yearOf(today);
  const [mode, setMode] = useState<PeriodMode>('year');
  const [year, setYear] = useState(currentYear);
  // The custom range ("Vlastní období"); the year mode reads its period from `year`
  const [fromDay, setFromDay] = useState(() => yearPeriod(currentYear).fromDay);
  const [toDay, setToDay] = useState(today);
  // The chart bucket that narrows the table card; a new period clears it
  const [bucket, setBucket] = useState<StockReportBucket | null>(null);

  const period = mode === 'year' ? yearPeriod(year) : { fromDay, toDay };
  const request = useMemo<StockTransactionsReportRequest | null>(
    () =>
      settingsLoading
        ? null
        : {
            fromDay: period.fromDay,
            toDay: period.toDay,
            reportCurrency: currencyCode,
            costMethod: settings.costMethod,
            holdingTestMonths: settings.holdingTestEnabled ? settings.holdingTestMonths : null,
          },
    [settingsLoading, period.fromDay, period.toDay, currencyCode, settings]
  );
  const { data: report, isPlaceholderData, isError, refetch } = useStockTransactionsReport(request);

  const firstDay = report?.firstTransactionDay ?? null;
  const minYear = Math.min(firstDay === null ? currentYear : yearOf(firstDay), currentYear);

  const changeYear = (next: number) => {
    setYear(next);
    setBucket(null);
  };
  const changeMode = (next: PeriodMode) => {
    if (next === 'custom') {
      // The custom range starts from the year on screen, up to today
      const shown = yearPeriod(year);
      setFromDay(shown.fromDay);
      setToDay(Math.min(shown.toDay, today));
    }
    setMode(next);
    setBucket(null);
  };
  const changeRange = (range: { fromDay: number; toDay: number }) => {
    setFromDay(range.fromDay);
    setToDay(range.toDay);
    setBucket(null);
  };

  const periodLabel =
    mode === 'year'
      ? year === currentYear
        ? t('period.yearToDate', { year })
        : String(year)
      : t('period.range', { from: fmt.day(fromDay), to: fmt.day(toDay) });

  const head = (
    <PageHead
      eyebrow={[tc('nav.groupTools'), tc('nav.stocks')]}
      title={t('title')}
      description={t('subtitle')}
      actions={
        <PeriodControls
          mode={mode}
          onModeChange={changeMode}
          year={year}
          minYear={minYear}
          maxYear={currentYear}
          onYearChange={changeYear}
          fromDay={fromDay}
          toDay={toDay}
          minDay={yearPeriod(minYear).fromDay}
          maxDay={today}
          onRangeChange={changeRange}
        />
      }
    />
  );

  if (isError) {
    return (
      <>
        {head}
        <ReportError onRetry={() => void refetch()} />
      </>
    );
  }

  // Settings or the first response still loading: skeletons, never a page spinner
  if (!report) {
    return (
      <>
        {head}
        <ResultHeroSkeleton />
        <ReportStatsSkeleton />
      </>
    );
  }

  if (report.firstTransactionDay === null) {
    return (
      <>
        {head}
        <EmptyState
          icon={<Receipt />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button asChild>
              <Link href="/stocks">{t('empty.action')}</Link>
            </Button>
          }
        />
      </>
    );
  }

  return (
    <>
      {head}
      <ConversionQualityNote quality={report.fxQuality} currency={report.currency} />
      <ResultHero
        report={report}
        settings={settings}
        onSettingsChange={save}
        periodLabel={periodLabel}
        stale={isPlaceholderData}
      />
      <ReportStats stats={report.stats} currency={report.currency} stale={isPlaceholderData} />
      <div className="mb-[17px] grid grid-cols-2 items-stretch gap-[18px]">
        <ActivityCard
          buckets={report.buckets}
          currency={report.currency}
          bucket={bucket}
          onBucketChange={setBucket}
          stale={isPlaceholderData}
        />
        <TickerResultsCard
          tickers={report.tickers}
          currency={report.currency}
          testOn={settings.holdingTestEnabled}
          stale={isPlaceholderData}
        />
      </div>
      {settings.holdingTestEnabled && (
        <HoldingTestCard
          report={report}
          months={settings.holdingTestMonths}
          stale={isPlaceholderData}
        />
      )}
      <TransactionsCard
        report={report}
        testOn={settings.holdingTestEnabled}
        costMethod={settings.costMethod}
        period={period}
        bucket={bucket}
        onBucketChange={setBucket}
        stale={isPlaceholderData}
      />
    </>
  );
}
