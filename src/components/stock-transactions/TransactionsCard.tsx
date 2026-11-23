import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Download, Loader2, Search } from 'lucide-react';
import { toast } from 'sonner';
import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from '@tauri-apps/plugin-fs';
import type {
  StockReportBucket,
  StockReportCostMethod,
  StockTransactionsReport,
} from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  ledgerRows,
  saleViews,
  type LedgerKind,
  type SaleClass,
} from '@/utils/stock-transactions-report';
import { ledgerCsv, ledgerCsvFilename } from '@/utils/stock-transactions-csv';
import { FilterBadge } from '@/components/common/FilterBadge';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import { Segmented } from '@/components/ui/segmented';
import { LedgerTable } from './LedgerTable';
import { SaleResultsTable } from './SaleResultsTable';

type View = 'sales' | 'ledger';

/**
 * The table card (spec D13, D14, D16): "Výsledek prodejů" (one row per sale, unfolding to its
 * purchases) and "Všechny transakce" (every buy, sale and dividend, exportable as CSV). Search,
 * the class filter and the kind filter are the card's own; the chart bucket belongs to the page,
 * which the filter badge shows and clears. The export saves exactly the ledger rows shown.
 */
export function TransactionsCard({
  report,
  testOn,
  costMethod,
  period,
  bucket,
  onBucketChange,
  stale,
}: {
  report: StockTransactionsReport;
  testOn: boolean;
  costMethod: StockReportCostMethod;
  period: { fromDay: number; toDay: number };
  bucket: StockReportBucket | null;
  onBucketChange: (next: StockReportBucket | null) => void;
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [view, setView] = useState<View>('sales');
  const [search, setSearch] = useState('');
  const [saleClass, setSaleClass] = useState<SaleClass>('all');
  const [kind, setKind] = useState<LedgerKind>('all');
  const [exporting, setExporting] = useState(false);

  // Switching the holding test off drops the class filter with it
  const [testWasOn, setTestWasOn] = useState(testOn);
  if (testWasOn !== testOn) {
    setTestWasOn(testOn);
    if (!testOn) setSaleClass('all');
  }
  const shownClass: SaleClass = testOn ? saleClass : 'all';

  const views = useMemo(
    () => saleViews(report.sales, { search, bucket, saleClass: shownClass }),
    [report.sales, search, bucket, shownClass]
  );
  const rows = useMemo(
    () => ledgerRows(report.ledger, { search, bucket, kind }),
    [report.ledger, search, bucket, kind]
  );

  const bucketLabel =
    bucket === null
      ? null
      : bucket.key.length === 4
        ? bucket.key
        : fmt.monthInSentence(new Date(bucket.fromDay * 1000), { month: 'long', year: 'numeric' });

  const exportCsv = async () => {
    // The file is named after what the table covers: the bucket (inside the period) when one narrows it
    const fromDay = Math.max(period.fromDay, bucket?.fromDay ?? period.fromDay);
    const toDay = Math.min(period.toDay, bucket?.toDay ?? period.toDay);
    try {
      setExporting(true);
      const path = await save({
        defaultPath: ledgerCsvFilename(fromDay, toDay),
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (!path) return;
      // UTF-8 BOM so spreadsheet apps detect the encoding
      await writeTextFile(path, '﻿' + ledgerCsv(rows, report.currency));
      toast(t('table.exported'), {
        description: tc('export.successDescription', { count: rows.length }),
      });
    } catch (error) {
      console.error('Export failed:', error);
      toast.error(tc('export.failed'), { description: String(error) });
    } finally {
      setExporting(false);
    }
  };

  const sales = view === 'sales';
  const uncovered = report.uncoveredTickers;

  return (
    <Card variant="table" className="mb-[17px] min-w-0">
      <CardHeader>
        <div className="min-w-0">
          <CardTitle>{t(sales ? 'sales.title' : 'ledger.title')}</CardTitle>
          <CardDescription>
            {sales ? t('sales.sub') : t('ledger.sub', { currency: report.currency })}
          </CardDescription>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <InputWrap icon={<Search />} className="w-60 max-[1239px]:w-[170px]">
            <Input
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              aria-label={t('table.search')}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </InputWrap>
          <Segmented
            value={view}
            onValueChange={setView}
            aria-label={t('table.view')}
            options={[
              { value: 'sales', label: t('table.viewSales') },
              { value: 'ledger', label: t('table.viewLedger') },
            ]}
          />
        </div>
      </CardHeader>

      {((sales && testOn) || !sales || bucketLabel !== null) && (
        <div className="flex min-h-[44px] flex-wrap items-center gap-3 px-[21px] pb-3.5">
          {sales && testOn && (
            <Segmented
              value={saleClass}
              onValueChange={setSaleClass}
              aria-label={t('table.classFilter')}
              options={[
                { value: 'all', label: t('table.classAll') },
                { value: 'taxable', label: t('table.classTaxable') },
                { value: 'exempt', label: t('table.classExempt') },
              ]}
            />
          )}
          {!sales && (
            <Segmented
              value={kind}
              onValueChange={setKind}
              aria-label={t('table.kindFilter')}
              options={[
                { value: 'all', label: t('table.kindAll') },
                { value: 'buy', label: t('table.kindBuy') },
                { value: 'sell', label: t('table.kindSell') },
                { value: 'dividend', label: t('table.kindDividend') },
              ]}
            />
          )}
          {bucketLabel !== null && (
            <FilterBadge
              labels={[bucketLabel]}
              title={t('table.bucketTitle')}
              onClear={() => onBucketChange(null)}
            />
          )}
          {!sales && (
            <Button
              variant="outline"
              size="sm"
              className="ml-auto"
              title={t('table.exportTitle')}
              disabled={rows.length === 0 || exporting}
              aria-busy={exporting || undefined}
              onClick={() => void exportCsv()}
            >
              {exporting ? <Loader2 className="animate-spin" /> : <Download />}
              {t('table.export')}
            </Button>
          )}
        </div>
      )}

      <div className={cn('transition-opacity duration-base', stale && 'opacity-50')}>
        {sales && uncovered.length > 0 && (
          <div className="px-[21px] pb-3.5">
            <Alert className="py-2.5">
              <AlertDescription>
                {t('table.uncovered', { count: uncovered.length, tickers: uncovered.join(', ') })}
              </AlertDescription>
            </Alert>
          </div>
        )}
        {sales ? (
          <SaleResultsTable
            views={views}
            saleClass={shownClass}
            testOn={testOn}
            costMethod={costMethod}
            currency={report.currency}
          />
        ) : (
          <LedgerTable rows={rows} currency={report.currency} />
        )}
      </div>
    </Card>
  );
}
