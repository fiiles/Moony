import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { loansApi } from '@/lib/tauri-api';
import { todayUtcDay } from '@shared/calculations/loan-amortization';
import type { ConvertFn } from '@shared/fx';
import { useMainConvert } from '@/hooks/use-fx';
import { loanMetrics, loanRow, type LoanMetrics, type LoanRow } from '@/utils/loans';

/**
 * Loans with their amortized outstanding balance as of today plus the
 * derived rows (trajectory, repaid share, payoff day) and totals the list page
 * shows, in the main currency at the current quotes (a loan without a rate is left
 * out of the totals and named in `metrics.missing`). The totals and the weighted
 * rate use the outstanding balance, never the original principal. `refetchOnMount: 'always'` because the balance changes with
 * the calendar even when nothing was edited (the global default never refetches).
 */
export function useLoans(): {
  loans: LoanRow['loan'][];
  rows: LoanRow[];
  isLoading: boolean;
  metrics: LoanMetrics;
  today: number;
  convert: ConvertFn;
} {
  const { data: loans = [], isLoading } = useQuery({
    queryKey: ['loans'],
    queryFn: () => loansApi.getAll(),
    refetchOnMount: 'always',
  });
  const today = todayUtcDay();
  const convert = useMainConvert();
  const rows = useMemo(
    () => loans.map((loan) => loanRow(loan, today, convert)),
    [loans, today, convert]
  );
  const metrics = useMemo(() => loanMetrics(rows, today), [rows, today]);
  return { loans, rows, isLoading, metrics, today, convert };
}

/** Repayment schedule and totals of one loan as of today (loan detail page). */
export function useLoanSchedule(id: string | undefined) {
  return useQuery({
    queryKey: ['loan-schedule', id],
    queryFn: () => loansApi.getSchedule(id!),
    enabled: !!id,
    refetchOnMount: 'always',
  });
}
