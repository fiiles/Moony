import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { investmentsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertStockDividend } from '@shared/schema';

/**
 * Query keys (prefix-matched) that show received dividends or anything a position's page
 * derives next to them. A dividend changes no quantity or value, but the mutation contract is
 * uniform (AGENTS rule 7), and the performance views will read dividends (GIPS spec).
 */
const STOCK_DIVIDEND_KEYS = [
  ['stock-dividends'],
  ['investments'],
  ['investment'],
  ['investment-transactions'],
  ['stocks-analysis'],
  ['stock-performance'],
] as const;

/**
 * Record, edit and delete received dividends of a stock position: domain keys + portfolio
 * metrics + cashflow report, then a fresh snapshot and the portfolio history (reference:
 * use-bank-account-mutations.ts).
 */
export function useStockDividendMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');

  const refreshAfterChange = async () => {
    for (const queryKey of STOCK_DIVIDEND_KEYS) {
      queryClient.invalidateQueries({ queryKey });
    }
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot must not turn it into an error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const createDividend = useMutation({
    mutationFn: ({ investmentId, data }: { investmentId: string; data: InsertStockDividend }) =>
      investmentsApi.createDividend(investmentId, data),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tc('status.success'), { description: t('detail.dividendRecords.added') });
    },
    onError,
  });

  const updateDividend = useMutation({
    mutationFn: ({ id, data }: { id: string; data: InsertStockDividend }) =>
      investmentsApi.updateDividend(id, data),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tc('status.success'), { description: t('detail.dividendRecords.updated') });
    },
    onError,
  });

  const deleteDividend = useMutation({
    mutationFn: (id: string) => investmentsApi.deleteDividend(id),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tc('status.success'), { description: t('detail.dividendRecords.deleted') });
    },
    onError,
  });

  return { createDividend, updateDividend, deleteDividend };
}
