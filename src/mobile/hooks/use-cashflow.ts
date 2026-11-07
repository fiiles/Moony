import { useCallback, useMemo } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useLocation, useSearch } from 'wouter';
import type { BudgetingReport, CashflowActuals } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { budgetingApi, cashflowApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { expenseRows, type ExpenseRow } from '@/utils/cashflow-budget-rows';
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
 * longer moves to the month of the newest transaction; a period already pinned stays pinned, and
 * `pin` pins the one written even when it is the current month (the opening period).
 */
export function useCashflowState(): [
  CashflowState,
  (next: CashflowState, options?: { pin?: boolean }) => void,
] {
  const search = useSearch();
  const [, setLocation] = useLocation();
  const state = useMemo(() => parseCashflowSearch(search), [search]);
  const setState = useCallback(
    (next: CashflowState, { pin = false }: { pin?: boolean } = {}) => {
      const chosen =
        pin ||
        periodChosen(search) ||
        next.timeframe !== state.timeframe ||
        next.offset !== state.offset;
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
  /** The budgets of the same period, accounts and currency (fetched with the actuals, never apart). */
  budgeting: BudgetingReport | undefined;
  /** The desktop's expense rows: the cashflow groups joined with the period's limits. */
  expenses: ExpenseRow[];
  /** The currency the report amounts are in: the report's own, else the profile currency. */
  reportCurrency: string;
  /** The report is loading for the first time (there is nothing to show yet). */
  isLoading: boolean;
  /** The report shown is the previous period's, while the selected one loads. */
  isPlaceholder: boolean;
  /** The report (actuals or budgets) failed to load. */
  isError: boolean;
  refetch: () => void;
}

/** Both reports of one period, account scope and report currency, fetched as one. */
interface Overview {
  actuals: CashflowActuals;
  budgeting: BudgetingReport;
}

const NO_ROWS: ExpenseRow[] = [];

/**
 * The actuals and the budgets of the selected period as ONE query, keyed exactly like the desktop
 * Cashflow's (`src/pages/Cashflow.tsx`): the expense rows never join a report of the new period or
 * accounts with one of the old, loading, placeholder, error and retry are one state, a money write
 * that refreshes `['cashflow-report']` refreshes it, and the same period is one cache entry on both
 * shells. `enabled` false holds it back while the opening period is not decided.
 */
export function useCashflowReport(
  state: CashflowState,
  { enabled = true }: { enabled?: boolean } = {}
): CashflowReport {
  const { currencyCode, fxRevision } = useCurrency();
  const { timeframe, accountIds } = state;
  const { range, periodLabel } = usePeriodLabel(state);

  const query = useQuery<Overview>({
    queryKey: [
      'cashflow-report',
      'overview',
      range.start,
      range.end,
      timeframe,
      accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: async () => {
      const [actuals, budgeting] = await Promise.all([
        cashflowApi.getActuals(range.start, range.end, accountIds, currencyCode),
        budgetingApi.getReport(range.start, range.end, timeframe, accountIds, currencyCode),
      ]);
      return { actuals, budgeting };
    },
    enabled,
    staleTime: 0,
    refetchOnMount: 'always',
    placeholderData: keepPreviousData,
  });

  const { data, refetch: refetchQuery } = query;
  const expenses = useMemo(
    () =>
      data
        ? expenseRows(data.actuals.expenseCategories, data.budgeting.expenseCategories)
        : NO_ROWS,
    [data]
  );
  const refetch = useCallback(() => void refetchQuery(), [refetchQuery]);

  return {
    range,
    periodLabel,
    actuals: data?.actuals,
    budgeting: data?.budgeting,
    expenses,
    reportCurrency: data?.actuals.reportCurrency ?? currencyCode,
    isLoading: query.isLoading,
    isPlaceholder: query.isPlaceholderData,
    isError: query.isError,
    refetch,
  };
}
