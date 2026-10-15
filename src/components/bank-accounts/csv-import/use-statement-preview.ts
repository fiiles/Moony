import { bankAccountsApi } from '@/lib/tauri-api';
import type { StatementImportOptionsInput } from '@shared/schema';
import { useImportPreview } from './use-import-preview';

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
 * the last change (see `useImportPreview`): the statement counterpart of
 * `useCsvPreview`.
 */
export function useStatementPreview({
  accountId,
  filePath,
  options,
  country,
  enabled,
}: UseStatementPreviewOptions) {
  return useImportPreview({
    queryKey: ['statement-import-preview', accountId, filePath],
    keyExtra: [country],
    queryFn: (debounced: StatementImportOptionsInput) =>
      bankAccountsApi.previewStatementImport(accountId, filePath, debounced, country),
    options,
    enabled,
  });
}
