import { useTranslation } from 'react-i18next';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import type { MoneyOptions } from '@/lib/format';
import { cn } from '@/lib/utils';

interface ConvertedMoneyProps {
  /** The amount in the main (profile) currency; null when it could not be converted. */
  value: number | null;
  /** The native amount and its currency, shown when `value` is null. */
  amount: number;
  currency: string;
  /** Formatting of both figures (default: 0 decimals). */
  options?: MoneyOptions;
  /**
   * Dated rates are still loading: show the native amount without the missing marker
   * (always the case while the current quotes load).
   */
  pending?: boolean;
  /** Text before the amount (a sign), styled with it. */
  prefix?: string;
  className?: string;
}

/**
 * A converted amount in the main currency or, without a rate, the native amount with
 * the missing marker the reports use (loss colour, the reason in the tooltip). While the
 * rates load the native amount shows plainly (pending, not missing).
 */
export function ConvertedMoney({
  value,
  amount,
  currency,
  options = { decimals: 0 },
  pending = false,
  prefix = '',
  className,
}: ConvertedMoneyProps) {
  const { t } = useTranslation('common');
  const { currencyCode, fxReady } = useCurrency();
  const fmt = useFormat();
  if (value !== null) {
    return (
      <span className={className}>
        {prefix}
        {fmt.money(value, currencyCode, options)}
      </span>
    );
  }
  const native = `${prefix}${fmt.money(amount, currency, options)}`;
  // A null while the quotes load is pending, not missing
  if (pending || !fxReady) return <span className={className}>{native}</span>;
  return (
    <span
      className={cn(className, 'text-loss')}
      title={t('fxQuality.missing', { currency: currencyCode, currencies: currency })}
    >
      {native}
    </span>
  );
}
