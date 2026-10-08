import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';

/** Format report amounts in the currency declared by their response. */
export function useReportMoney(reportCurrency?: string) {
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  const displayCurrency = reportCurrency ?? currencyCode;
  const money = (amount: number) => fmt.money(amount, displayCurrency);
  const signedMoney = (amount: number) => fmt.money(amount, displayCurrency, { signed: true });
  return {
    money,
    signedMoney,
    displayCurrency,
  };
}
