import { createContext, useContext } from 'react';
import { CurrencyCode, CurrencyDef } from '@shared/currencies';
import type { FxQuotes } from '@shared/fx';

export type CurrencyContextValue = {
  currencyCode: CurrencyCode;
  currency: CurrencyDef;
  /** Current quotes (reference-based, derived codes included); undefined until loaded. */
  fx: FxQuotes | undefined;
  /** Fetched-at seconds of the quotes; part of every converted query key. */
  fxRevision: number;
  /**
   * The quotes have loaded (or failed to). Until then a null conversion means "pending",
   * not "missing": no missing marker, no missing note.
   */
  fxReady: boolean;
  /**
   * Convert at the current quotes; `to` defaults to the profile currency.
   * Null when a quote is missing (never 1:1). Renamed to `convert` in Task 21.
   */
  fxConvert: (amount: number, from: string, to?: string) => number | null;
  setCurrency: (c: CurrencyCode) => void;
  /** @deprecated removed in Task 21 */
  formatCurrency: (value: number, opts?: Intl.NumberFormatOptions) => string;
  /**
   * Convert a CZK value to the display currency and format it with an explicit sign, 0 decimals.
   * @deprecated removed in Task 21
   */
  formatCurrencySigned: (value: number) => string;
  /** @deprecated removed in Task 21 */
  formatCurrencyRaw: (value: number, opts?: Intl.NumberFormatOptions) => string;
  /** @deprecated removed in Task 21 */
  formatCurrencyShort: (value: number) => string;
  /**
   * Format a price with smart rounding: the currency's decimals below 1000, 0 decimals from 1000 up
   * @deprecated removed in Task 21
   */
  formatPrice: (value: number) => string;
  /** CZK-based, 1:1 fallback for unknown codes. @deprecated removed in Task 21 (use fxConvert) */
  convert: (amount: number, from: CurrencyCode, to: CurrencyCode) => number;
  /** @deprecated removed in Task 21 (use fxRevision) */
  ratesTimestamp: number; // Added to trigger re-renders when rates update
};

/** Consumed by CurrencyProvider in ./currency-provider.tsx; use useCurrency() elsewhere. */
export const CurrencyContext = createContext<CurrencyContextValue | undefined>(undefined);

export function useCurrency() {
  const ctx = useContext(CurrencyContext);
  if (!ctx) throw new Error('useCurrency must be used within CurrencyProvider');
  return ctx;
}
