import React, { useEffect, useMemo, useState } from 'react';
import { useAuth } from '@/hooks/use-auth';
import { useQueryClient } from '@tanstack/react-query';
import { portfolioApi } from '@/lib/tauri-api';
import { CurrencyContext } from '@/lib/currency';
import { getFormatters } from '@/lib/format';
import { useLanguage } from '@/i18n/I18nProvider';
import {
  CURRENCIES,
  CurrencyCode,
  CurrencyDef,
  convertFromCzK,
  convertToCzK,
  priceDecimals,
  updateExchangeRates,
} from '@shared/currencies';

export function CurrencyProvider({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  const { getLocale } = useLanguage();
  const queryClient = useQueryClient();
  const [ratesTimestamp, setRatesTimestamp] = useState(0);

  // Derive currencyCode from user profile - no need for separate state
  const currencyCode = useMemo<CurrencyCode>(() => {
    return (user?.currency as CurrencyCode) ?? 'CZK';
  }, [user?.currency]);

  // Initialize and refresh exchange rates after unlock.
  // IMPORTANT: keyed on the user *id* (not the user object) because:
  //   - load_rates_from_db runs inside db.open_with_password (on unlock)
  //   - save_rates_to_db (called by refreshExchangeRates) requires an open DB
  //   - before unlock, both calls fail/return empty → frontend stays at hardcoded fallbacks
  // The id is a stable primitive, so this runs once per unlock; depending on the
  // whole user object would re-fetch ECB rates on every profile write.
  const userId = user?.id;
  useEffect(() => {
    if (userId === undefined) return; // DB not open yet; wait for unlock

    async function initRates() {
      // Step 1: Immediately sync frontend with backend rates (fast IPC, no network).
      // DB is now open, so Rust has already loaded rates from SQLite. This ensures
      // the Stocks/Crypto pages use the same rates as portfolio-metrics from the
      // very first render after unlock, before the ECB network call completes.
      try {
        const backendRates = await portfolioApi.getExchangeRates();
        if (backendRates && Object.keys(backendRates).length > 1) {
          updateExchangeRates(backendRates);
          setRatesTimestamp(Date.now());
        }
      } catch {
        // ignore — will be corrected by the ECB fetch below
      }

      // Step 2: Refresh from ECB (slow, network). save_rates_to_db now succeeds
      // because the DB is open. Updates both Rust in-memory rates and the frontend.
      try {
        const freshRates = await portfolioApi.refreshExchangeRates();
        if (freshRates && typeof freshRates === 'object') {
          updateExchangeRates(freshRates);
          setRatesTimestamp(Date.now());
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        }
      } catch (error) {
        console.warn('[CURRENCY] Failed to fetch ECB rates, using backend rates:', error);
      }
    }
    initRates();
  }, [userId, queryClient]); // re-runs on unlock (userId: undefined → number)

  const currency =
    (CURRENCIES as Record<CurrencyCode, CurrencyDef | undefined>)[currencyCode] ?? CURRENCIES.CZK;

  function convert(amount: number, from: CurrencyCode, to: CurrencyCode): number {
    if (from === to) return amount;
    // If converting FROM base (CZK) to target
    if (from === 'CZK') {
      return convertFromCzK(amount, to);
    }
    // If converting TO base (CZK) from source
    if (to === 'CZK') {
      return convertToCzK(amount, from);
    }
    // Cross conversion: From -> CZK -> To
    const inCzk = convertToCzK(amount, from);
    return convertFromCzK(inCzk, to);
  }

  // All text output goes through the locale-driven formatters:
  // separators and symbol placement follow the UI language, not the currency.
  const fmt = getFormatters(getLocale());
  const decimalsFrom = (opts: Intl.NumberFormatOptions | undefined, fallback: number) =>
    opts?.maximumFractionDigits ?? opts?.minimumFractionDigits ?? fallback;

  /** Convert a CZK value to the display currency and format it (0 decimals by default). */
  function formatCurrency(value: number, opts?: Intl.NumberFormatOptions) {
    if (value === null || value === undefined || Number.isNaN(Number(value))) return '';
    const convertedValue = convertFromCzK(value, currencyCode);
    return fmt.money(convertedValue, currencyCode, { decimals: decimalsFrom(opts, 0) });
  }

  /** Convert a CZK value to the display currency and format it with an explicit sign, 0 decimals. */
  function formatCurrencySigned(value: number) {
    if (value === null || value === undefined || Number.isNaN(Number(value))) return '';
    const convertedValue = convertFromCzK(value, currencyCode);
    return fmt.money(convertedValue, currencyCode, { signed: true, decimals: 0 });
  }

  /**
   * Format a value that's already in the user's preferred currency (no conversion)
   * Use this for:
   * - Values already converted to user currency
   * - Calculator inputs where user enters in their preferred currency
   */
  function formatCurrencyRaw(value: number, opts?: Intl.NumberFormatOptions) {
    if (value === null || value === undefined || Number.isNaN(Number(value))) return '';
    return fmt.money(value, currencyCode, { decimals: decimalsFrom(opts, 0) });
  }

  /** Compact display-currency amount for cards and axes ("1.3M", "350K"). */
  function formatCurrencyShort(value: number) {
    if (value === null || value === undefined || Number.isNaN(Number(value))) return '';
    const convertedValue = convertFromCzK(value, currencyCode);
    if (Math.abs(convertedValue) < 1000) {
      return fmt.money(convertedValue, currencyCode, { decimals: 0 });
    }
    return fmt.money(convertedValue, currencyCode, { compact: true });
  }

  /**
   * Format a price with smart rounding:
   * - the currency's own decimals for values <= 999 (0 for CZK/JPY, 2 elsewhere)
   * - 0 decimal places for values > 999
   * Use this for per-unit prices (e.g., crypto/stock price per unit)
   */
  function formatPrice(value: number): string {
    return fmt.money(value, currencyCode, { decimals: priceDecimals(value, currencyCode) });
  }

  // No-op: currency is now derived from user.currency, changes go through profile API
  const setCurrency = (_code: CurrencyCode) => {
    // Intentionally empty - currency updates happen via profile API
    // The currencyCode is derived from user.currency via useMemo
  };

  return (
    <CurrencyContext.Provider
      value={{
        currencyCode,
        currency,
        setCurrency,
        formatCurrency,
        formatCurrencySigned,
        formatCurrencyRaw,
        formatCurrencyShort,
        formatPrice,
        convert,
        ratesTimestamp,
      }}
    >
      {children}
    </CurrencyContext.Provider>
  );
}
