import { useMemo } from 'react';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { usePortfolioHistory } from '@/hooks/use-portfolio';
import { useCurrency } from '@/lib/currency';
import { chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { cashTrendPoints } from '@/mobile/lib/cash-trend';

/**
 * The cash line of "Vývoj hotovosti" (spec §8.4): the recorded cash of the portfolio history from
 * the start of `period`, ending on `todayValue` (the page's total; `null` while unknown), and the
 * currencies the history could not convert. The history refetches when a backend rebuild finishes
 * (`usePortfolioHistory` listens), as on the desktop's Bankovní účty.
 */
export function useCashTrend(
  period: ChartPeriod,
  todayValue: number | null
): { points: LinePoint[]; missing: string[] } {
  const { currencyCode } = useCurrency();
  // A whole UTC day, so the query key holds still for the day however often this renders
  const start = chartPeriodStart(period, Date.now() / 1000);
  const { data: history } = usePortfolioHistory(start);
  const points = useMemo(
    () => cashTrendPoints(history, currencyCode, todayValue, Date.now() / 1000),
    [history, currencyCode, todayValue]
  );
  return { points, missing: history?.fxQuality.missingCurrencies ?? [] };
}
