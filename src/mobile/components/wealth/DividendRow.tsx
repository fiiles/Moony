import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Coins, Ellipsis, Pencil, Trash2 } from 'lucide-react';
import { priceDecimals } from '@shared/currencies';
import type { StockDividend } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

interface DividendRowProps {
  dividend: StockDividend;
  /** The report currency the gross is converted into. */
  currency: string;
  /** Converts at the rates of the ex-date; `null` without a rate and until `ratesReady`. */
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
  /** The row a tap on the chart points at: it takes `well` and fades back. */
  hot: boolean;
  /** Without it the menu of a manual dividend has only Smazat (the page owns the edit sheet). */
  onEdit?: (dividend: StockDividend) => void;
  onDelete: (dividend: StockDividend) => void;
}

/**
 * A received dividend of a stock position (spec §A3): a `Coins` chip in the gain tone, "Dividenda"
 * over the ex-date and what the payout was (per share for Yahoo's, the note for a manual one),
 * and the gross "+ X" at the rates of the ex-date over the native gross. A Yahoo dividend has no
 * menu and no tap (it cannot be changed); a manual one opens Upravit (not when superseded) and
 * Smazat, anchored like `TradeRow`'s. A superseded one is dimmed with the value struck through.
 */
export function DividendRow({
  dividend,
  currency,
  convertAt,
  ratesReady,
  hot,
  onEdit,
  onDelete,
}: DividendRowProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [menuOpen, setMenuOpen] = useState(false);

  const isYahoo = dividend.source === 'yahoo';
  const gross = Number(dividend.grossAmount) || 0;
  const converted = convertAt(gross, dividend.currency, currency, dividend.exDate);
  const perShare = Number(dividend.amountPerShare);
  const detail = dividend.superseded
    ? ts('detail.dividendRecords.superseded')
    : isYahoo
      ? ts('detail.dividendRecords.perShare', {
          amount: moneyText(perShare, dividend.currency, {
            decimals: priceDecimals(perShare, dividend.currency),
          }),
        })
      : dividend.note;
  const struck = dividend.superseded ? 'line-through' : undefined;
  const menu = !isYahoo;

  return (
    <EntityRow
      id={`tx-${dividend.id}`}
      className={cn(
        'transition-colors ease-out motion-reduce:transition-none',
        hot ? 'bg-well duration-0' : '[transition-duration:1200ms]',
        dividend.superseded && 'opacity-60'
      )}
      chip={
        <RowChip tone="gain">
          <Coins strokeWidth={1.75} />
        </RowChip>
      }
      title={ts('detail.dividendRecords.badge')}
      sub={detail ? `${fmt.day(dividend.exDate)} · ${detail}` : fmt.day(dividend.exDate)}
      value={
        <span className={struck}>
          {converted !== null ? (
            <Money value={converted} currency={currency} options={{ decimals: 0 }} sign="+ " />
          ) : (
            <Money value={gross} currency={dividend.currency} options={{ decimals: 0 }} sign="+ " />
          )}
        </span>
      }
      valueTone="gain"
      valueSub={
        converted === null && ratesReady
          ? t('wealth.noRate', { currency: dividend.currency })
          : isYahoo
            ? ts('detail.dividendRecords.sourceYahoo')
            : moneyText(gross, dividend.currency, { decimals: 2 })
      }
      onClick={menu ? () => setMenuOpen(true) : undefined}
      onLongPress={menu ? () => setMenuOpen(true) : undefined}
      accessory={
        menu ? (
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label={t('wealth.position.dividend.menu')}
                className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
              >
                <Ellipsis aria-hidden className="size-5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              {onEdit && !dividend.superseded && (
                <RowMenuItem onSelect={() => onEdit(dividend)}>
                  <Pencil strokeWidth={1.75} aria-hidden />
                  {tc('buttons.edit')}
                </RowMenuItem>
              )}
              <RowMenuItem variant="danger" onSelect={() => onDelete(dividend)}>
                <Trash2 strokeWidth={1.75} aria-hidden />
                {tc('buttons.delete')}
              </RowMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        ) : undefined
      }
    />
  );
}
