import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Receipt } from 'lucide-react';
import type { StockReportTickerResult } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { AssetLogo } from '@/components/common/AssetLogo';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';

const VISIBLE = 6;

/**
 * "Výsledek podle titulu" (spec D11): one row per sold ticker, sorted from the largest gain to
 * the largest loss as the report delivers them. The bars share one zero line (series tokens, never
 * the gain and loss fills); the amount is signed and coloured. With the holding test on a sub-line
 * tells whether the result is exempt, taxable or split. Six rows, the rest behind a text button.
 */
export function TickerResultsCard({
  tickers,
  currency,
  testOn,
  stale,
}: {
  tickers: StockReportTickerResult[];
  currency: string;
  testOn: boolean;
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { signedMoney } = useReportMoney(currency);
  const [all, setAll] = useState(false);

  const results = tickers.map((row) => Number(row.resultAfterFees));
  const maxGain = Math.max(0, ...results);
  const maxLoss = Math.max(0, ...results.map((r) => -r));
  const span = maxGain + maxLoss;
  const zero = span > 0 ? (maxLoss / span) * 100 : 0;

  const shown = all ? tickers : tickers.slice(0, VISIBLE);

  const quantity = (text: string) => {
    const value = Number(text);
    return t('quantity', { count: value, value: fmt.number(value, { maximumFractionDigits: 6 }) });
  };

  const testLine = (row: StockReportTickerResult) => {
    if (row.taxableResult === null || row.exemptResult === null) return null;
    const taxable = Number(row.taxableResult);
    const exempt = Number(row.exemptResult);
    if (taxable === 0 && exempt === 0) return null;
    if (exempt === 0) return t('tickerResults.allTaxable');
    if (taxable === 0) return t('tickerResults.allExempt');
    return t('tickerResults.taxablePart', { amount: signedMoney(taxable) });
  };

  return (
    <Card className="flex min-w-0 flex-col">
      <CardHeader>
        <div className="min-w-0">
          <CardTitle>{t('tickerResults.title')}</CardTitle>
          {tickers.length > 0 && <CardDescription>{t('tickerResults.sub')}</CardDescription>}
        </div>
      </CardHeader>
      <CardContent className={cn('flex-1 transition-opacity duration-base', stale && 'opacity-50')}>
        {tickers.length === 0 ? (
          <EmptyState
            bare
            className="py-8"
            icon={<Receipt />}
            title={t('tickerResults.emptyTitle')}
            description={t('tickerResults.emptyBody')}
          />
        ) : (
          <ul className="m-0 list-none p-0">
            {shown.map((row, i) => {
              const result = results[i];
              const width = span > 0 ? (Math.abs(result) / span) * 100 : 0;
              return (
                <li
                  key={row.investmentId}
                  className="grid grid-cols-[minmax(0,1.3fr)_minmax(48px,1fr)_auto] items-center gap-x-3 border-b border-line-soft py-2.5 last:border-0"
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <AssetLogo ticker={row.ticker} type="stock" className="shrink-0" />
                    <div className="min-w-0">
                      <b className="block truncate text-table font-650 text-ink">
                        {row.companyName || row.ticker}
                      </b>
                      <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                        {row.ticker} · {quantity(row.quantity)}
                      </small>
                    </div>
                  </div>
                  <div aria-hidden className="relative h-[7px] min-w-0 rounded-full bg-well">
                    <i
                      className="absolute inset-y-[-3px] w-px bg-line-strong"
                      style={{ left: `${zero}%` }}
                    />
                    <i
                      className={cn(
                        'absolute inset-y-0 rounded-full',
                        result < 0 ? 'bg-s3' : 'bg-s1'
                      )}
                      style={
                        result < 0
                          ? { right: `${100 - zero}%`, width: `${width}%` }
                          : { left: `${zero}%`, width: `${width}%` }
                      }
                    />
                  </div>
                  <div className="text-right">
                    <span
                      className={cn(
                        'block whitespace-nowrap text-table font-650 num',
                        result > 0 && 'text-gain',
                        result < 0 && 'text-loss'
                      )}
                    >
                      {signedMoney(result)}
                    </span>
                    {testOn && testLine(row) && (
                      <small className="mt-[3px] block whitespace-nowrap text-micro font-500 text-ink-4">
                        {testLine(row)}
                      </small>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        {tickers.length > VISIBLE && (
          <div className="pt-2.5">
            <Button
              variant="link"
              size="sm"
              className="text-micro"
              onClick={() => setAll((v) => !v)}
            >
              {all
                ? t('tickerResults.showFewer', { n: VISIBLE })
                : t('tickerResults.showAll', { count: tickers.length })}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
