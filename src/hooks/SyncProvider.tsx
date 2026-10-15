import { MONEY_FLOW_KEYS } from '@/lib/money-flow-refresh';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useState, useEffect, useRef, useCallback, ReactNode } from 'react';
import { portfolioApi, priceApi } from '@/lib/tauri-api';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { McpDataChangedPayload } from '@shared/schema';
import { SyncContext } from './sync-context';
import { useAuth } from './use-auth';
import { FX_HISTORY_KEY, FX_SNAPSHOT_KEY } from './use-fx';

export function SyncProvider({ children }: { children: ReactNode }) {
  const [isSyncing, setIsSyncing] = useState(false);
  const [progress, setProgress] = useState({ current: 0, total: 0 });
  const [lastResult, setLastResult] = useState<Awaited<
    ReturnType<typeof portfolioApi.startBackfill>
  > | null>(null);
  const isSyncingRef = useRef(false);
  const queryClient = useQueryClient();
  const { appStatus } = useAuth();

  const startBackfill = useCallback(async () => {
    // Use ref to prevent concurrent runs
    if (isSyncingRef.current) return;

    isSyncingRef.current = true;

    try {
      setIsSyncing(true);
      setProgress({ current: 0, total: 0 });

      // The command plans and queues what the start needs (the one-time rebuild of the whole
      // history, else the missing days) and returns at once; the rebuild runs in the background
      // and `recalculation-complete` (below) refetches the views when a batch is done.
      const result = await portfolioApi.startBackfill();

      setProgress({
        current: result.days_processed,
        total: result.total_days,
      });
      setLastResult(result);
    } catch (error) {
      console.error('[Sync] Backfill failed:', error);
    } finally {
      isSyncingRef.current = false;
      setIsSyncing(false);
    }
  }, []);

  // Auto-run after every unlock: refresh stale prices then queue the history rebuild. Every
  // command fails while the database is locked, and the provider mounts before the lock screen
  // is answered, so a timer from mount would fire against a locked database and the one-time
  // rebuild would never run.
  useEffect(() => {
    if (appStatus !== 'unlocked') return;

    const runStartupSync = async () => {
      // Step 1: refresh any stale prices so charts and values are up to date
      try {
        const status = await portfolioApi.getPriceStatus();
        const needsRefresh = status.stocksStale || status.cryptoStale || status.exchangeRatesStale;

        if (needsRefresh) {
          await Promise.allSettled([
            status.exchangeRatesStale
              ? portfolioApi.refreshExchangeRates().then(() => {
                  queryClient.invalidateQueries({ queryKey: FX_SNAPSHOT_KEY });
                  queryClient.invalidateQueries({ queryKey: FX_HISTORY_KEY });
                })
              : Promise.resolve(),
            status.stocksStale ? priceApi.refreshStockPrices() : Promise.resolve(),
            status.cryptoStale ? priceApi.refreshCryptoPrices() : Promise.resolve(),
          ]);
          // Fresh prices change every position value, list or detail, and what is built on them
          void invalidateAfterPriceRefresh(queryClient);
          queryClient.invalidateQueries({ queryKey: ['price-status'] });
        }
      } catch (error) {
        console.error('[Sync] Auto price refresh failed:', error);
      }

      // Step 2: queue the history rebuild, the whole history once and later the missing days
      // (prices are now fresh)
      await startBackfill();
    };

    const timer = setTimeout(runStartupSync, 5000);

    return () => clearTimeout(timer);
  }, [appStatus, startBackfill, queryClient]);

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

  // The background conversion of plain attachment files (ADR 0017) moves rows from their plain
  // file to the encrypted store: refresh its status and the attachment lists when it ends.
  useEffect(() => {
    const unlisten = listen('attachments-conversion-finished', () => {
      for (const key of [
        'attachment-status',
        'real-estate-photo-batches',
        'real-estate-documents',
        'insurance-documents',
      ]) {
        queryClient.invalidateQueries({ queryKey: [key] });
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  // The FX backfill (after a rate refresh or a statement import) can add
  // historical rates: rows previously reported as missing FX now convert, and
  // costs estimated at today's rate get their day's rate. The rate refresh
  // returns before the backfill finishes, so this is the moment to refetch.
  useEffect(() => {
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: FX_HISTORY_KEY });
      for (const key of [
        'budgeting-report',
        'budgeting-category-transactions',
        'cashflow-report',
      ]) {
        queryClient.invalidateQueries({ queryKey: [key] });
      }
      // Positions, analyses and histories convert at the historical rates too
      void invalidateAfterPriceRefresh(queryClient);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  // Refresh after MCP write tools change data (rule-7 ritual, driven by the
  // mcp-data-changed event the embedded server emits). Query keys verified
  // against the real hooks/pages (grep for `queryKey: ['`) as of this writing.
  useEffect(() => {
    const domainKeys: Record<string, string[][]> = {
      bank: MONEY_FLOW_KEYS.map((key) => [key]),
      'bank-accounts': MONEY_FLOW_KEYS.map((key) => [key]),
      stocks: [
        ['investments'],
        ['investment'],
        ['investment-transactions'],
        ['transactions'],
        ['all-stock-transactions'],
        ['dividend-summary'],
        ['stocks-analysis'],
        ['stock-performance'],
        ['stock-import-batches'],
        ['stock-monitor'],
      ],
      crypto: [['crypto'], ['crypto-detail'], ['crypto-transactions'], ['all-crypto-transactions']],
      bonds: [['bonds']],
      loans: [['loans'], ['loan-schedule'], ['real-estate-loans'], ['available-loans']],
      'real-estate': [['real-estate'], ['real-estate-valuations'], ['real-estate-costs']],
      'other-assets': [['other-assets'], ['other-asset-transactions']],
      insurance: [['insurance'], ['available-insurances'], ['real-estate-insurances']],
      // Stock Monitor watchlist: rows only — see the net-worth guard below
      'stock-monitor': [['stock-monitor']],
    };

    const unlisten = listen<McpDataChangedPayload>('mcp-data-changed', (event) => {
      const { domain } = event.payload;

      for (const key of domainKeys[domain] ?? []) {
        queryClient.invalidateQueries({ queryKey: key });
      }
      // Not a useMutation, so the MutationCache refresh does not cover MCP writes.
      queryClient.invalidateQueries({ queryKey: ['milestones'] });

      // Financial bookkeeping is durable backend work. Events only refresh views,
      // including when no renderer was mounted at the moment of the import.
      if (domain !== 'stock-monitor') {
        for (const key of [
          'projection',
          'portfolio-metrics',
          'cashflow-report',
          'portfolio-history',
          'budgeting-report',
          'budgeting-category-transactions',
          'cashflow-actuals',
        ]) {
          queryClient.invalidateQueries({ queryKey: [key] });
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
