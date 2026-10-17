import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { WatchedStockRow } from '@shared/schema';
import { priceDecimals } from '@shared/currencies';
import { dayChange, targetReached } from '@/utils/stock-monitor';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { ChangeLine } from './ChangeLine';

/** A price string of the wire as a number; `null` when it is missing or not one. */
const priceOf = (value: string | null) => {
  const n = parseFloat(value ?? '');
  return Number.isFinite(n) ? n : null;
};

/**
 * A watched stock (spec §8.8, prototype frame 11): the ticker with its name, the price in the
 * stock's own currency, the change since yesterday's close ("▲ 1,2 % dnes") and, when there is a
 * target, where it stands ("cíl 120 $", "cíl dosažen"). The prices are native and never
 * converted; they go through the eye like every amount. A tap will open the watch detail (a
 * later part of Phase 4): for now it says so.
 */
export function WatchedRow({ row }: { row: WatchedStockRow }) {
  const { t } = useTranslation('mobile');
  const moneyText = useMoneyText();
  const currency = row.currency ?? '';
  const price = (value: number) =>
    moneyText(value, currency, { decimals: priceDecimals(value, currency) });

  const current = priceOf(row.currentPrice);
  const target = priceOf(row.targetPrice);
  const day = dayChange(row.currentPrice, row.previousClose);
  const targetText =
    target === null
      ? null
      : targetReached(current, target, row.targetDirection)
        ? t('wealth.stocks.targetReached')
        : t('wealth.stocks.target', { price: price(target) });

  return (
    <EntityRow
      chip={<RowChip>{row.ticker.slice(0, 2).toUpperCase()}</RowChip>}
      title={row.ticker}
      sub={row.shortName ?? row.longName ?? ''}
      value={current === null ? '—' : price(current)}
      valueSub={
        day !== null || targetText !== null ? (
          <>
            <ChangeLine
              change={day === null ? null : { delta: day.abs, ratio: day.pct / 100 }}
              currency={currency}
              trail={t('wealth.stocks.dayChange')}
            />
            {day !== null && targetText !== null && ' · '}
            {targetText}
          </>
        ) : undefined
      }
      onClick={() => toast(t('wealth.placeholder.watchDetail'))}
    />
  );
}
