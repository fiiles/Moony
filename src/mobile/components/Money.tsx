import type { MoneyOptions } from '@/lib/format';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

interface MoneyProps {
  value: number;
  /** The currency the value is in (the report currency, or a record's own). */
  currency: string;
  /** Formatting: decimals, `signed`, `compact` (see `fmt.money`). */
  options?: MoneyOptions;
  /** Text before the amount ("+ ", "− "). */
  sign?: string;
  className?: string;
}

/** An amount on the phone as an element: formatted by `useFormat().money`. */
export function Money({ value, currency, options, sign = '', className }: MoneyProps) {
  const money = useMoneyText();
  return (
    <span className={className}>
      {sign}
      {money(value, currency, options)}
    </span>
  );
}
