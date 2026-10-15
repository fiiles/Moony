import { bankAccountsApi } from '@/lib/tauri-api';
import type { CsvImportConfigInput } from '@shared/schema';
import { useImportPreview } from './use-import-preview';

interface UseCsvPreviewOptions {
  accountId: string;
  filePath: string;
  /** The config the import would use; null while the mapping is incomplete. */
  config: CsvImportConfigInput | null;
  enabled: boolean;
}

/**
 * Dry run of the CSV import (`previewCsvImport`) for the current mapping, ~300 ms
 * after the last change (see `useImportPreview`).
 */
export function useCsvPreview({ accountId, filePath, config, enabled }: UseCsvPreviewOptions) {
  return useImportPreview({
    queryKey: ['csv-import-preview', accountId, filePath],
    queryFn: (debounced: CsvImportConfigInput) =>
      bankAccountsApi.previewCsvImport(accountId, filePath, debounced),
    options: config,
    enabled,
  });
}
