import { useQuery } from '@tanstack/react-query';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { stockImportApi } from '@/lib/tauri-api';
import type { StockImportConfig } from '@shared/schema';
import { PREVIEW_DEBOUNCE_MS } from './import-config';

interface UseStockImportPreviewOptions {
  filePath: string;
  /** The config the import would use; null while the mapping is incomplete. */
  config: StockImportConfig | null;
  enabled: boolean;
}

/**
 * Dry run of the import (`previewStockCsvImport`) for the current mapping,
 * ~300 ms after the last change. The config is part of the query key, so a
 * response for an older config can never replace a newer one; the previous
 * table of the same file stays on screen (dimmed) while the next one loads.
 *
 * Nothing is cached (`gcTime: 0`): the verdict depends on what is in the
 * database, which an import or a deletion changes.
 */
export function useStockImportPreview({ filePath, config, enabled }: UseStockImportPreviewOptions) {
  const debounced = useDebouncedValue(config, PREVIEW_DEBOUNCE_MS);

  const query = useQuery({
    queryKey: ['stock-import-preview', filePath, debounced],
    queryFn: () => stockImportApi.preview(filePath, debounced!),
    enabled: enabled && config != null && debounced != null,
    staleTime: 0,
    gcTime: 0,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[1] === filePath ? previous : undefined,
  });

  return {
    preview: query.data,
    error: query.error,
    /** The table on screen belongs to an older mapping (typing, loading). */
    isUpdating: config != null && (config !== debounced || query.isFetching),
    /** There is no table yet to keep on screen. */
    isLoadingFirst: config != null && query.data === undefined && !query.isError,
  };
}
