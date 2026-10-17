import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  AssetValuation,
  CryptoPositions,
  OtherAsset,
  RealEstate,
  StockPositions,
} from '@shared/schema';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useBonds } from '@/hooks/use-bonds';
import { useNetWorthOverview } from '@/hooks/use-net-worth-overview';
import { useAllRealEstateValuations } from '@/hooks/use-real-estate';
import { useCurrency } from '@/lib/currency';
import { cryptoApi, investmentsApi, otherAssetsApi, realEstateApi } from '@/lib/tauri-api';
import { toCryptoRow, toStockRow } from '@/utils/positions';
import { tickerSummary } from '@/utils/trade-events';
import { periodChange, type PeriodChange } from '@/mobile/lib/change';
import {
  propertyChange,
  topTickers,
  visibleClasses,
  type InvestmentClass,
  type WealthClass,
} from '@/mobile/lib/wealth-rows';

/** Tickers named in a class row's sub-line before "+N". */
const TICKERS_SHOWN = 3;
/** Names of other assets named in a row's sub-line before "+N". */
const NAMES_SHOWN = 2;

/** The key of a class in `portfolioMetrics`: `null` there means held, but nothing converted. */
const TOTAL_KEY = {
  savings: 'totalSavings',
  investments: 'totalInvestments',
  bonds: 'totalBonds',
  crypto: 'totalCrypto',
  otherAssets: 'totalOtherAssets',
  realEstate: 'totalRealEstate',
} as const satisfies Record<WealthClass, string>;

const WEALTH_CLASSES = Object.keys(TOTAL_KEY) as WealthClass[];

export interface WealthData {
  overview: ReturnType<typeof useNetWorthOverview>;
  reportCurrency: string;
  /** Per class: today's value (0 while unknown), whether it is unknown (held, none converted), and the 30-day change. */
  classes: Record<WealthClass, { value: number; unknown: boolean; change: PeriodChange | null }>;
  stocks: { count: number; tickers: string; fxMissing: string[] };
  crypto: { count: number; tickers: string };
  bonds: { count: number; nearestMaturity: number | null; yearlyCoupon: number };
  otherAssets: { count: number; names: string; valuedAt: number | null };
  properties: { property: RealEstate; change: { ratio: number | null; day: number } | null }[];
  cash: { accounts: number };
  visible: InvestmentClass[];
  isLoading: boolean;
}

/**
 * Everything Majetek shows (spec §8.8), from the queries the desktop pages use so the cache is
 * shared: the class totals and their 30-day base from `useNetWorthOverview('30D')`, the list
 * queries for the counts and names under each class row, the revaluations behind a property's
 * change. The values are the backend's, in the report currency; a class it could not convert is
 * `unknown`, never 0.
 */
export function useWealth(): WealthData {
  const { user } = useAuth();
  const { currencyCode, fxRevision } = useCurrency();
  const overview = useNetWorthOverview('30D');
  const { portfolioMetrics, display, hasHistory, oldest, reportCurrency } = overview;

  const { data: stockPositions } = useQuery<StockPositions>({
    queryKey: ['investments', currencyCode, fxRevision],
    queryFn: () => investmentsApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });
  const { data: cryptoPositions } = useQuery<CryptoPositions>({
    queryKey: ['crypto', currencyCode, fxRevision],
    queryFn: () => cryptoApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });
  const { metrics: bondMetrics } = useBonds();
  const { data: otherAssets = [] } = useQuery<OtherAsset[]>({
    queryKey: ['other-assets'],
    queryFn: () => otherAssetsApi.getAll(),
  });
  // The same key and query as the desktop's Nemovitosti page
  const { data: properties = [] } = useQuery<RealEstate[]>({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
  });
  const { data: valuations = [] } = useAllRealEstateValuations();
  const { accounts } = useBankAccounts();

  const classes = Object.fromEntries(
    WEALTH_CLASSES.map((cls) => {
      const unknown = portfolioMetrics?.[TOTAL_KEY[cls]] === null;
      return [
        cls,
        {
          value: display[cls],
          unknown,
          change: hasHistory ? periodChange(unknown ? null : display[cls], oldest?.[cls]) : null,
        },
      ];
    })
  ) as WealthData['classes'];

  const stockRows = useMemo(() => (stockPositions?.items ?? []).map(toStockRow), [stockPositions]);
  const cryptoRows = useMemo(
    () => (cryptoPositions?.items ?? []).map(toCryptoRow),
    [cryptoPositions]
  );
  const propertyRows = useMemo(() => {
    const byProperty = new Map<string, AssetValuation[]>();
    for (const valuation of valuations) {
      const own = byProperty.get(valuation.assetId);
      if (own) own.push(valuation);
      else byProperty.set(valuation.assetId, [valuation]);
    }
    return properties.map((property) => ({
      property,
      change: propertyChange(byProperty.get(property.id) ?? []),
    }));
  }, [properties, valuations]);

  const openStocks = stockRows.filter((row) => row.quantity > 0);
  const openCrypto = cryptoRows.filter((row) => row.quantity > 0);
  const newestUpdate = otherAssets.reduce<number | null>(
    (newest, asset) => (newest === null || asset.updatedAt > newest ? asset.updatedAt : newest),
    null
  );

  return {
    overview,
    reportCurrency,
    classes,
    stocks: {
      count: openStocks.length,
      tickers: topTickers(
        stockRows.map((row) => ({
          ticker: row.ticker,
          quantity: row.quantity,
          value: row.marketValue,
        })),
        TICKERS_SHOWN
      ),
      fxMissing: stockPositions?.fxQuality.missingCurrencies ?? [],
    },
    crypto: {
      count: openCrypto.length,
      tickers: topTickers(
        cryptoRows.map((row) => ({
          ticker: row.ticker,
          quantity: row.quantity,
          value: row.marketValue,
        })),
        TICKERS_SHOWN
      ),
    },
    bonds: {
      count: bondMetrics.liveCount,
      nearestMaturity: bondMetrics.nearest?.maturityDate ?? null,
      yearlyCoupon: bondMetrics.projectedYearlyIncome,
    },
    otherAssets: {
      count: otherAssets.length,
      names: tickerSummary(
        otherAssets.map((asset) => asset.name),
        NAMES_SHOWN
      ),
      valuedAt: newestUpdate,
    },
    properties: propertyRows,
    cash: { accounts: accounts.filter((account) => !account.excludeFromBalance).length },
    visible: visibleClasses(user?.menuPreferences ?? { investments: true, bonds: true }),
    isLoading: overview.isCheckingPortfolio,
  };
}
