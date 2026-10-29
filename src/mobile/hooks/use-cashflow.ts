import { useCallback, useMemo } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useLocation, useSearch } from 'wouter';
import type { BudgetingReport, CashflowActuals, CategorySpendingSummary } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { budgetingApi, cashflowApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { getUtcPeriodRange, type UtcPeriodRange } from '@/utils/period';
import {
  cashflowSearch,
  parseCashflowSearch,
  periodChosen,
  type CashflowState,
} from '@/mobile/lib/cashflow-period';
import { routes } from '@/mobile/shell/routes';

/**
 * The period and accounts of the Cashflow tab, kept in the URL search of its root (see
 * `CashflowState`). Writing replaces the history entry, so paging through months does not add
 * a screen to go back through. A write that changes the period pins it (`chosen`), so the tab no
 * longer moves to the newest month with data; a period already pinned stays pinned.
 */
export function useCashflowState(): [CashflowState, (next: CashflowState) => void] {
  const search = useSearch();
  const [, setLocation] = useLocation();
  const state = useMemo(() => parseCashflowSearch(search), [search]);
  const setState = useCallback(
    (next: CashflowState) => {
      const chosen =
        periodChosen(search) || next.timeframe !== state.timeframe || next.offset !== state.offset;
      setLocation(routes.cashflow + cashflowSearch(next, { chosen }), { replace: true });
    },
    [search, state, setLocation]
  );
  return [state, setState];
}

/**
 * The range of the selected period and its label ("Říjen 2026", "Q4 2026" or "2026"): all a screen
 * that only names the period needs, without fetching the report (the category drilldown).
 */
export function usePeriodLabel(state: Pick<CashflowState, 'timeframe' | 'offset'>): {
  range: UtcPeriodRange;
  periodLabel: string;
} {
  const fmt = useFormat();
  const { timeframe, offset } = state;
  const range = useMemo(() => getUtcPeriodRange(timeframe, offset), [timeframe, offset]);
  const periodLabel = useMemo(() => {
    if (timeframe === 'monthly') return fmt.month(range.startDate);
    if (timeframe === 'quarterly') {
      return `Q${Math.floor(range.startDate.getUTCMonth() / 3) + 1} ${range.startDate.getUTCFullYear()}`;
    }
    return String(range.startDate.getUTCFullYear());
  }, [timeframe, range, fmt]);
  return { range, periodLabel };
}

export interface CashflowReport {
  range: UtcPeriodRange;
  /** "Říjen 2026", "Q4 2026" or "2026". */
  periodLabel: string;
  actuals: CashflowActuals | undefined;
  /** The budgets of the period; they only add the limits to the rows, so the screen does not wait for them. */
  budgeting: BudgetingReport | undefined;
  /** The expense budget summaries by category id. */
  budgetByCategory: ReadonlyMap<string, CategorySpendingSummary>;
  /** The currency the report amounts are in: the report's own, else the profile currency. */
  reportCurrency: string;
  /** The actuals are loading for the first time (there is nothing to show yet). */
  isLoading: boolean;
  /** The actuals or the budgets shown are those of the previous period, while the selected one loads. */
  isPlaceholder: boolean;
  /** The actuals failed to load. */
  isError: boolean;
  refetch: () => void;
}

const NO_BUDGETS: ReadonlyMap<string, CategorySpendingSummary> = new Map();

/**
 * The actuals and the budgets of the selected period. The query keys are the desktop's
 * (`src/pages/Cashflow.tsx`, `src/pages/Budgeting.tsx`), so a money write that refreshes
 * `['cashflow-report']` and `['budgeting-report']` refreshes the phone too, and the same
 * period is one cache entry on both.
 */
export function useCashflowReport(state: CashflowState): CashflowReport {
  const { currencyCode, fxRevision } = useCurrency();
  const { timeframe, accountIds } = state;
  const { range, periodLabel } = usePeriodLabel(state);

  const actualsQuery = useQuery<CashflowActuals>({
    queryKey: [
      'cashflow-report',
      'actuals',
      range.start,
      range.end,
      accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: () => cashflowApi.getActuals(range.start, range.end, accountIds, currencyCode),
    staleTime: 0,
    refetchOnMount: 'always',
    placeholderData: keepPreviousData,
  });

  const budgetingQuery = useQuery<BudgetingReport>({
    queryKey: [
      'budgeting-report',
      range.start,
      range.end,
      timeframe,
      accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: () =>
      budgetingApi.getReport(range.start, range.end, timeframe, accountIds, currencyCode),
    // Like the actuals: the limits stay on the rows while the next period loads, so both dim
    // and swap together
    placeholderData: keepPreviousData,
  });

  const actuals = actualsQuery.data;
  const budgeting = budgetingQuery.data;

  const budgetByCategory = useMemo(
    () =>
      budgeting
        ? new Map(budgeting.expenseCategories.map((summary) => [summary.categoryId, summary]))
        : NO_BUDGETS,
    [budgeting]
  );

  const { refetch: refetchActuals } = actualsQuery;
  const { refetch: refetchBudgeting } = budgetingQuery;
  const refetch = useCallback(() => {
    void refetchActuals();
    void refetchBudgeting();
  }, [refetchActuals, refetchBudgeting]);

  return {
    range,
    periodLabel,
    actuals,
    budgeting,
    budgetByCategory,
    reportCurrency: actuals?.reportCurrency ?? currencyCode,
    isLoading: actualsQuery.isLoading,
    isPlaceholder: actualsQuery.isPlaceholderData || budgetingQuery.isPlaceholderData,
    isError: actualsQuery.isError,
    refetch,
  };
}
