import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { Bond } from '@shared/schema';
import { todayUtcDay } from '@shared/calculations';
import { bondsApi } from '@/lib/tauri-api';
import {
  bondCouponCzk,
  bondValueCzk,
  isMatured,
  nearestMaturity,
  weightedYearsToMaturity,
} from '@/utils/bonds';

export interface BondsMetrics {
  /** Face value of the live bonds, CZK. */
  totalValue: number;
  /** Coupon rate weighted by CZK face value, percent. */
  averageYield: number;
  /** Yearly coupons of the live bonds, CZK. */
  projectedYearlyIncome: number;
  liveCount: number;
  maturedCount: number;
  currencies: string[];
  nearest: Bond | null;
  nearestValueCzk: number;
  weightedYears: number | null;
  lastMaturity: number | null;
}

/** Bonds with the list page's derived numbers (design system §7 List, prototype bonds.html). */
export function useBonds() {
  const { data: bonds = [], isLoading } = useQuery({
    queryKey: ['bonds'],
    queryFn: () => bondsApi.getAll(),
  });
  const today = todayUtcDay();

  const metrics = useMemo((): BondsMetrics => {
    const live = bonds.filter((b) => !isMatured(b, today));
    let totalValue = 0;
    let weightedRate = 0;
    let income = 0;
    for (const bond of live) {
      const value = bondValueCzk(bond);
      totalValue += value;
      weightedRate += value * (parseFloat(bond.interestRate || '0') || 0);
      income += bondCouponCzk(bond);
    }
    const nearest = nearestMaturity(live, today);
    return {
      totalValue,
      averageYield: totalValue > 0 ? weightedRate / totalValue : 0,
      projectedYearlyIncome: income,
      liveCount: live.length,
      maturedCount: bonds.length - live.length,
      currencies: Array.from(new Set(live.map((b) => b.currency || 'CZK'))),
      nearest,
      nearestValueCzk: nearest ? bondValueCzk(nearest) : 0,
      weightedYears: weightedYearsToMaturity(live, today),
      lastMaturity: live.reduce<number | null>(
        (max, b) =>
          b.maturityDate !== null && (max === null || b.maturityDate > max) ? b.maturityDate : max,
        null
      ),
    };
  }, [bonds, today]);

  return { bonds, isLoading, metrics, today };
}
