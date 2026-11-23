import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockTransactionsReport } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { openLotGroups } from '@/utils/stock-transactions-report';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { HoldingTestDialog } from './HoldingTestDialog';
import { holdingLength } from './holding-length';
import { lotName, useShareLine } from './holding-lot-helpers';
import { PassChip, UnrealizedCell } from './holding-lot-parts';

const ROWS = 4;

/**
 * "Blíží se časový test" (spec D12, holding test on only): the four nearest open lots that have
 * not passed, as agenda rows (a date chip, name and open quantity, purchase day, unrealized
 * result), a foot line with the share already passed, and a button to the dialog with every lot.
 */
export function HoldingTestCard({
  report,
  months,
  stale,
}: {
  report: StockTransactionsReport;
  months: number;
  stale: boolean;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const [open, setOpen] = useState(false);
  const { currency, today } = report;

  const groups = openLotGroups(report.openLots, today);
  const share = useShareLine(groups, currency);
  const nearest = groups.waiting.slice(0, ROWS);
  const later = groups.waiting.length - nearest.length;

  const quantity = (text: string) => {
    const value = Number(text);
    return t('quantity', { count: value, value: fmt.number(value, { maximumFractionDigits: 6 }) });
  };

  return (
    <Card variant="flat" className="mb-[17px] min-w-0">
      <CardHeader>
        <div className="min-w-0">
          <CardTitle>{t('upcoming.title')}</CardTitle>
          <CardDescription>
            {t('upcoming.sub', { length: holdingLength(t, months) })}
          </CardDescription>
        </div>
        <Button variant="outline" size="sm" className="shrink-0" onClick={() => setOpen(true)}>
          {groups.waiting.length > ROWS
            ? t('upcoming.showAll', { n: groups.waiting.length })
            : t('upcoming.showDetail')}
        </Button>
      </CardHeader>
      <CardContent className={cn('transition-opacity duration-base', stale && 'opacity-50')}>
        {nearest.length > 0 ? (
          <ul className="m-0 grid list-none grid-cols-2 gap-x-7 p-0 [&>li:nth-last-child(2):nth-child(odd)]:border-0">
            {nearest.map((lot, i) => (
              <li
                key={`${lot.investmentId}-${lot.buyDay}-${lot.quantity}-${i}`}
                className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 border-b border-line-soft py-2.5 last:border-0"
              >
                {lot.passDay !== null && <PassChip passDay={lot.passDay} today={today} compact />}
                <span className="min-w-0">
                  <b className="block truncate text-table font-650 text-ink">
                    {lotName(lot)} · {quantity(lot.quantity)}
                  </b>
                  <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                    {t('upcoming.boughtOn', { date: fmt.day(lot.buyDay) })}
                  </small>
                </span>
                <span className="text-right">
                  <UnrealizedCell lot={lot} currency={currency} />
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="my-1.5 text-table text-ink-3">{t('upcoming.none')}</p>
        )}
        <p className="mb-0 mt-2.5 text-micro font-500 text-ink-4">
          {later > 0 && `${t('upcoming.footMore', { count: later })} `}
          {share}
        </p>
      </CardContent>
      <HoldingTestDialog
        open={open}
        onOpenChange={setOpen}
        groups={groups}
        today={today}
        currency={currency}
        months={months}
        share={share}
      />
    </Card>
  );
}
