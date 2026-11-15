import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import type { PeriodUnit } from '@/mobile/lib/period-unit';

/**
 * An amount with its period unit next to it, the one form on the phone (README §13, "Dates and
 * periods", C6): "2 270 Kč / měs.", "11 216 Kč / rok", "{{amount}}/月". Without a unit (a one-time
 * payment) the amount stands alone.
 */
export function usePeriodText() {
  const { t } = useTranslation('mobile');
  return useCallback(
    (amount: string, unit: PeriodUnit | null) =>
      unit === null ? amount : t(`period.${unit}`, { amount }),
    [t]
  );
}
