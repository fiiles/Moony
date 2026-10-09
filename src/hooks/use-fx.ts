import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { fxApi } from '@/lib/tauri-api';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';
import {
  convert,
  convertOn,
  makeHistory,
  type ConvertAtFn,
  type ConvertFn,
  type FxHistory,
  type FxQuotes,
} from '@shared/fx';

const SECONDS_PER_DAY = 86_400;
/** Mirrors the ten-day lookback of `convertOn`, so the first day of a range can still resolve. */
const HISTORY_LOOKBACK_DAYS = 10;

/** Query keys to invalidate after the exchange rates were refreshed. */
export const FX_SNAPSHOT_KEY = ['fx-snapshot'] as const;
export const FX_HISTORY_KEY = ['fx-history'] as const;

/**
 * Current quotes from the backend. `revision` (fetched-at seconds, 0 when no
 * rates are loaded) belongs in the key of every query whose value is converted,
 * so converted values refetch when the rates change.
 */
export function useFxSnapshot(): {
  quotes: FxQuotes | undefined;
  revision: number;
  isLoading: boolean;
} {
  const { user } = useAuth();
  const unlocked = user?.id !== undefined;
  const { data, isLoading } = useQuery({
    queryKey: FX_SNAPSHOT_KEY,
    queryFn: () => fxApi.getSnapshot(),
    enabled: unlocked,
    staleTime: 5 * 60 * 1000,
  });
  const quotes = useMemo<FxQuotes | undefined>(
    () => (data ? { reference: data.reference, units: data.units } : undefined),
    [data]
  );
  return { quotes, revision: data?.revision ?? 0, isLoading: unlocked && isLoading };
}

/**
 * Stored daily quotes covering `[startTs, endTs]` (unix seconds, any time of
 * day) plus the lookback `convertOn` may use. `undefined` while loading or
 * when no range is given.
 */
export function useFxHistory(
  startTs: number | undefined,
  endTs: number | undefined
): FxHistory | undefined {
  const { user } = useAuth();
  const startDay =
    startTs === undefined
      ? undefined
      : Math.floor(startTs / SECONDS_PER_DAY) * SECONDS_PER_DAY -
        HISTORY_LOOKBACK_DAYS * SECONDS_PER_DAY;
  const endDay =
    endTs === undefined ? undefined : Math.floor(endTs / SECONDS_PER_DAY) * SECONDS_PER_DAY;
  const { data } = useQuery({
    queryKey: [...FX_HISTORY_KEY, startDay, endDay],
    queryFn: () => fxApi.getHistory(startDay as number, endDay as number),
    enabled: user?.id !== undefined && startDay !== undefined && endDay !== undefined,
    staleTime: 60 * 60 * 1000,
  });
  // The history carries its own reference: it is never assumed to match the snapshot's.
  return useMemo(() => (data ? makeHistory(data.days, data.reference) : undefined), [data]);
}

/**
 * Converts amounts at the rates of their own day (common-day rule) from `firstTs` (unix
 * seconds) up to today. `convertAt` returns `null` when a day cannot be converted and while
 * the daily quotes are still loading; `ready` tells the two apart.
 */
export function useDatedFx(firstTs: number | undefined): {
  ready: boolean;
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
} {
  const history = useFxHistory(firstTs, firstTs === undefined ? undefined : Date.now() / 1000);
  return useMemo(
    () => ({
      ready: history !== undefined,
      convertAt: (amount, from, to, timestamp) => convertOn(history, amount, from, to, timestamp),
    }),
    [history]
  );
}

/**
 * Converts amounts at the rates of a transaction's own day (common-day rule), for the
 * transactions' date range. `convertAt` returns `null` when a day cannot be converted and
 * while the daily quotes are still loading; `ready` tells the two apart.
 */
export function useTransactionFx(transactions: { transactionDate: number }[] | undefined): {
  ready: boolean;
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
} {
  const first = useMemo(
    () =>
      transactions && transactions.length > 0
        ? Math.min(...transactions.map((tx) => tx.transactionDate))
        : undefined,
    [transactions]
  );
  return useDatedFx(first);
}

/**
 * Converter into the profile (main) currency at the current quotes. Stable until the
 * currency or the rates change, so it can sit in `useMemo` dependencies.
 */
export function useMainConvert(): ConvertFn {
  const { currencyCode, fx, fxRevision } = useCurrency();
  return useMemo<ConvertFn>(() => {
    // A new revision is a new set of quotes, even when the object compares equal
    void fxRevision;
    return (amount, currency) => convert(fx, amount, currency, currencyCode);
  }, [fx, fxRevision, currencyCode]);
}

/**
 * Converter into the profile (main) currency at the rates of each value's own day, from
 * `firstTs` up to today. `ready` is false while the daily quotes load (`convertAt` then
 * returns null for every foreign amount).
 */
export function useDatedMainConvert(firstTs: number | undefined): {
  ready: boolean;
  convertAt: ConvertAtFn;
} {
  const { currencyCode } = useCurrency();
  const { ready, convertAt } = useDatedFx(firstTs);
  return useMemo(
    () => ({
      ready,
      convertAt: (amount, currency, timestamp) =>
        convertAt(amount, currency, currencyCode, timestamp),
    }),
    [ready, convertAt, currencyCode]
  );
}
