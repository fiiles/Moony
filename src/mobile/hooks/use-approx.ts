import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useMainConvert } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { approxMoneyOptions, approxOf } from '@/mobile/lib/money-rule';

/** A conversion the caller already has (a payment at its own day's rate) instead of today's quotes. */
interface ApproxSource {
  /** The amount in the main currency; null without a rate. */
  converted: number | null;
  /** The rates behind `converted` are still loading: a null is then not "no rate". */
  pending: boolean;
}

/**
 * The line under a native amount (README §13, "Money and currencies"): "≈ 25 000 Kč" when the
 * record's currency is not the main one, "bez kurzu pro EUR" when it cannot be converted, and
 * `undefined` (no line) for a record in the main currency, a zero amount and while the rates load.
 * Converts at today's quotes unless the caller passes its own conversion. The amount is unsigned:
 * the native amount above it carries the sign.
 */
export function useApprox(): (
  amount: number,
  currency: string,
  source?: ApproxSource
) => string | undefined {
  const { t } = useTranslation('mobile');
  const moneyText = useMoneyText();
  const { currencyCode, fxReady } = useCurrency();
  const convert = useMainConvert();
  return useCallback(
    (amount, currency, source) => {
      const approx = approxOf({
        amount,
        currency,
        mainCurrency: currencyCode,
        converted: source ? source.converted : convert(amount, currency),
        pending: source ? source.pending : !fxReady,
      });
      if (approx.kind === 'none') return undefined;
      if (approx.kind === 'noRate') return t('money.noRate', { currency });
      const value = Math.abs(approx.value);
      return t('money.approx', {
        amount: moneyText(value, currencyCode, approxMoneyOptions(value, currencyCode)),
      });
    },
    [t, moneyText, currencyCode, fxReady, convert]
  );
}
