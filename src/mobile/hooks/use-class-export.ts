import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { exportApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';

/** The class pages that export their CSV. */
export type ExportKind = 'stocks' | 'crypto' | 'bonds' | 'otherAssets';

/**
 * What each class exports: the desktop's CSV, and the name it is staged under. The names are the
 * ones Rust allow-lists (`TEXT_EXPORT_FILE_NAMES` in `services/backup.rs`): the phone never uses the
 * dated `filename` the export command returns, which is the desktop's.
 */
const EXPORTS = {
  stocks: { fileName: 'moony-stock-transactions.csv', load: exportApi.stockTransactions },
  crypto: { fileName: 'moony-crypto-transactions.csv', load: exportApi.cryptoTransactions },
  bonds: { fileName: 'moony-bonds.csv', load: exportApi.bonds },
  otherAssets: { fileName: 'moony-other-assets.csv', load: exportApi.otherAssets },
} as const;

/**
 * A byte-order mark in front of the CSV, as the desktop's export writes it: Excel and Numbers
 * read a UTF-8 file without one as the system's code page and garble the accents.
 */
const BOM = '\uFEFF';

/**
 * "Export" in the `···` of Akcie, Krypto, Dluhopisy and Ostatní aktiva (spec §8.8): fetches the
 * class's CSV from the backend and hands it to the Files picker (`useExportTextFile`). A class with
 * nothing in it says so instead of opening the picker; a saved file is told ("Export dokončen" with
 * the number of records), a cancelled picker is not; a failure is told with its message. While it
 * runs `isPending` is on, so the menu item can be disabled.
 */
export function useClassExport(kind: ExportKind): {
  exportCsv: () => Promise<void>;
  isPending: boolean;
} {
  const { t: tc } = useTranslation('common');
  const { exportText } = useExportTextFile();
  const [isPending, setPending] = useState(false);

  const exportCsv = useCallback(async () => {
    const { fileName, load } = EXPORTS[kind];
    setPending(true);
    try {
      const { csv, count } = await load();
      if (count === 0) {
        toast(tc('export.noData'), { description: tc('export.noDataDescription') });
        return;
      }
      const outcome = await exportText(fileName, BOM + csv);
      if (outcome === 'saved') {
        toast(tc('export.success'), { description: tc('export.successDescription', { count }) });
      }
    } catch (error) {
      toast.error(tc('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), tc),
      });
    } finally {
      setPending(false);
    }
  }, [kind, exportText, tc]);

  return { exportCsv, isPending };
}
