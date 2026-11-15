import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { AssetValuation, CryptoPositions, OtherAsset, RealEstate } from '@shared/schema';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useBonds } from '@/hooks/use-bonds';
import { useNetWorthOverview } from '@/hooks/use-net-worth-overview';
import { useAllRealEstateValuations } from '@/hooks/use-real-estate';
import { useCurrency } from '@/lib/currency';
import { cryptoApi, otherAssetsApi, realEstateApi } from '@/lib/tauri-api';
import { toCryptoRow } from '@/utils/positions';
import { tickerSummary } from '@/utils/trade-events';
import { useValuedDays } from '@/mobile/hooks/use-other-asset';
import { useStockPositions } from '@/mobile/hooks/use-stock-positions';
import { periodChange, type PeriodChange } from '@/mobile/lib/change';
import {
  propertyChange,
  topTickers,
  visibleClasses,
  type InvestmentClass,
  type WealthClass,
} from '@/mobile/lib/wealth-rows';
import { movesWithRates } from '@/mobile/lib/money-rule';

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
  /**
   * Per class: `count` is what the row names (open positions, live issues), `records` is every
   * record of the class, closed or matured too — what decides whether the class is empty.
   */
  stocks: {
    count: number;
    records: number;
    tickers: string;
    fxMissing: string[];
    loading: boolean;
  };
  crypto: { count: number; records: number; tickers: string; loading: boolean };
  bonds: {
    count: number;
    records: number;
    nearestMaturity: number | null;
    yearlyCoupon: number;
    loading: boolean;
  };
  otherAssets: {
    count: number;
    records: number;
    names: string;
    valuedAt: number | null;
    loading: boolean;
  };
  properties: { property: RealEstate; change: { ratio: number | null; day: number } | null }[];
  /** Something is held in another currency than the report's: the total moves with rates (A8). */
  movesWithRates: boolean;
  /** The properties' list is still being read: the section stands in outline, never missing (E1). */
  propertiesLoading: boolean;
  cash: { accounts: number; loading: boolean };
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

  // The query of the Akcie page: one definition, one cache entry
  const {
    positions: stockPositions,
    rows: stockRows,
    isLoading: stocksLoading,
  } = useStockPositions();
  const { data: cryptoPositions, isLoading: cryptoLoading } = useQuery<CryptoPositions>({
    queryKey: ['crypto', currencyCode, fxRevision],
    queryFn: () => cryptoApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });
  const { bonds: bondRecords, metrics: bondMetrics, isLoading: bondsLoading } = useBonds();
  const { data: otherAssets = [], isLoading: otherLoading } = useQuery<OtherAsset[]>({
    queryKey: ['other-assets'],
    queryFn: () => otherAssetsApi.getAll(),
  });
  // The same key and query as the desktop's Nemovitosti page
  const { data: properties = [], isLoading: propertiesLoading } = useQuery<RealEstate[]>({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
  });
  const { data: valuations = [] } = useAllRealEstateValuations();
  const { accounts, isLoading: accountsLoading } = useBankAccounts();

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
  // The newest day any asset was valued (not touched: a buy moves `updatedAt`, not the price)
  const valuedDays = useValuedDays(otherAssets);
  const newestUpdate = [...valuedDays.values()].reduce<number | null>(
    (newest, day) => (newest === null || day > newest ? day : newest),
    null
  );

  return {
    overview,
    reportCurrency,
    classes,
    stocks: {
      count: openStocks.length,
      records: stockRows.length,
      tickers: topTickers(
        stockRows.map((row) => ({
          ticker: row.ticker,
          quantity: row.quantity,
          value: row.marketValue,
        })),
        TICKERS_SHOWN
      ),
      fxMissing: stockPositions?.fxQuality.missingCurrencies ?? [],
      loading: stocksLoading,
    },
    crypto: {
      count: openCrypto.length,
      records: cryptoRows.length,
      tickers: topTickers(
        cryptoRows.map((row) => ({
          ticker: row.ticker,
          quantity: row.quantity,
          value: row.marketValue,
        })),
        TICKERS_SHOWN
      ),
      loading: cryptoLoading,
    },
    bonds: {
      count: bondMetrics.liveCount,
      records: bondRecords.length,
      nearestMaturity: bondMetrics.nearest?.maturityDate ?? null,
      yearlyCoupon: bondMetrics.projectedYearlyIncome,
      loading: bondsLoading,
    },
    otherAssets: {
      count: otherAssets.length,
      records: otherAssets.length,
      names: tickerSummary(
        otherAssets.map((asset) => asset.name),
        NAMES_SHOWN
      ),
      valuedAt: newestUpdate,
      loading: otherLoading,
    },
    properties: propertyRows,
    movesWithRates: movesWithRates(
      [
        ...accounts.filter((account) => !account.excludeFromBalance).map((a) => a.currency),
        ...stockRows.filter((row) => row.quantity > 0).map((row) => row.originalCurrency),
        ...openCrypto.map((row) => row.originalCurrency),
        ...properties.map((property) => property.marketPriceCurrency),
        ...otherAssets.map((asset) => asset.marketPriceCurrency),
      ],
      reportCurrency
    ),
    propertiesLoading,
    cash: {
      accounts: accounts.filter((account) => !account.excludeFromBalance).length,
      loading: accountsLoading,
    },
    visible: visibleClasses(user?.menuPreferences ?? { investments: true, bonds: true }),
    isLoading: overview.isCheckingPortfolio,
  };
}
