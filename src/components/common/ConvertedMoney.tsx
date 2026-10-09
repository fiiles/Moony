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
  /** The rates are still loading: show the native amount without the missing marker. */
  pending?: boolean;
  className?: string;
}

/**
 * A converted amount in the main currency or, without a rate, the native amount with
 * the missing marker the reports use (loss colour, the reason in the tooltip).
 */
export function ConvertedMoney({
  value,
  amount,
  currency,
  options = { decimals: 0 },
  pending = false,
  className,
}: ConvertedMoneyProps) {
  const { t } = useTranslation('common');
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  if (value !== null) {
    return <span className={className}>{fmt.money(value, currencyCode, options)}</span>;
  }
  const native = fmt.money(amount, currency, options);
  if (pending) return <span className={className}>{native}</span>;
  return (
    <span
      className={cn(className, 'text-loss')}
      title={t('fxQuality.missing', { currency: currencyCode, currencies: currency })}
    >
      {native}
    </span>
  );
}
