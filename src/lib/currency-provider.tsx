import React, { useEffect, useMemo } from 'react';
import { useAuth } from '@/hooks/use-auth';
import { useQueryClient } from '@tanstack/react-query';
import { portfolioApi } from '@/lib/tauri-api';
import { CurrencyContext } from '@/lib/currency';
import { useFxSnapshot, FX_SNAPSHOT_KEY, FX_HISTORY_KEY } from '@/hooks/use-fx';
import { convert as fxConvertPure } from '@shared/fx';
import { CURRENCIES, CurrencyCode, CurrencyDef } from '@shared/currencies';

export function CurrencyProvider({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const { quotes: fx, revision: fxRevision, isLoading: fxLoading } = useFxSnapshot();

  // Derive currencyCode from user profile - no need for separate state
  const currencyCode = useMemo<CurrencyCode>(() => {
    return (user?.currency as CurrencyCode) ?? 'CZK';
  }, [user?.currency]);

  // Refresh exchange rates from the ECB after unlock.
  // IMPORTANT: keyed on the user *id* (not the user object) because:
  //   - load_rates_from_db runs inside db.open_with_password (on unlock), so the
  //     FX snapshot query already serves the stored rates
  //   - save_rates_to_db (called by refreshExchangeRates) requires an open DB
  // The id is a stable primitive, so this runs once per unlock; depending on the
  // whole user object would re-fetch ECB rates on every profile write.
  const userId = user?.id;
  useEffect(() => {
    if (userId === undefined) return; // DB not open yet; wait for unlock

    async function refreshRates() {
      try {
        await portfolioApi.refreshExchangeRates();
        queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        queryClient.invalidateQueries({ queryKey: FX_SNAPSHOT_KEY });
        queryClient.invalidateQueries({ queryKey: FX_HISTORY_KEY });
        queryClient.invalidateQueries({ queryKey: ['price-status'] });
      } catch (error) {
        console.warn('[CURRENCY] Failed to fetch ECB rates, using stored rates:', error);
      }
    }
    refreshRates();
  }, [userId, queryClient]); // re-runs on unlock (userId: undefined → number)

  const currency =
    (CURRENCIES as Record<CurrencyCode, CurrencyDef | undefined>)[currencyCode] ?? CURRENCIES.CZK;

  function convert(amount: number, from: string, to: string = currencyCode): number | null {
    return fxConvertPure(fx, amount, from, to);
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
        fx,
        fxRevision,
        fxReady: !fxLoading,
        convert,
        setCurrency,
      }}
    >
      {children}
    </CurrencyContext.Provider>
  );
}
