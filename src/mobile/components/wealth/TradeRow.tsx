import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Pencil, TrendingDown, TrendingUp, Trash2 } from 'lucide-react';
import { priceDecimals } from '@shared/currencies';
import type { InvestmentTransaction } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { nativeTotal } from '@/mobile/lib/position-series';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

interface TradeRowProps {
  tx: InvestmentTransaction;
  /** The report currency the value is converted into. */
  currency: string;
  /** Converts at the rates of the trade's own day; `null` without a rate and until `ratesReady`. */
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
  /** The row a tap on its mark on the chart points at: it takes `well` and fades back. */
  hot: boolean;
  onEdit: (tx: InvestmentTransaction) => void;
  onDelete: (tx: InvestmentTransaction) => void;
}

/**
 * A buy or a sell of the position (spec §8.8, prototype frame 12): the entity row with a ▲ / ▽
 * chip (green for a sell), "Nákup · 12 ks" over the day and the price in the trade's currency, and
 * the total at the rates of the trade's day — "− 32 750 Kč" for a buy, "+ 12 880 Kč" for a sell —
 * over the native total. Without a rate for that day the native total stands in the value and
 * the sub-line says so. A tap or a long press opens the row's menu (Upravit, Smazat), anchored
 * like the agenda row's.
 */
export function TradeRow({
  tx,
  currency,
  convertAt,
  ratesReady,
  hot,
  onEdit,
  onDelete,
}: TradeRowProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [menuOpen, setMenuOpen] = useState(false);

  const sell = tx.type === 'sell';
  const quantity = parseFloat(tx.quantity) || 0;
  const price = parseFloat(tx.pricePerUnit) || 0;
  const txCurrency = tx.currency || currency;
  const total = nativeTotal(tx);
  const converted = convertAt(total, txCurrency, currency, tx.transactionDate);
  const sign = sell ? '+ ' : '− ';
  const Glyph = sell ? TrendingDown : TrendingUp;

  return (
    <EntityRow
      id={`tx-${tx.id}`}
      // The tint goes on at once and fades back over 1.2 s when the row stops being hot
      className={cn(
        'transition-colors ease-out motion-reduce:transition-none',
        hot ? 'bg-well duration-0' : 'duration-[1200ms]'
      )}
      chip={
        <RowChip tone={sell ? 'gain' : 'default'}>
          <Glyph strokeWidth={1.75} />
        </RowChip>
      }
      title={t(`wealth.position.tx.${sell ? 'sell' : 'buy'}`, {
        quantity: fmt.number(quantity, { maximumFractionDigits: 4 }),
        unit: ts('table.unit'),
      })}
      sub={`${fmt.day(tx.transactionDate)} · ${moneyText(price, txCurrency, {
        decimals: priceDecimals(price, txCurrency),
      })}`}
      value={
        converted !== null ? (
          <Money value={converted} currency={currency} options={{ decimals: 0 }} sign={sign} />
        ) : (
          <Money value={total} currency={txCurrency} options={{ decimals: 0 }} sign={sign} />
        )
      }
      valueTone={sell ? 'gain' : undefined}
      valueSub={
        converted === null && ratesReady
          ? t('wealth.noRate', { currency: txCurrency })
          : moneyText(total, txCurrency, { decimals: 2 })
      }
      onClick={() => setMenuOpen(true)}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={t('wealth.position.tx.menu')}
              className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
            >
              <Ellipsis aria-hidden className="size-5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[220px]">
            <DropdownMenuItem className={MENU_ITEM} onSelect={() => onEdit(tx)}>
              <Pencil strokeWidth={1.75} aria-hidden />
              {tc('buttons.edit')}
            </DropdownMenuItem>
            <DropdownMenuItem variant="danger" className={MENU_ITEM} onSelect={() => onDelete(tx)}>
              <Trash2 strokeWidth={1.75} aria-hidden />
              {ts('detail.deleteTransaction')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    />
  );
}
