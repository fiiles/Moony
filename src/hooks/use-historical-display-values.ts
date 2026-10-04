import { useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { portfolioApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import type { CurrencyCode } from '@shared/currencies';
import type { PortfolioMetricsHistory } from '@shared/schema';
import {
  convertBreakdownAtDay,
  convertCzkAtDay,
  dayKeyFor,
  resolveRatesForDay,
  SECONDS_PER_DAY,
  type RatesByDay,
} from '@/utils/historical-rates';

// Fetch a few extra days before the oldest row so that rows falling on days
// without a rate snapshot (weekends/holidays) can still resolve to the
// closest earlier day's rates.
const RATE_LOOKBACK_SECONDS = 7 * SECONDS_PER_DAY;

export interface HistoricalDisplayValues {
  /** Current display currency (passthrough from useCurrency). */
  currencyCode: CurrencyCode;
  /** True when values need conversion (display currency is not CZK). */
  isConverting: boolean;
  /**
   * Convert one history data point (native `*ByCurrency` breakdown JSON +
   * stored CZK total) into the display currency using the rates of the day it
   * was recorded (closest earlier day, then today's rates as last resort).
   * For CZK display this returns `czkTotal` untouched.
   */
  convertPoint: (
    recordedAt: number,
    breakdownJson: string | null | undefined,
    czkTotal: number
  ) => number;
  /**
   * Convert a stored CZK amount into the display currency at the recorded
   * day's rate (fallback: today's rate). Used where no native breakdown
   * matches the wanted total (e.g. excluded personal real estate).
   */
  convertCzkPoint: (recordedAt: number, czkAmount: number) => number;
  /** Convert a CZK amount at today's rates (for the live "today" point). */
  convertCzkToday: (czkAmount: number) => number;
}

/**
 * Shared conversion logic for every chart fed by `portfolio-history`
 * (spec 2026-08-06-calculation-fixes §3): fetches historical exchange rates
 * for the history's date range and exposes per-day converters so charts show
 * what the portfolio was worth in the display currency on each day.
 *
 * For CZK display no rates query fires and stored CZK totals pass through
 * unchanged.
 */
export function useHistoricalDisplayValues(
  history: PortfolioMetricsHistory[] | undefined
): HistoricalDisplayValues {
  const { currencyCode, convert } = useCurrency();

  const range = useMemo(() => {
    if (!history || history.length === 0) return undefined;
    let min = Infinity;
    let max = -Infinity;
    for (const h of history) {
      if (h.recordedAt < min) min = h.recordedAt;
      if (h.recordedAt > max) max = h.recordedAt;
    }
    return { startTs: dayKeyFor(min) - RATE_LOOKBACK_SECONDS, endTs: dayKeyFor(max) };
  }, [history]);

  const { data: ratesByDay } = useQuery<RatesByDay>({
    queryKey: ['exchange-rates-range', range?.startTs ?? 0, range?.endTs ?? 0],
    queryFn: () => portfolioApi.getExchangeRatesForDateRange(range!.startTs, range!.endTs),
    enabled: !!range && currencyCode !== 'CZK',
    staleTime: 60 * 60 * 1000,
  });

  const convertToday = useCallback(
    (amount: number, fromCurrency: string) =>
      convert(amount, fromCurrency as CurrencyCode, currencyCode),
    [convert, currencyCode]
  );

  const convertPoint = useCallback(
    (recordedAt: number, breakdownJson: string | null | undefined, czkTotal: number) => {
      if (currencyCode === 'CZK') return czkTotal;
      const dayRates = resolveRatesForDay(ratesByDay, dayKeyFor(recordedAt));
      return convertBreakdownAtDay(breakdownJson, czkTotal, dayRates, currencyCode, convertToday);
    },
    [ratesByDay, currencyCode, convertToday]
  );

  const convertCzkPoint = useCallback(
    (recordedAt: number, czkAmount: number) => {
      if (currencyCode === 'CZK') return czkAmount;
      const dayRates = resolveRatesForDay(ratesByDay, dayKeyFor(recordedAt));
      return convertCzkAtDay(czkAmount, dayRates, currencyCode, convertToday);
    },
    [ratesByDay, currencyCode, convertToday]
  );

  const convertCzkToday = useCallback(
    (czkAmount: number) => convert(czkAmount, 'CZK', currencyCode),
    [convert, currencyCode]
  );

  return {
    currencyCode,
    isConverting: currencyCode !== 'CZK',
    convertPoint,
    convertCzkPoint,
    convertCzkToday,
  };
}
