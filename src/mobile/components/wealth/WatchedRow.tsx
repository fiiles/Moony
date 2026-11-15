import { useTranslation } from 'react-i18next';
import { InstrumentChip } from './InstrumentChip';
import { Plus, Target } from 'lucide-react';
import type { WatchedStockRow } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { dayChange } from '@/utils/stock-monitor';
import { EntityRow } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useOpenWithIntent } from '@/mobile/hooks/use-row-intent';
import { priceOf, watchPriceText, watchTargetState } from '@/mobile/lib/watch-detail';
import { watchPath } from '@/mobile/shell/routes';
import { ChangeLine } from './ChangeLine';
import { RowBadge } from '@/mobile/components/RowBadge';

/**
 * A watched stock (spec §8.8, prototype frame 11): the ticker with its name, the price in the
 * stock's own currency and the change since yesterday's close ("▲ 1,2 % dnes"). The target is a
 * badge after the ticker — "cíl 120 $" while it waits, "cíl dosažen" once the price has crossed
 * it — and nothing without a target. A tap opens the watch detail; the long press's menu opens it
 * with the target sheet or the buy dialog (README §13, Gestures).
 *
 * The prices are Yahoo's public numbers, native and never converted.
 */
export function WatchedRow({ row }: { row: WatchedStockRow }) {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const openWith = useOpenWithIntent();
  const path = watchPath(row.ticker);
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
      chip={<InstrumentChip ticker={row.ticker} type="stock" />}
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
      href={path}
      menu={{
        label: tc('mobile:labelValue', { label: tc('labels.actions'), value: row.ticker }),
        children: (
          <>
            <RowMenuItem onSelect={() => openWith(path, 'target')}>
              <Target strokeWidth={1.75} aria-hidden />
              {tm(target === null ? 'actions.setTarget' : 'actions.editTarget')}
            </RowMenuItem>
            <RowMenuItem onSelect={() => openWith(path, 'buy')}>
              <Plus strokeWidth={1.75} aria-hidden />
              {tc('buttons.buy')}
            </RowMenuItem>
          </>
        ),
      }}
    />
  );
}
