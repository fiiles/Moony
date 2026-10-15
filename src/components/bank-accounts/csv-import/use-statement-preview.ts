import { useQuery } from '@tanstack/react-query';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { bankAccountsApi } from '@/lib/tauri-api';
import type { StatementImportOptionsInput } from '@shared/schema';
import { PREVIEW_DEBOUNCE_MS } from './import-config';

interface UseStatementPreviewOptions {
  accountId: string;
  filePath: string;
  /** The options the import would use; null while there is no statement to preview. */
  options: StatementImportOptionsInput | null;
  /** The profile country, so a legacy-encoded file decodes as it did on inspection. */
  country: string | undefined;
  enabled: boolean;
}

/**
 * Dry run of one statement of the file (`previewStatementImport`), ~300 ms after
 * the last change: the statement counterpart of `useCsvPreview`. The
 * options are part of the query key, so a response for older options can never
 * replace newer ones; the previous table of the same file stays on screen
 * (dimmed) while the next one loads.
 *
 * Nothing is cached (`gcTime: 0`): the verdict depends on what is in the
 * database, which an import or a deletion changes.
 */
export function useStatementPreview({
  accountId,
  filePath,
  options,
  country,
  enabled,
}: UseStatementPreviewOptions) {
  const debounced = useDebouncedValue(options, PREVIEW_DEBOUNCE_MS);

  const query = useQuery({
    queryKey: ['statement-import-preview', accountId, filePath, debounced, country],
    queryFn: () => bankAccountsApi.previewStatementImport(accountId, filePath, debounced!, country),
    enabled: enabled && options != null && debounced != null,
    staleTime: 0,
    gcTime: 0,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[2] === filePath ? previous : undefined,
  });

  return {
    preview: query.data,
    error: query.error,
    /** The table on screen belongs to older options (a pick, loading). */
    isUpdating:
      options != null &&
      (JSON.stringify(options) !== JSON.stringify(debounced) || query.isFetching),
    /** There is no table yet to keep on screen. */
    isLoadingFirst: options != null && query.data === undefined && !query.isError,
  };
}
