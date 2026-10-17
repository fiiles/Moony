import type { PortfolioHistoryPoint, PortfolioHistoryResponse } from '@shared/schema';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { utcDayFloor } from '@/utils/chart-axis';
import { amountOf } from '@/utils/positions';
import { periodChange, type PeriodChange } from './change';

/** The classes of the portfolio history a class page draws (the desktop trend card's types). */
export type TrendClass = 'investments' | 'crypto' | 'realEstate' | 'otherAssets' | 'savings';

/**
 * The part of a history day a class page shows (report currency); `null` when nothing in it could
 * be converted that day. `PortfolioTrendCard`'s `historyValue` as a pure function: real estate is
 * the personal and the investment part together, and unknown when either is.
 */
export function classHistoryValue(point: PortfolioHistoryPoint, cls: TrendClass): number | null {
  switch (cls) {
    case 'investments':
      return amountOf(point.totalInvestments);
    case 'crypto':
      return amountOf(point.totalCrypto);
    case 'otherAssets':
      return amountOf(point.totalOtherAssets);
    case 'savings':
      return amountOf(point.totalSavings);
    case 'realEstate': {
      const personal = amountOf(point.totalRealEstatePersonal);
      const investment = amountOf(point.totalRealEstateInvestment);
      return personal === null || investment === null ? null : personal + investment;
    }
  }
}

/**
 * The line of a class page: the recorded days of the class and today's value as its last point
 * (the desktop trend card's `points`). A history in another currency than `mainCurrency` (still
 * the previous one while a currency change settles) must not meet today's value, so it gives no
 * days. A day whose class could not be converted is left out, not plotted as a drop to 0; an
 * unknown `todayValue` leaves today out as well.
 */
export function classTrendPoints(
  history: Pick<PortfolioHistoryResponse, 'currency' | 'points'> | undefined,
  cls: TrendClass,
  mainCurrency: string,
  todayValue: number | null,
  nowSec: number
): LinePoint[] {
  const days = history !== undefined && history.currency === mainCurrency ? history.points : [];
  const points = days.flatMap((day) => {
    const value = classHistoryValue(day, cls);
    return value === null ? [] : [{ t: day.recordedAt, value }];
  });
  const today = utcDayFloor(nowSec);
  if (todayValue === null) return points.filter((point) => utcDayFloor(point.t) !== today);
  const last = points[points.length - 1];
  if (last !== undefined && utcDayFloor(last.t) === today) {
    return [...points.slice(0, -1), { ...last, value: todayValue }];
  }
  return [...points, { t: Math.floor(nowSec), value: todayValue }];
}

/** The last point against the first: the change a period's hero line says. `null` under two points. */
export function pointsChange(points: readonly LinePoint[]): PeriodChange | null {
  if (points.length < 2) return null;
  return periodChange(points[points.length - 1].value, points[0].value);
}
