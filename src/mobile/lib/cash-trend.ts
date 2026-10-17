import type { PortfolioHistoryPoint } from '@shared/schema';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { utcDayFloor } from '@/utils/chart-axis';
import { amountOf } from '@/utils/positions';

/**
 * The recorded cash of the portfolio history as a line (the `cash` trend of the desktop's
 * `PortfolioTrendCard`): `totalSavings` per day in the profile currency, a day whose cash could
 * not be converted left out (not plotted as a drop to 0), the last point today's total. A history
 * still in the previous currency is ignored; without today's value (`null`) today is left out.
 */
export function cashTrendPoints(
  history: { currency: string; points: PortfolioHistoryPoint[] } | undefined,
  mainCurrency: string,
  todayValue: number | null,
  nowSec: number
): LinePoint[] {
  const days = history && history.currency === mainCurrency ? history.points : [];
  const points = days.flatMap((day) => {
    const value = amountOf(day.totalSavings);
    return value === null ? [] : [{ t: day.recordedAt, value }];
  });
  const today = utcDayFloor(nowSec);
  if (todayValue === null) return points.filter((p) => utcDayFloor(p.t) !== today);
  const last = points[points.length - 1];
  if (last && utcDayFloor(last.t) === today) {
    return [...points.slice(0, -1), { t: last.t, value: todayValue }];
  }
  return [...points, { t: Math.floor(nowSec), value: todayValue }];
}
