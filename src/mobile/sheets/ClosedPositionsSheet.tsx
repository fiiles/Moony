import { useMemo } from 'react';
import { InstrumentChip } from '@/mobile/components/wealth/InstrumentChip';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { useFormat } from '@/lib/use-format';
import type { PositionRowBase } from '@/utils/positions';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { Sheet } from '@/mobile/components/Sheet';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { closedPositions, lastSellDays, type PositionTrade } from '@/mobile/lib/closed-positions';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { displayName } from '@/mobile/lib/position-rows';
import { statTone } from '@/mobile/lib/stat-money';

interface ClosedPositionsSheetProps<
  R extends PositionRowBase & { companyName?: string; name?: string },
> {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Every position of the class: the closed ones are picked out here. */
  rows: readonly R[];
  /** Which class the positions are: it decides where the chip's logo comes from. */
  kind: 'stock' | 'crypto';
  /** Every trade of the class: the last sell of a position is the day it was closed. */
  trades: readonly PositionTrade[];
  /** The report currency: the realized result is the backend's, converted into it. */
  currency: string;
  /** Where a tap on a position goes. */
  hrefOf: (row: R) => string;
}

/**
 * "Uzavřené pozice" from the `···` of Akcie and Krypto (spec §8.8, ruling 11): the positions that
 * hold nothing any more, the newest close first — the ticker over the name, the realized result
 * signed (green or red; "—" when the backend could not convert it) over the day of
 * the last sell, none when no sell is known. A tap closes the sheet and opens the position, once
 * the sheet's history entry is off the stack (`leaveAfterSheet`).
 */
export function ClosedPositionsSheet<
  R extends PositionRowBase & { companyName?: string; name?: string },
>({ open, onOpenChange, rows, kind, trades, currency, hrefOf }: ClosedPositionsSheetProps<R>) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [, setLocation] = useLocation();

  const sellDays = useMemo(() => lastSellDays(trades), [trades]);
  const closed = useMemo(
    () => closedPositions(rows, (id) => sellDays.get(id) ?? null),
    [rows, sellDays]
  );

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('wealth.stocks.menu.closed')}
      description={closed.length === 0 ? t('wealth.closed.empty') : undefined}
    >
      {closed.length > 0 && (
        <EntityRows className="mb-0">
          {closed.map(({ row, closedAt }) => (
            <EntityRow
              key={row.id}
              chip={<InstrumentChip ticker={row.ticker} type={kind} />}
              title={row.ticker}
              sub={displayName(row)}
              value={
                row.realizedGain === null
                  ? '—'
                  : moneyText(row.realizedGain, currency, { signed: true, decimals: 0 })
              }
              valueTone={statTone(row.realizedGain)}
              valueSub={
                closedAt === null
                  ? undefined
                  : t('wealth.closed.on', { date: fmt.day(closedAt, shortDayOptions(closedAt)) })
              }
              onClick={() =>
                leaveAfterSheet(
                  () => onOpenChange(false),
                  () => setLocation(hrefOf(row))
                )
              }
            />
          ))}
        </EntityRows>
      )}
    </Sheet>
  );
}
