import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { dayLabel } from '@/mobile/lib/ledger';

/**
 * The one way the phone words a payment's day (C11): "Dnes", "Včera", else the short date with the
 * year only when it is not this year ("10. 10.", "10. 10. 2025", `shortDayOptions`'s rule). The
 * ledger's day heads, the categorize queue, a Cashflow category's rows and Přehled's "Poslední
 * pohyby" all use it, so a payment reads the same wherever it is listed.
 */
export function useTransactionDay(): (unixSeconds: number) => string {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  return useCallback(
    (unixSeconds: number) => {
      const label = dayLabel(utcDayFloor(unixSeconds), utcDayFloor(Date.now() / 1000));
      if ('key' in label) return t(`time.${label.key}`);
      return fmt.day(label.date, {
        day: 'numeric',
        month: 'numeric',
        year: label.withYear ? 'numeric' : undefined,
      });
    },
    [t, fmt]
  );
}
