import { useCallback } from 'react';
import { exportApi } from '@/lib/tauri-api';
import { useCsvExport } from '@/mobile/hooks/use-csv-export';

/** The pages that export a CSV: the class pages, and Finance's loans and policies. */
export type ExportKind = 'stocks' | 'crypto' | 'bonds' | 'otherAssets' | 'loans' | 'insurance';

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
  loans: { fileName: 'moony-loans.csv', load: exportApi.loans },
  insurance: { fileName: 'moony-insurance-policies.csv', load: exportApi.insurancePolicies },
} as const;

/**
 * "Export" in the `···` of Akcie, Krypto, Dluhopisy and Ostatní aktiva (spec §8.8) and of Finance
 * (its loans and its policies, the desktop list pages' exports): fetches the CSV from the backend and hands it to the Files picker. The run, its toasts and its
 * pending state are `useCsvExport`'s (the Cashflow tab's Export shares them).
 */
export function useClassExport(kind: ExportKind): {
  exportCsv: () => Promise<void>;
  isPending: boolean;
} {
  const { run, isPending } = useCsvExport();

  const exportCsv = useCallback(() => {
    const { fileName, load } = EXPORTS[kind];
    return run(fileName, load);
  }, [kind, run]);

  return { exportCsv, isPending };
}
