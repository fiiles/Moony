import { useMutation } from '@tanstack/react-query';
import { queryClient } from '@/lib/queryClient';
import { stockMonitorApi } from '@/lib/tauri-api';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';

export function useStockMonitorMutations() {
  const { t } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');

  // Spec D10: the watchlist never affects net worth — domain keys only.
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['stock-monitor'] });

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const followMutation = useMutation({
    mutationFn: (ticker: string) => stockMonitorApi.follow(ticker),
    onSuccess: (watched) => {
      invalidate();
      toast(t('toast.followed', { ticker: watched.ticker }));
    },
    onError,
  });

  const unfollowMutation = useMutation({
    mutationFn: (ticker: string) => stockMonitorApi.unfollow(ticker),
    onSuccess: (_data, ticker) => {
      invalidate();
      toast(t('toast.unfollowed', { ticker }));
    },
    onError,
  });

  const targetPriceMutation = useMutation({
    mutationFn: ({ ticker, targetPrice }: { ticker: string; targetPrice: string | null }) =>
      stockMonitorApi.setTargetPrice(ticker, targetPrice),
    onSuccess: () => {
      invalidate();
      toast(tc('status.success'));
    },
    onError,
  });

  const notesMutation = useMutation({
    mutationFn: ({ ticker, notes }: { ticker: string; notes: string }) =>
      stockMonitorApi.updateNotes(ticker, notes),
    onSuccess: () => {
      invalidate();
      toast(tc('status.success'));
    },
    onError,
  });

  // Force-refresh a single ticker (detail page) — works for unfollowed tickers too
  const refreshDetailMutation = useMutation({
    mutationFn: (ticker: string) => stockMonitorApi.getDetail(ticker, true),
    onSuccess: () => invalidate(),
    onError,
  });

  const followPortfolioMutation = useMutation({
    mutationFn: () => stockMonitorApi.followPortfolio(),
    onSuccess: (added) => {
      invalidate();
      if (added.length > 0) {
        toast(t('toast.followedPortfolio', { count: added.length }));
      }
    },
    onError,
  });

  const refreshMutation = useMutation({
    mutationFn: () => stockMonitorApi.refreshPrices(true),
    onSuccess: () => invalidate(),
    onError,
  });

  return {
    followMutation,
    unfollowMutation,
    targetPriceMutation,
    notesMutation,
    refreshMutation,
    refreshDetailMutation,
    followPortfolioMutation,
  };
}
