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
 * An amount on the phone as an element: formatted by `useFormat().money` and shown as a mask
 * ("•••• Kč") while amounts are hidden (the eye on Přehled). One of four routes to the same switch,
 * by what the place needs:
 * - `<Money>`: where an element will do (the hero's change line, the stat notes);
 * - `useMoneyText()`: where a plain string is needed (the hero and stat values, which size
 *   themselves to the shown text, the allocation ring's accessible name) and as the formatter handed
 *   to shared hooks (the chart's jump callout, the agenda sub-line);
 * - `ConvertedMoney` with its `format` prop (recent moves, which keep the missing-rate marker);
 * - `maskMoney` / `MASK` for text that is not formatted by `fmt.money` (the chart's value axis, the
 *   record-balances hint).
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
