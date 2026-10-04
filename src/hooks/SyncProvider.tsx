import { useState, useEffect, useRef, useCallback, ReactNode } from 'react';
import { portfolioApi, priceApi } from '@/lib/tauri-api';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { McpDataChangedPayload } from '@shared/schema';
import { SyncContext } from './sync-context';

export function SyncProvider({ children }: { children: ReactNode }) {
  const [isSyncing, setIsSyncing] = useState(false);
  const [progress, setProgress] = useState({ current: 0, total: 0 });
  const [lastResult, setLastResult] = useState<Awaited<
    ReturnType<typeof portfolioApi.startBackfill>
  > | null>(null);
  const isSyncingRef = useRef(false);
  const hasRun = useRef(false);
  const queryClient = useQueryClient();

  const startBackfill = useCallback(async () => {
    // Use ref to prevent concurrent runs
    if (isSyncingRef.current) return;

    isSyncingRef.current = true;

    try {
      setIsSyncing(true);
      setProgress({ current: 0, total: 0 });

      const result = await portfolioApi.startBackfill();

      setProgress({
        current: result.days_processed,
        total: result.total_days,
      });
      setLastResult(result);

      // If we processed any days, invalidate portfolio queries to refresh dashboard
      if (result.days_processed > 0) {
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
        queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      }

      // Backfill native currency breakdowns for existing snapshots (safe to re-run)
      try {
        await portfolioApi.backfillCurrencyBreakdowns();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (err) {
        console.warn('[Sync] Currency breakdown backfill failed (non-critical):', err);
      }
    } catch (error) {
      console.error('[Sync] Backfill failed:', error);
    } finally {
      isSyncingRef.current = false;
      setIsSyncing(false);
    }
  }, [queryClient]);

  // Auto-run on mount (after login): refresh stale prices then backfill history
  useEffect(() => {
    if (hasRun.current) return;
    hasRun.current = true;

    const runStartupSync = async () => {
      // Step 1: refresh any stale prices so charts and values are up to date
      try {
        const status = await portfolioApi.getPriceStatus();
        const needsRefresh = status.stocksStale || status.cryptoStale || status.exchangeRatesStale;

        if (needsRefresh) {
          await Promise.allSettled([
            status.exchangeRatesStale ? portfolioApi.refreshExchangeRates() : Promise.resolve(),
            status.stocksStale ? priceApi.refreshStockPrices() : Promise.resolve(),
            status.cryptoStale ? priceApi.refreshCryptoPrices() : Promise.resolve(),
          ]);
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['crypto'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
          queryClient.invalidateQueries({ queryKey: ['price-status'] });
        }
      } catch (error) {
        console.error('[Sync] Auto price refresh failed:', error);
      }

      // Step 2: backfill missing historical snapshots (prices are now fresh)
      await startBackfill();
    };

    const timer = setTimeout(runStartupSync, 5000);

    return () => clearTimeout(timer);
  }, [startBackfill, queryClient]);

  // Record today's snapshot (called after asset changes)
  const recordTodaySnapshot = useCallback(async () => {
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    } catch (error) {
      console.error('[Sync] Failed to record snapshot:', error);
    }
  }, [queryClient]);

  // Refresh after MCP write tools change data (rule-7 ritual, driven by the
  // mcp-data-changed event the embedded server emits). Query keys verified
  // against the real hooks/pages (grep for `queryKey: ['`) as of this writing.
  useEffect(() => {
    const domainKeys: Record<string, string[][]> = {
      bank: [
        ['bank-accounts'],
        ['bank-account'],
        ['bank-transactions'],
        ['transaction-categories'],
      ],
      stocks: [
        ['investments'],
        ['investment'],
        ['investment-transactions'],
        ['transactions'],
        ['all-stock-transactions'],
        ['dividend-summary'],
        ['stocks-analysis'],
      ],
      crypto: [['crypto'], ['crypto-detail'], ['crypto-transactions'], ['all-crypto-transactions']],
      bonds: [['bonds']],
      loans: [['loans'], ['loan-schedule'], ['real-estate-loans'], ['available-loans']],
      'real-estate': [['real-estate']],
      'other-assets': [['other-assets'], ['other-asset-transactions']],
      insurance: [['insurance']],
      // Stock Monitor watchlist: rows only — see the net-worth guard below
      'stock-monitor': [['stock-monitor']],
    };

    const unlisten = listen<McpDataChangedPayload>('mcp-data-changed', async (event) => {
      const { domain, tickers, earliestDate } = event.payload;

      for (const key of domainKeys[domain] ?? []) {
        queryClient.invalidateQueries({ queryKey: key });
      }

      // Following a stock, or noting a target on it, changes nothing about what
      // the user owns — so the watchlist skips the portfolio/snapshot ritual
      // (spec D10 of the Stock Monitor design, same deviation the UI makes).
      const affectsNetWorth = domain !== 'stock-monitor';
      if (affectsNetWorth) {
        queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });

        try {
          await portfolioApi.recordSnapshot();
          queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
        } catch (error) {
          console.error('[Sync] MCP refresh: snapshot failed:', error);
        }
      }

      // Only market-priced domains need a price/dividend refresh.
      const isPricedDomain = domain === 'stocks' || domain === 'crypto';

      if (isPricedDomain) {
        // Same fire-and-forget refresh the Add modals use — new holdings have
        // no current price until this runs.
        const refresh =
          domain === 'stocks' ? priceApi.refreshStockPrices() : priceApi.refreshCryptoPrices();
        refresh
          .then(() => {
            queryClient.invalidateQueries({
              queryKey: [domain === 'stocks' ? 'investments' : 'crypto'],
            });
            queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });

            if (domain === 'stocks') {
              // Newly imported tickers have no dividend data until this runs
              // too (mirrors ImportInvestmentsModal's background refresh chain).
              return priceApi.refreshDividends().then(() => {
                queryClient.invalidateQueries({ queryKey: ['investments'] });
                queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
              });
            }
          })
          .then(() => {
            // Prices (and, for stocks, dividends) are fresh now — re-record
            // today's snapshot so a same-day import of a brand-new ticker
            // isn't left undervalued until the next mutation (recordSnapshot
            // is an idempotent upsert; record_todays_ticker_values skips
            // tickers that had no price yet, which is why this re-run matters).
            portfolioApi
              .recordSnapshot()
              .then(() => queryClient.invalidateQueries({ queryKey: ['portfolio-history'] }))
              .catch((error) =>
                console.error('[Sync] MCP refresh: re-record snapshot failed:', error)
              );
          })
          .catch((error) => console.error('[Sync] MCP refresh: price refresh failed:', error));
      }

      // Retroactive imports leave the portfolio history stale until it is recalculated.
      // Priced domains recalculate per ticker, so an empty ticker list means
      // there is nothing to recalculate; other assets have no tickers at all and
      // are recalculated as a whole asset class, so the earliest date is the only
      // condition (the Rust command's `_` arm routes it to AssetType::OtherAssets).
      if (isPricedDomain || domain === 'other-assets') {
        // Local midnight, not the backend's UTC day boundary — the mismatch
        // (at most the local UTC offset) only ever widens the window and
        // fires an extra, idempotent recalculation; it never suppresses one
        // that's actually needed.
        const todayStart = Math.floor(new Date().setHours(0, 0, 0, 0) / 1000);
        const isRetroactive = earliestDate !== null && earliestDate < todayStart;
        const hasScope = isPricedDomain ? tickers.length > 0 : true;
        if (isRetroactive && hasScope) {
          portfolioApi
            .triggerImportRecalculation(domain, tickers, earliestDate)
            .catch((error) =>
              console.error('[Sync] MCP refresh: historical recalculation failed:', error)
            );
        }
      }
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  return (
    <SyncContext.Provider
      value={{ isSyncing, progress, lastResult, startBackfill, recordTodaySnapshot }}
    >
      {children}
    </SyncContext.Provider>
  );
}
