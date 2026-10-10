import { createContext, useContext } from 'react';
import { CurrencyCode } from '@shared/currencies';
import type { FxQuotes } from '@shared/fx';

export type CurrencyContextValue = {
  /** The profile (main) currency: reports default to it, and only it is shown as a symbol. */
  currencyCode: CurrencyCode;
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
   * Null when a quote is missing (never 1:1); same currency is the identity.
   */
  convert: (amount: number, from: string, to?: string) => number | null;
  setCurrency: (c: CurrencyCode) => void;
};

/** Consumed by CurrencyProvider in ./currency-provider.tsx; use useCurrency() elsewhere. */
export const CurrencyContext = createContext<CurrencyContextValue | undefined>(undefined);

export function useCurrency() {
  const ctx = useContext(CurrencyContext);
  if (!ctx) throw new Error('useCurrency must be used within CurrencyProvider');
  return ctx;
}
