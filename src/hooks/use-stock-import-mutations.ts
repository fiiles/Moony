import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { portfolioApi, priceApi, stockImportApi } from '@/lib/tauri-api';
import type { StockImportConfig } from '@shared/schema';

/**
 * Query keys (prefix-matched) whose data is derived from stock transactions or
 * lists the recorded imports. Mirrors the `stocks` domain list in SyncProvider
 * plus the analysis views.
 */
const STOCK_IMPORT_KEYS = [
  ['investments'],
  ['investment'],
  ['investment-transactions'],
  ['transactions'],
  ['all-stock-transactions'],
  ['dividend-summary'],
  ['stocks-analysis'],
  ['tag-metrics'],
  ['stock-twr'],
  ['stock-import-batches'],
] as const;

/**
 * What every change to stock transactions needs (AGENTS rule 7): the domain
 * keys, portfolio metrics and the cashflow report, then a fresh snapshot and
 * the portfolio history. The write already succeeded, so a failed snapshot
 * must not turn it into an error.
 */
async function refreshAfterStockChange(queryClient: QueryClient) {
  for (const queryKey of STOCK_IMPORT_KEYS) {
    queryClient.invalidateQueries({ queryKey });
  }
  queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
  queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
  try {
    await portfolioApi.recordSnapshot();
    queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
  } catch (error) {
    console.error('Failed to record portfolio snapshot:', error);
  }
}

/** New positions get their prices and dividends in the background, as after a manual purchase. */
function refreshPricesInBackground(queryClient: QueryClient) {
  priceApi
    .refreshStockPrices()
    .then(() => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      return priceApi.refreshDividends();
    })
    .then(() => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
    })
    .catch((error: unknown) => {
      console.error('Background refresh error:', error);
    });
}

/**
 * Writes of the stock import wizard: the import itself, undoing a recorded
 * import, and the remembered formats. Importing and undoing follow the
 * mutation contract (reference: use-bank-account-mutations.ts); what the user
 * is told about a failure is up to the dialog, which knows the row messages.
 */
export function useStockImportMutations() {
  const queryClient = useQueryClient();

  const importCsv = useMutation({
    mutationFn: ({ filePath, config }: { filePath: string; config: StockImportConfig }) =>
      stockImportApi.import(filePath, config),
    onSuccess: async (result) => {
      // Their duplicate verdicts are out of date now: the rows are in the database.
      queryClient.removeQueries({ queryKey: ['stock-import-preview'] });
      await refreshAfterStockChange(queryClient);
      if (result.imported > 0) refreshPricesInBackground(queryClient);
    },
  });

  const undoBatch = useMutation({
    mutationFn: (batchId: string) => stockImportApi.undoBatch(batchId),
    onSuccess: async () => {
      queryClient.removeQueries({ queryKey: ['stock-import-preview'] });
      await refreshAfterStockChange(queryClient);
    },
  });

  const saveFormat = useMutation({
    mutationFn: ({
      name,
      headers,
      config,
    }: {
      name: string;
      headers: string[];
      config: StockImportConfig;
    }) => stockImportApi.saveFormat(name, headers, config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['stock-import-formats'] });
    },
  });

  const deleteFormat = useMutation({
    mutationFn: (id: string) => stockImportApi.deleteFormat(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['stock-import-formats'] });
    },
  });

  return { importCsv, undoBatch, saveFormat, deleteFormat };
}
