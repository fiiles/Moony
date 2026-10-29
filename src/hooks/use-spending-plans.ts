import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { spendingPlansApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import type { PlanCandidateFilters } from '@shared/schema';

/**
 * Every plan of every year with its spend (plan currency), for the plan pickers. Spent is
 * converted at the payments' days, so a rate refresh refetches it (`fxRevision`).
 */
export function useSpendingPlanIndex() {
  const { fxRevision } = useCurrency();
  return useQuery({
    queryKey: ['spending-plans', 'index', fxRevision],
    queryFn: () => spendingPlansApi.getIndex(),
  });
}

/**
 * One year's plans with their spend; the totals are in the report (main) currency. The previous
 * response stays while another year loads (check `data.year`), so the year stepper keeps its range.
 */
export function useSpendingPlansOverview(year: number, enabled = true) {
  const { currencyCode, fxRevision } = useCurrency();
  return useQuery({
    queryKey: ['spending-plans', year, currencyCode, fxRevision],
    queryFn: () => spendingPlansApi.getOverview(year, currencyCode),
    placeholderData: keepPreviousData,
    enabled,
  });
}

/** One plan with its spend per month and category, in the plan currency. */
export function useSpendingPlanDetail(id: string | undefined) {
  const { fxRevision } = useCurrency();
  return useQuery({
    queryKey: ['spending-plan', id, fxRevision],
    queryFn: () => spendingPlansApi.getDetail(id as string),
    enabled: !!id,
  });
}

/** The payments tagged with a plan (pending ones included), native amounts. */
export function useSpendingPlanTransactions(id: string | undefined) {
  return useQuery({
    queryKey: ['spending-plan-transactions', id],
    queryFn: () => spendingPlansApi.getTransactions(id as string),
    enabled: !!id,
  });
}

/** Payments that can be added to a plan, across all accounts ("Přidat platby do plánu"). */
export function usePlanCandidates(filters: PlanCandidateFilters, enabled: boolean) {
  return useQuery({
    queryKey: ['spending-plan-candidates', filters],
    queryFn: () => spendingPlansApi.searchCandidates(filters),
    enabled,
  });
}
