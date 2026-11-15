import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Coins, Pencil, Trash2 } from 'lucide-react';
import { priceDecimals } from '@shared/currencies';
import type { StockDividend } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { useApprox } from '@/mobile/hooks/use-approx';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { exactMoneyOptions } from '@/mobile/lib/money-rule';

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
 * and the gross "+ X" in the dividend's own currency, exactly, over its "≈" conversion at the
 * rates of the ex-date (A6). That Yahoo is the source is said once, under the list (D27). A Yahoo dividend has no
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
  const approx = useApprox();
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
          <Money
            value={gross}
            currency={dividend.currency}
            options={exactMoneyOptions(gross, dividend.currency)}
            sign="+ "
          />
        </span>
      }
      valueTone="gain"
      valueSub={approx(gross, dividend.currency, { converted, pending: !ratesReady })}
      onClick={menu ? () => setMenuOpen(true) : undefined}
      menu={
        menu
          ? {
              label: t('wealth.position.dividend.menu'),
              // The row's tap opens the menu; the visible `···` says it has one
              shown: true,
              open: menuOpen,
              onOpenChange: setMenuOpen,
              children: (
                <>
                  {onEdit && !dividend.superseded && (
                    <RowMenuItem onSelect={() => onEdit(dividend)}>
                      <Pencil strokeWidth={1.75} aria-hidden />
                      {tc('buttons.edit')}
                    </RowMenuItem>
                  )}
                  <RowMenuItem variant="danger" onSelect={() => onDelete(dividend)}>
                    <Trash2 strokeWidth={1.75} aria-hidden />
                    {ts('detail.dividendRecords.confirmDelete.confirm')}
                  </RowMenuItem>
                </>
              ),
            }
          : undefined
      }
    />
  );
}
