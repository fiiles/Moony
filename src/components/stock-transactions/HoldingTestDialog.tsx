import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarClock } from 'lucide-react';
import type { StockReportOpenLot } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { openLotsByTicker } from '@/utils/stock-transactions-report';
import { cn } from '@/lib/utils';
import { AssetLogo } from '@/components/common/AssetLogo';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Segmented } from '@/components/ui/segmented';
import { holdingLength } from './holding-length';
import { lotName } from './holding-lot-helpers';
import { PassChip } from './holding-lot-parts';

type Tab = 'waiting' | 'passed';

const COLUMNS = 'grid-cols-[104px_minmax(0,1fr)_minmax(0,1.2fr)_112px_132px]';

interface Groups {
  waiting: StockReportOpenLot[];
  passed: StockReportOpenLot[];
}

/**
 * "Časový test otevřených pozic" (spec D12): every open lot, the ones still waiting and the ones
 * that have passed, grouped by position (owner, 2026-11-23): a head row per position with its
 * open quantity, value and unrealized result, then its lots with the day the sale passes (or
 * passed), the open quantity of the bought quantity, the purchase day and price. The positions
 * follow their nearest day. Plain
 * scrolling content: no Select or Popover in here (wheel scrolling over a portaled popover inside
 * a dialog is blocked in the WebKit webview).
 */
export function HoldingTestDialog({
  open,
  onOpenChange,
  groups,
  today,
  currency,
  months,
  share,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  groups: Groups;
  today: number;
  currency: string;
  months: number;
  /** The foot line about the share already passed; empty when no lot has a value. */
  share: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const { t: tc } = useTranslation('common');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[800px]">
        <DialogHeader>
          <DialogTitle>{t('upcoming.dialog.title')}</DialogTitle>
          <DialogDescription>
            {t('upcoming.dialog.sub', { length: holdingLength(t, months) })}
          </DialogDescription>
        </DialogHeader>
        {/* Its own component: the tab starts on "waiting" with every opening */}
        <LotsTable groups={groups} today={today} currency={currency} />
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="ghost">{tc('buttons.close')}</Button>
          </DialogClose>
          {share && <span className="text-micro font-500 text-ink-4">{share}</span>}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function LotsTable({
  groups,
  today,
  currency,
}: {
  groups: Groups;
  today: number;
  currency: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { money, signedMoney, nativeMoney } = useReportMoney(currency);
  const [tab, setTab] = useState<Tab>('waiting');
  const positions = openLotsByTicker(groups[tab]);

  const num = (value: string | number) => fmt.number(Number(value), { maximumFractionDigits: 6 });
  const signed = (value: number | null, strong: boolean) =>
    value === null ? (
      <span className="text-ink-4">—</span>
    ) : (
      <span
        className={cn(
          'whitespace-nowrap text-table num',
          strong ? 'font-650' : 'font-500',
          value > 0 && 'text-gain',
          value < 0 && 'text-loss'
        )}
      >
        {signedMoney(value)}
      </span>
    );

  return (
    <>
      <Segmented
        value={tab}
        onValueChange={setTab}
        options={[
          {
            value: 'waiting',
            label: t('upcoming.dialog.tabWaiting', { n: groups.waiting.length }),
          },
          { value: 'passed', label: t('upcoming.dialog.tabPassed', { n: groups.passed.length }) },
        ]}
        className="justify-self-start"
      />
      {positions.length === 0 ? (
        <EmptyState
          bare
          className="py-8"
          icon={<CalendarClock />}
          title={t(
            tab === 'waiting'
              ? 'upcoming.dialog.emptyWaitingTitle'
              : 'upcoming.dialog.emptyPassedTitle'
          )}
          description={t(
            tab === 'waiting'
              ? 'upcoming.dialog.emptyWaitingBody'
              : 'upcoming.dialog.emptyPassedBody'
          )}
        />
      ) : (
        <div className="max-h-[min(52vh,420px)] min-h-[160px] overflow-y-auto pr-3 [scrollbar-gutter:stable]">
          <div role="table" className="min-w-0">
            <div
              role="row"
              className={cn(
                'sticky top-0 z-[1] grid gap-x-3 border-b border-line-soft bg-paper px-2 pb-[7px] text-thead uppercase text-ink-3',
                COLUMNS
              )}
            >
              <span role="columnheader">
                {t(tab === 'waiting' ? 'upcoming.dialog.colPasses' : 'upcoming.dialog.colPassed')}
              </span>
              <span role="columnheader">{t('upcoming.dialog.colQuantity')}</span>
              <span role="columnheader">{t('upcoming.dialog.colBought')}</span>
              <span role="columnheader" className="text-right">
                {t('upcoming.dialog.colValue')}
              </span>
              <span role="columnheader" className="text-right">
                {t('upcoming.dialog.colUnrealized')}
              </span>
            </div>
            {positions.map((position) => (
              <div key={position.investmentId} role="rowgroup" className="mt-2">
                <div
                  role="row"
                  className={cn('grid items-center gap-x-3 rounded-r2 bg-well px-2 py-2', COLUMNS)}
                >
                  <span role="rowheader" className="col-span-3 flex min-w-0 items-center gap-2.5">
                    <AssetLogo ticker={position.ticker} type="stock" className="shrink-0" />
                    <span className="min-w-0">
                      <b className="block truncate text-table font-650 text-ink">
                        {lotName(position.lots[0])}
                      </b>
                      <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                        {position.ticker} ·{' '}
                        {t('upcoming.dialog.groupLots', { count: position.lots.length })} ·{' '}
                        {t('quantity', {
                          count: position.quantity,
                          value: num(position.quantity),
                        })}
                      </small>
                    </span>
                  </span>
                  <span
                    role="cell"
                    className="whitespace-nowrap text-right text-table font-650 num text-ink"
                  >
                    {position.value === null ? '—' : money(position.value)}
                  </span>
                  <span role="cell" className="text-right">
                    {signed(position.unrealized, true)}
                  </span>
                </div>
                {position.lots.map((lot, i) => {
                  const quantity = Number(lot.quantity);
                  const lotQuantity = Number(lot.lotQuantity);
                  return (
                    <div
                      key={`${lot.buyDay}-${lot.quantity}-${i}`}
                      role="row"
                      className={cn(
                        'grid items-center gap-x-3 border-b border-line-soft px-2 py-2 last:border-0',
                        COLUMNS
                      )}
                    >
                      <span role="cell">
                        {lot.passDay !== null && <PassChip passDay={lot.passDay} today={today} />}
                      </span>
                      <span role="cell" className="min-w-0 truncate text-table num text-ink-2">
                        {quantity < lotQuantity
                          ? t('upcoming.dialog.ofLot', {
                              count: lotQuantity,
                              open: num(lot.quantity),
                              lot: num(lot.lotQuantity),
                            })
                          : t('quantity', { count: quantity, value: num(lot.quantity) })}
                      </span>
                      <span role="cell" className="min-w-0 text-table text-ink-2">
                        {fmt.day(lot.buyDay)}
                        <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                          {t('upcoming.dialog.perShare', {
                            price: nativeMoney(Number(lot.buyPrice), lot.buyCurrency),
                          })}
                        </small>
                      </span>
                      <span
                        role="cell"
                        className="whitespace-nowrap text-right text-table num text-ink-2"
                      >
                        {lot.value === null ? '—' : money(Number(lot.value))}
                      </span>
                      <span role="cell" className="text-right">
                        {signed(lot.unrealized === null ? null : Number(lot.unrealized), false)}
                      </span>
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      )}
    </>
  );
}
