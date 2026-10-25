import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { translateApiError } from '@/lib/translate-api-error';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';

/**
 * A byte-order mark in front of the CSV, as the desktop's export writes it: Excel and Numbers
 * read a UTF-8 file without one as the system's code page and garble the accents. Written as an
 * escape: a literal U+FEFF is invisible in the source and an editor may drop it.
 */
export const BOM = '\uFEFF';

/**
 * The run behind an "Export" menu item that saves a CSV (the class pages' and the Cashflow tab's):
 * `load` produces the CSV and the number of records in it, and `run` hands the text to the Files
 * picker (`useExportTextFile`) under `fileName`, which Rust must allow-list. A load with nothing in
 * it (`count` 0) says so instead of opening the picker; a saved file is told ("Export dokončen"
 * with the number of records), a cancelled picker is not; a failure is told with its message. While
 * it runs `isPending` is on, so the menu item can be disabled.
 */
export function useCsvExport(): {
  run: (fileName: string, load: () => Promise<{ csv: string; count: number }>) => Promise<void>;
  isPending: boolean;
} {
  const { t: tc } = useTranslation('common');
  const { exportText } = useExportTextFile();
  const [isPending, setPending] = useState(false);

  const run = useCallback(
    async (fileName: string, load: () => Promise<{ csv: string; count: number }>) => {
      setPending(true);
      try {
        const { csv, count } = await load();
        if (count === 0) {
          toast(tc('export.noData'), { description: tc('export.noDataDescription') });
          return;
        }
        const outcome = await exportText(fileName, BOM + csv);
        if (outcome === 'saved') {
          toast(tc('export.success'), {
            description: tc('export.successDescription', { count }),
          });
        }
      } catch (error) {
        toast.error(tc('status.error'), {
          description: translateApiError(error instanceof Error ? error : String(error), tc),
        });
      } finally {
        setPending(false);
      }
    },
    [exportText, tc]
  );

  return { run, isPending };
}
