import { createContext, useContext } from 'react';
import { CurrencyCode, CurrencyDef } from '@shared/currencies';

export type CurrencyContextValue = {
  currencyCode: CurrencyCode;
  currency: CurrencyDef;
  setCurrency: (c: CurrencyCode) => void;
  formatCurrency: (value: number, opts?: Intl.NumberFormatOptions) => string;
  /** Convert a CZK value to the display currency and format it with an explicit sign, 0 decimals. */
  formatCurrencySigned: (value: number) => string;
  formatCurrencyRaw: (value: number, opts?: Intl.NumberFormatOptions) => string;
  formatCurrencyShort: (value: number) => string;
  /** Format a price with smart rounding: the currency's decimals below 1000, 0 decimals from 1000 up */
  formatPrice: (value: number) => string;
  convert: (amount: number, from: CurrencyCode, to: CurrencyCode) => number;
  ratesTimestamp: number; // Added to trigger re-renders when rates update
};

/** Consumed by CurrencyProvider in ./currency-provider.tsx; use useCurrency() elsewhere. */
export const CurrencyContext = createContext<CurrencyContextValue | undefined>(undefined);

export function useCurrency() {
  const ctx = useContext(CurrencyContext);
  if (!ctx) throw new Error('useCurrency must be used within CurrencyProvider');
  return ctx;
}
