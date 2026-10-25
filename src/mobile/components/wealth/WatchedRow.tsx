import { useTranslation } from 'react-i18next';
import type { WatchedStockRow } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { dayChange } from '@/utils/stock-monitor';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { priceOf, watchPriceText, watchTargetState } from '@/mobile/lib/watch-detail';
import { watchPath } from '@/mobile/shell/routes';
import { ChangeLine } from './ChangeLine';
import { RowBadge } from '@/mobile/components/RowBadge';

/**
 * A watched stock (spec §8.8, prototype frame 11): the ticker with its name, the price in the
 * stock's own currency and the change since yesterday's close ("▲ 1,2 % dnes"). The target is a
 * badge after the ticker — "cíl 120 $" while it waits, "cíl dosažen" once the price has crossed
 * it — and nothing without a target. A tap opens the watch detail.
 *
 * The prices are Yahoo's public numbers, native and never converted: unlike the person's amounts
 * they stay visible while amounts are hidden (the one exception to the eye).
 */
export function WatchedRow({ row }: { row: WatchedStockRow }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const currency = row.currency ?? '';

  const current = priceOf(row.currentPrice);
  const target = priceOf(row.targetPrice);
  const day = dayChange(row.currentPrice, row.previousClose);
  const targetState = watchTargetState(current, target, row.targetDirection);
  const targetText =
    target === null || targetState === 'none'
      ? null
      : targetState === 'reached'
        ? t('wealth.stocks.targetReached')
        : t('wealth.stocks.target', { price: watchPriceText(fmt, target, currency) });

  return (
    <EntityRow
      chip={<RowChip>{row.ticker.slice(0, 2).toUpperCase()}</RowChip>}
      title={
        <>
          {row.ticker}
          {targetText !== null && <RowBadge>{targetText}</RowBadge>}
        </>
      }
      sub={row.shortName ?? row.longName ?? ''}
      value={current === null ? '—' : watchPriceText(fmt, current, currency)}
      valueSub={
        day !== null ? (
          <ChangeLine
            change={{ delta: day.abs, ratio: day.pct / 100 }}
            currency={currency}
            trail={t('wealth.stocks.dayChange')}
          />
        ) : undefined
      }
      href={watchPath(row.ticker)}
    />
  );
}
