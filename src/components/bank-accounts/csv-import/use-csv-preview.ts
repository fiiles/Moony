import { useQuery } from '@tanstack/react-query';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { bankAccountsApi } from '@/lib/tauri-api';
import type { CsvImportConfigInput } from '@shared/schema';
import { PREVIEW_DEBOUNCE_MS } from './import-config';

interface UseCsvPreviewOptions {
  accountId: string;
  filePath: string;
  /** The config the import would use; null while the mapping is incomplete. */
  config: CsvImportConfigInput | null;
  enabled: boolean;
}

/**
 * Dry run of the import (`previewCsvImport`) for the current mapping, ~300 ms
 * after the last change. The config is part of the query key, so a response for
 * an older config can never replace a newer one; the previous table of the same
 * file stays on screen (dimmed) while the next one loads.
 *
 * Nothing is cached (`gcTime: 0`): the verdict depends on what is in the
 * database, which an import or a deletion changes.
 */
export function useCsvPreview({ accountId, filePath, config, enabled }: UseCsvPreviewOptions) {
  const debounced = useDebouncedValue(config, PREVIEW_DEBOUNCE_MS);

  const query = useQuery({
    queryKey: ['csv-import-preview', accountId, filePath, debounced],
    queryFn: () => bankAccountsApi.previewCsvImport(accountId, filePath, debounced!),
    enabled: enabled && config != null && debounced != null,
    staleTime: 0,
    gcTime: 0,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[2] === filePath ? previous : undefined,
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
