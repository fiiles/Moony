import { useFormat } from '@/lib/use-format';
import type { Formatters } from '@/lib/format';

/**
 * `useFormat().money` for the phone: the same formatter (never its own `Intl`). Use it where a
 * plain string is needed (a text that sizes itself, an accessible name, a chart label);
 * `<Money>` renders the same text as an element. It has the signature of `fmt.money`, so shared
 * hooks that format money take it as their formatter.
 */
export function useMoneyText(): Formatters['money'] {
  return useFormat().money;
}
