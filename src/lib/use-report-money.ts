import { hasKnownRate, type CurrencyCode } from '@shared/currencies';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';

/** Reports are historical CZK. Foreign presentation uses only a known current display rate. */
export function useReportMoney() {
  const { currencyCode, convert } = useCurrency();
  const fmt = useFormat();
  const displayCurrency: CurrencyCode = hasKnownRate(currencyCode) ? currencyCode : 'CZK';
  const money = (amount: number) =>
    fmt.money(convert(amount, 'CZK', displayCurrency), displayCurrency);
  const signedMoney = (amount: number) =>
    fmt.money(convert(amount, 'CZK', displayCurrency), displayCurrency, { signed: true });
  return {
    money,
    signedMoney,
    displayCurrency,
    unsupported: displayCurrency !== currencyCode,
    converted: displayCurrency !== 'CZK',
  };
}
