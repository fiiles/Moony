import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { formatReportMoney } from '@/utils/report-currencies';

/**
 * Format report amounts in the currency declared by their response, at ISO
 * precision so rows, totals and the CSV export reconcile (CZK 100.40 + 100.40
 * reads 200.80, not 100 + 100 = 201). `nativeMoney` formats report details in
 * their own currency (budget rows, drilldown originals, planning components).
 */
export function useReportMoney(reportCurrency?: string) {
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  const displayCurrency = reportCurrency ?? currencyCode;
  const money = (amount: number) => formatReportMoney(fmt, amount, displayCurrency);
  const signedMoney = (amount: number) =>
    formatReportMoney(fmt, amount, displayCurrency, { signed: true });
  const nativeMoney = (amount: number, currency: string, opts?: { signed?: boolean }) =>
    formatReportMoney(fmt, amount, currency, opts);
  return {
    money,
    signedMoney,
    nativeMoney,
    displayCurrency,
  };
}
