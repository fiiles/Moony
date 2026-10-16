import { useCallback } from 'react';
import { useFormat } from '@/lib/use-format';
import type { Formatters } from '@/lib/format';
import { hiddenMoneyOptions, maskMoney } from '@/mobile/lib/hide-amounts';
import { useHideAmounts } from './use-hide-amounts';

/**
 * `useFormat().money` for the phone: the same formatter (never its own `Intl`), masked while
 * amounts are hidden (and then in full, never compact: "mil." would give the size away). Use it where a plain string is needed (a text that sizes itself, an
 * accessible name, a chart label); `<Money>` renders the same text as an element. It has the
 * signature of `fmt.money`, so shared hooks that format money take it as their formatter.
 */
export function useMoneyText(): Formatters['money'] {
  const fmt = useFormat();
  const { hidden } = useHideAmounts();
  return useCallback<Formatters['money']>(
    (amount, currency, options) =>
      hidden
        ? maskMoney(fmt.money(amount, currency, hiddenMoneyOptions(options)))
        : fmt.money(amount, currency, options),
    [fmt, hidden]
  );
}
