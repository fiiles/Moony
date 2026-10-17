import { useCallback, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { BudgetGoal } from '@shared/schema';
import { useCategories } from '@/hooks/use-categories';
import { useCurrency } from '@/lib/currency';
import { budgetingApi } from '@/lib/tauri-api';
import { currentMonthStart } from '@/utils/budget-goal-versions';
import { averagesByCategory, averageWindow, type LimitMode } from '@/mobile/lib/budget-limit';
import { budgetedCategoryIds } from '@/mobile/lib/budget-rows';

const NO_GOALS: readonly BudgetGoal[] = [];

/** How long the six-month report counts as fresh (the desktop dialog's value). */
const AVERAGE_STALE_MS = 5 * 60 * 1000;

/**
 * Everything `BudgetLimitSheet` needs, and the one state it opens with, for the two pages that
 * open it (Spravovat rozpočty and the Cashflow report's row menus). The sheet resets its form on
 * the `open` false → true transition and does not re-read a `mode` that changes while it is open,
 * so `open` and `mode` live in one object and `openLimit` sets them together — a page cannot get
 * that wrong. An addition without a category is refused while none can be picked (`canPick`: the
 * limits have not loaded, or no category is free; the sheet's first step would be wrong or empty).
 *
 * The limits come from `['budget-goal-history']` (every version of every goal, the sheet's own
 * query), the hint's averages from one six-month report (`averageWindow`, the desktop dialog's
 * keys); `withAverages: false` holds that report back until the sheet opens (the Cashflow tab does
 * not need it before). The averages are in `averageCurrency`, the report's own.
 */
export function useBudgetLimitSheet(withAverages: boolean) {
  const { currencyCode, fxRevision } = useCurrency();
  const categoriesQuery = useCategories();
  const { categories } = categoriesQuery;
  const [sheet, setSheet] = useState<{ open: boolean; mode: LimitMode | null }>({
    open: false,
    mode: null,
  });

  const historyQuery = useQuery({
    queryKey: ['budget-goal-history'],
    queryFn: () => budgetingApi.getBudgetGoalHistory(),
  });
  const history = historyQuery.data ?? NO_GOALS;

  const span = useMemo(() => averageWindow(), []);
  const reportQuery = useQuery({
    queryKey: [
      'budgeting-report',
      span.start,
      span.end,
      'monthly',
      undefined,
      currencyCode,
      fxRevision,
    ],
    queryFn: () => budgetingApi.getReport(span.start, span.end, 'monthly', undefined, currencyCode),
    enabled: withAverages || sheet.open,
    staleTime: AVERAGE_STALE_MS,
  });
  const report = reportQuery.data;
  const averages = useMemo(() => averagesByCategory(report), [report]);
  const averageCurrency = report?.reportCurrency ?? currencyCode;

  // The categories a new limit can go to: the expense ones without a limit in effect or scheduled
  const budgeted = useMemo(() => budgetedCategoryIds(history, currentMonthStart()), [history]);
  const availableCategories = useMemo(
    () => categories.filter((c) => c.kind === 'expense' && !budgeted.has(c.id)),
    [categories, budgeted]
  );

  // `availableCategories` means something only once the limits are known: while they load (or
  // failed) every expense category looks free
  const canPick = historyQuery.isSuccess && availableCategories.length > 0;
  const openLimit = useCallback(
    (mode: LimitMode) => {
      if (mode.kind === 'add' && mode.categoryId === null && !canPick) return;
      setSheet({ open: true, mode });
    },
    [canPick]
  );

  return {
    /** The props of `BudgetLimitSheet`, to spread. */
    sheetProps: {
      open: sheet.open,
      onOpenChange: (open: boolean) => setSheet((current) => ({ ...current, open })),
      mode: sheet.mode,
      categories,
      availableCategories,
      averages,
      averageCurrency,
    },
    openLimit,
    categoriesQuery,
    /** Every stored goal with its versions; empty until loaded. */
    history,
    historyQuery,
    /** The six-month report the averages come from. */
    reportQuery,
    averages,
    averageCurrency,
    /** Categories whose monthly limit is in effect or scheduled. */
    budgeted,
    availableCategories,
    /** A budget can be added by picking a category: the limits are known and a category is free. */
    canPick,
  };
}
