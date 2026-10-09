import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { fxApi } from '@/lib/tauri-api';
import { useAuth } from '@/hooks/use-auth';
import { makeHistory, type FxHistory, type FxQuotes } from '@shared/fx';

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
  const { quotes } = useFxSnapshot();
  const reference = quotes?.reference;
  return useMemo(
    () => (data && reference ? makeHistory(data, reference) : undefined),
    [data, reference]
  );
}
