import { useMemo } from 'react';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { SIGNED_WHOLE, WHOLE } from '@/mobile/lib/money-rule';

/**
 * The amounts of a report on the phone: whole, as every summary is (README §13, "Money and
 * currencies"), in the currency the report declares. The phone's counterpart of the desktop's
 * `useReportMoney`, which writes the ISO minor units so that a table's rows add up to its total;
 * on the phone a category row, a total and a limit are summaries, and only a single payment is
 * exact. `nativeMoney` writes an amount of another currency (a limit's) the same way.
 */
export function useSummaryMoney(reportCurrency: string) {
  const moneyText = useMoneyText();
  return useMemo(
    () => ({
      money: (amount: number) => moneyText(amount, reportCurrency, WHOLE),
      signedMoney: (amount: number) => moneyText(amount, reportCurrency, SIGNED_WHOLE),
      nativeMoney: (amount: number, currency: string, opts?: { signed?: boolean }) =>
        moneyText(amount, currency, { ...opts, decimals: 0 }),
    }),
    [moneyText, reportCurrency]
  );
}
