import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { STOCK_TRANSACTION_KEYS } from '@/hooks/use-investment-mutations';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { portfolioApi, priceApi, stockSplitsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { StockSplitApplyResult } from '@shared/schema';

/**
 * Apply, revert, dismiss, record and delete a stock split (spec section 4.2). Apply, revert
 * and record move transactions, so they follow the mutation contract: domain keys +
 * portfolio-metrics + cashflow-report, then a fresh snapshot and portfolio-history
 * (reference: use-bank-account-mutations.ts), then the forced price refresh of D21 so the
 * cached quote is not a pre-split one. Dismiss and delete only change the split rows.
 */
export function useStockSplitMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');

  const refreshAfterTransactionChange = async () => {
    for (const queryKey of STOCK_TRANSACTION_KEYS) {
      queryClient.invalidateQueries({ queryKey });
    }
    queryClient.invalidateQueries({ queryKey: ['stock-splits'] });
    queryClient.invalidateQueries({ queryKey: ['milestones'] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot or quote refresh must not turn it into an
    // error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
    try {
      await priceApi.refreshStockPrices(true);
      await invalidateAfterPriceRefresh(queryClient);
    } catch (error) {
      console.error('Failed to refresh stock prices after a split:', error);
    }
  };

  const refreshSplitRows = () => {
    queryClient.invalidateQueries({ queryKey: ['stock-splits'] });
    queryClient.invalidateQueries({ queryKey: ['milestones'] });
  };

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const ratioOf = ({ split }: StockSplitApplyResult) => `${split.numerator}:${split.denominator}`;

  const revert = useMutation({
    mutationFn: (id: string) => stockSplitsApi.revert(id),
    onSuccess: async (result) => {
      await refreshAfterTransactionChange();
      toast(tc('status.success'), {
        description: t('splits.toast.reverted', {
          ticker: result.split.ticker,
          count: result.rewritten,
          ratio: ratioOf(result),
        }),
      });
    },
    onError,
  });

  const apply = useMutation({
    mutationFn: (id: string) => stockSplitsApi.apply(id),
    onSuccess: async (result) => {
      await refreshAfterTransactionChange();
      toast(tc('status.success'), {
        description: t('splits.toast.applied', {
          ticker: result.split.ticker,
          count: result.rewritten,
          ratio: ratioOf(result),
        }),
        action: { label: t('splits.undo'), onClick: () => revert.mutate(result.split.id) },
      });
    },
    onError,
  });

  const record = useMutation({
    mutationFn: stockSplitsApi.record,
    onSuccess: async (result) => {
      await refreshAfterTransactionChange();
      toast(tc('status.success'), {
        description: t('splits.toast.recorded', {
          ticker: result.split.ticker,
          count: result.rewritten,
          ratio: ratioOf(result),
        }),
      });
    },
    onError,
  });

  const dismiss = useMutation({
    mutationFn: (id: string) => stockSplitsApi.dismiss(id),
    onSuccess: (split) => {
      refreshSplitRows();
      toast(tc('status.success'), {
        description: t('splits.toast.dismissed', {
          ticker: split.ticker,
          ratio: `${split.numerator}:${split.denominator}`,
        }),
      });
    },
    onError,
  });

  const remove = useMutation({
    mutationFn: (id: string) => stockSplitsApi.remove(id),
    onSuccess: () => {
      refreshSplitRows();
      toast(tc('status.success'), { description: t('splits.toast.deleted') });
    },
    onError,
  });

  return { apply, revert, dismiss, record, remove };
}
