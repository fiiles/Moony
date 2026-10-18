import { useEffect, useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import type {
  InvestmentTransaction,
  StockCompanyInfo,
  StockPosition,
  TickerValueHistoryReport,
} from '@shared/schema';
import { useTransactionFx } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { investmentsApi, portfolioApi, stockMonitorApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { toStockRow, type StockRow } from '@/utils/positions';

/**
 * Company data moves slowly and the backend asks Yahoo at most once a day, so within the hour a
 * repeat visit reuses what the card already has.
 */
const COMPANY_INFO_STALE_TIME_MS = 60 * 60 * 1000;

/** Query keys (prefix-matched) a deleted position leaves stale, besides the metrics and the cashflow. */
const DELETE_KEYS = [
  ['investments'],
  ['all-stock-transactions'],
  // The position's imported trades are gone: the history's counts change.
  ['stock-import-batches'],
] as const;

export interface PositionData {
  /** The backend's valuation of the position in the report currency; `undefined` until loaded. */
  position: StockPosition | undefined;
  /** The position as the trade and price dialogs take it (`toStockRow`); `null` until loaded or when gone. */
  holding: StockRow | null;
  reportCurrency: string;
  /** Every trade of the position, in the order the backend gives them. */
  transactions: InvestmentTransaction[];
  /** Converts a trade at the rates of its own day; `null` without a rate and until `ratesReady`. */
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
  /** The value and cost line of `period`, each day converted into the report currency. */
  history: TickerValueHistoryReport | undefined;
  /** What Yahoo reported about the company (the stored record shows at once, the fresh one replaces it). */
  companyInfo: StockCompanyInfo | undefined;
  /** The company data has not answered yet (loading, or a retry on its way). */
  companyPending: boolean;
  refetchCompany: () => void;
  isLoading: boolean;
  /** Loaded, and there is no such position (a deep link to a deleted one). */
  notFound: boolean;
}

/**
 * Everything the position detail shows (spec §8.8), from the queries of the desktop's
 * `StockDetail` and `PositionHero` so the cache is shared: the position in the report currency,
 * its trades, the value history of the period (refetched when the backend finishes a
 * recalculation, and backfilled once for a position imported without history) and the company
 * data in two steps.
 */
export function usePosition(id: string | undefined, period: ChartPeriod): PositionData {
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision } = useCurrency();

  const { data: position, isLoading } = useQuery<StockPosition | null>({
    queryKey: ['investment', id, currencyCode, fxRevision],
    queryFn: () => investmentsApi.get(id!, currencyCode),
    enabled: !!id,
  });
  const investment = position?.investment;
  const ticker = investment?.ticker;
  const holding = useMemo(() => (investment ? toStockRow(investment) : null), [investment]);

  const { data: transactionData } = useQuery<InvestmentTransaction[]>({
    queryKey: ['investment-transactions', id],
    queryFn: () => investmentsApi.getTransactions(id!),
    enabled: !!id,
  });
  const transactions = useMemo(() => transactionData ?? [], [transactionData]);
  // Each trade converts at the rates of its own day; the hero takes everything from the backend.
  const { ready: ratesReady, convertAt } = useTransactionFx(transactionData);

  useEffect(() => {
    if (!ticker) return;
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['ticker-history', 'stock', ticker] });
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [queryClient, ticker]);

  // On-demand backfill, once per ticker, so the chart has history after a fresh import
  useEffect(() => {
    if (!ticker) return;
    investmentsApi
      .backfillHistory(ticker)
      .then((result) => {
        if (result.days_processed > 0) {
          queryClient.invalidateQueries({ queryKey: ['ticker-history', 'stock', ticker] });
        }
      })
      .catch((error) => console.error(`[Chart] Backfill failed for ${ticker}:`, error));
  }, [queryClient, ticker]);

  // First UTC day of the period (open for "All"): it only changes with the day, so it keys the query
  const periodStart = chartPeriodStart(period, Date.now() / 1000);
  const { data: history } = useQuery<TickerValueHistoryReport>({
    queryKey: ['ticker-history', 'stock', ticker, periodStart ?? 'all', currencyCode, fxRevision],
    queryFn: () =>
      investmentsApi.getHistory(ticker!, currencyCode, periodStart, Math.floor(Date.now() / 1000)),
    enabled: !!ticker,
    staleTime: 0,
    refetchOnMount: 'always',
  });

  // Company data comes from Yahoo through the stock_data cache. The stored record shows at once;
  // the second query lets the backend fetch it first when it is missing or over a day old, so a
  // slow Yahoo answer never holds back what is already known.
  const storedCompanyInfo = useQuery<StockCompanyInfo>({
    queryKey: ['stock-company-info', ticker, 'stored'],
    queryFn: () => investmentsApi.getCompanyInfo(ticker!, false),
    enabled: !!ticker,
    staleTime: COMPANY_INFO_STALE_TIME_MS,
  });
  const freshCompanyInfo = useQuery<StockCompanyInfo>({
    queryKey: ['stock-company-info', ticker, 'fresh'],
    queryFn: () => investmentsApi.getCompanyInfo(ticker!, true),
    enabled: !!ticker,
    staleTime: COMPANY_INFO_STALE_TIME_MS,
  });

  return {
    position: position ?? undefined,
    holding,
    reportCurrency: position?.currency ?? currencyCode,
    transactions,
    convertAt,
    ratesReady,
    history,
    companyInfo: freshCompanyInfo.data ?? storedCompanyInfo.data,
    companyPending: freshCompanyInfo.isPending || freshCompanyInfo.isFetching,
    refetchCompany: () => void freshCompanyInfo.refetch(),
    isLoading,
    notFound: !isLoading && !investment,
  };
}

/**
 * The position's writes: delete (the mutation contract — the domain keys, the metrics and the
 * cashflow, then a fresh snapshot and the history), rename and follow. The caller leaves the
 * screen when a delete has succeeded and then drops the position's own queries (`PositionDetail`:
 * removing them while the screen is still up would rebuild them empty).
 */
export function usePositionMutations(id: string | undefined, ticker: string | undefined) {
  const queryClient = useQueryClient();
  const { currencyCode } = useCurrency();
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const remove = useMutation({
    mutationFn: () => investmentsApi.delete(id!),
    onSuccess: async () => {
      for (const queryKey of DELETE_KEYS) queryClient.invalidateQueries({ queryKey });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // The delete already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(ts('toast.deleted'));
    },
    onError,
  });

  const rename = useMutation({
    mutationFn: (name: string) => investmentsApi.updateName(id!, name, currencyCode),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investment', id] });
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      toast(t('wealth.position.renamed'));
    },
    onError,
  });

  const follow = useMutation({
    mutationFn: () => stockMonitorApi.follow(ticker!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['stock-monitor'] });
      toast(t('wealth.position.followed'));
    },
    onError,
  });

  return { remove, rename, follow };
}
