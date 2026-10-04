import { useQuery } from '@tanstack/react-query';
import { stockImportApi } from '@/lib/tauri-api';

/**
 * Recorded imports, newest first. Read again every time the list is shown: the
 * remaining count of a batch changes when its trades are deleted one by one.
 */
export function useStockImportBatches(enabled: boolean) {
  return useQuery({
    queryKey: ['stock-import-batches'],
    queryFn: () => stockImportApi.listBatches(),
    enabled,
    staleTime: 0,
  });
}

/** Custom mappings remembered per header layout; only the wizard changes them. */
export function useStockImportFormats(enabled: boolean) {
  return useQuery({
    queryKey: ['stock-import-formats'],
    queryFn: () => stockImportApi.listFormats(),
    enabled,
  });
}
