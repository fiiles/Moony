import { useMutation } from '@tanstack/react-query';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { queryClient } from '@/lib/queryClient';
import { stockMonitorApi } from '@/lib/tauri-api';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import type { TargetDirection } from '@shared/schema';

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
      // Following fetches the ticker's price, shared with a held position of it
      void invalidateAfterPriceRefresh(queryClient);
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
    mutationFn: ({
      ticker,
      targetPrice,
      targetDirection,
    }: {
      ticker: string;
      targetPrice: string | null;
      targetDirection: TargetDirection | null;
    }) => stockMonitorApi.setTargetPrice(ticker, targetPrice, targetDirection),
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
    onSuccess: () => {
      invalidate();
      // The forced price is shared with a held position of the ticker
      void invalidateAfterPriceRefresh(queryClient);
    },
    onError,
  });

  const followPortfolioMutation = useMutation({
    mutationFn: () => stockMonitorApi.followPortfolio(),
    onSuccess: (added) => {
      invalidate();
      if (added.length > 0) {
        // Following the held tickers fetches their prices
        void invalidateAfterPriceRefresh(queryClient);
        toast(t('toast.followedPortfolio', { count: added.length }));
      }
    },
    onError,
  });

  const refreshMutation = useMutation({
    mutationFn: () => stockMonitorApi.refreshPrices(true),
    onSuccess: (result) => {
      invalidate();
      // A followed ticker can be a held one: its new price moves the portfolio too
      if (result.updated.length > 0) void invalidateAfterPriceRefresh(queryClient);
    },
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
