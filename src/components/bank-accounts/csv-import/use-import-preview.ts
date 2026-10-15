import { useQuery } from '@tanstack/react-query';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import type { CsvImportPreview } from '@shared/schema';
import { PREVIEW_DEBOUNCE_MS } from './import-config';

interface UseImportPreviewOptions<T> {
  /**
   * `[name, accountId, filePath]`: the query key starts with it, then the debounced options,
   * then `keyExtra`.
   */
  queryKey: readonly [name: string, accountId: string, filePath: string];
  /** Key parts after the options (the statement preview's country). */
  keyExtra?: readonly unknown[];
  /** The dry run for the debounced options. */
  queryFn: (options: T) => Promise<CsvImportPreview>;
  /** The options the import would use; null while there is nothing to preview. */
  options: T | null;
  enabled: boolean;
}

/**
 * Dry run of an import (CSV config or statement options), ~300 ms after the
 * last change. The options are part of the query key, so a response for older
 * options can never replace newer ones; the previous table of the same file
 * stays on screen (dimmed) while the next one loads.
 *
 * Nothing is cached (`gcTime: 0`): the verdict depends on what is in the
 * database, which an import or a deletion changes.
 */
export function useImportPreview<T>({
  queryKey,
  keyExtra = [],
  queryFn,
  options,
  enabled,
}: UseImportPreviewOptions<T>) {
  const debounced = useDebouncedValue(options, PREVIEW_DEBOUNCE_MS);
  const filePath = queryKey[2];

  const query = useQuery({
    queryKey: [...queryKey, debounced, ...keyExtra],
    queryFn: () => queryFn(debounced!),
    enabled: enabled && options != null && debounced != null,
    staleTime: 0,
    gcTime: 0,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[2] === filePath ? previous : undefined,
  });

  return {
    preview: query.data,
    error: query.error,
    /** The table on screen belongs to older options (typing, loading). */
    isUpdating:
      options != null &&
      (JSON.stringify(options) !== JSON.stringify(debounced) || query.isFetching),
    /** There is no table yet to keep on screen. */
    isLoadingFirst: options != null && query.data === undefined && !query.isError,
  };
}
