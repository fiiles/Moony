/**
 * Date-aware currency conversion over the transactions' date range.
 *
 * Fetches the exchange-rate timeseries covering the given transactions once
 * (cached 1h) and returns a `DatedConvertFn` that converts any amount between
 * currencies at the rates of a transaction's own day — the correct way to
 * value historical cost (ADR 0001: converting old values at today's rate is
 * wrong). Falls back to today's rates while loading or where the timeseries
 * has a gap.
 */

import { useEffect, useMemo } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { portfolioApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import type { CurrencyCode } from '@shared/currencies';
import type { DatedConvertFn } from '@shared/calculations';
import {
  dayKeyFor,
  makeDatedConvertFn,
  SECONDS_PER_DAY,
  type RatesByDay,
} from '@/utils/historical-rates';

// Extra days before the earliest transaction so weekend/holiday dates can
// resolve to the closest earlier snapshot.
const RATE_LOOKBACK_SECONDS = 14 * SECONDS_PER_DAY;

export function useDatedConvert(
  transactions: { transactionDate: number }[] | undefined
): DatedConvertFn {
  const { convert } = useCurrency();
  const queryClient = useQueryClient();

  // The backend extends exchange_rate_history after historical transactions
  // and on startup (fx_backfill); refetch the range when it announces new data.
  useEffect(() => {
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['exchange-rates-range'] });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  const range = useMemo(() => {
    if (!transactions || transactions.length === 0) return undefined;
    let min = Infinity;
    let max = -Infinity;
    for (const tx of transactions) {
      if (tx.transactionDate < min) min = tx.transactionDate;
      if (tx.transactionDate > max) max = tx.transactionDate;
    }
    return { startTs: dayKeyFor(min) - RATE_LOOKBACK_SECONDS, endTs: dayKeyFor(max) };
  }, [transactions]);

  const { data: ratesByDay } = useQuery<RatesByDay>({
    queryKey: ['exchange-rates-range', range?.startTs ?? 0, range?.endTs ?? 0],
    queryFn: () => portfolioApi.getExchangeRatesForDateRange(range!.startTs, range!.endTs),
    enabled: !!range,
    staleTime: 60 * 60 * 1000,
  });

  return useMemo(() => {
    const fallback = (amount: number, from: string, to: string) =>
      convert(amount, from as CurrencyCode, to as CurrencyCode);
    const convertAt = makeDatedConvertFn(ratesByDay, fallback);
    return (amount, from, to, dateTs) => convertAt(amount, from, to, dateTs);
  }, [ratesByDay, convert]);
}
