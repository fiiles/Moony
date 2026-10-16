import type { MoneyOptions } from '@/lib/format';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

interface MoneyProps {
  value: number;
  /** The currency the value is in (the report currency, or a record's own). */
  currency: string;
  /** Formatting: decimals, `signed`, `compact` (see `fmt.money`). */
  options?: MoneyOptions;
  /** Text before the amount ("+ ", "− "); it stays when the amount is masked. */
  sign?: string;
  className?: string;
}

/**
 * An amount on the phone: formatted by `useFormat().money` and shown as a mask ("•••• Kč") while
 * amounts are hidden (the eye on Přehled). Every money element of the phone shell renders through
 * it, so the one switch reaches them all.
 */
export function Money({ value, currency, options, sign = '', className }: MoneyProps) {
  const money = useMoneyText();
  return (
    <span className={className}>
      {sign}
      {money(value, currency, options)}
    </span>
  );
}
