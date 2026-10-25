import { useCallback } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { dataApi } from '@/lib/tauri-api';
import { exportsOnThisDevice, type ExportOutcome } from '@/mobile/lib/export-file';

/**
 * Hands a text to the system's Files picker (the recovery key's `.txt`, a class page's CSV): Rust
 * writes it to `Documents/<fileName>`, which the iOS save picker then exports to the place the
 * person chooses, and Rust removes the staged file again, whatever the person did. The webview has
 * no fs commands: `fileName` must be a name Rust allow-lists (`RECOVERY_KEY_FILE`, a password
 * change's `changedRecoveryKeyFileName`, the names in `use-class-export.ts`). Where exporting does
 * not work (anything but iOS) nothing is staged: the person is told and the outcome is
 * `'cancelled'`.
 *
 * A removal that fails rejects the whole export, even after the picker saved the file: the text
 * would still be lying in Documents, and the caller must say so rather than report a clean save.
 */
export function useExportTextFile(): {
  exportText: (fileName: string, contents: string) => Promise<ExportOutcome>;
} {
  const { t } = useTranslation('mobile');

  const exportText = useCallback(
    async (fileName: string, contents: string): Promise<ExportOutcome> => {
      if (!exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '')) {
        toast(t('backup.unavailableHere'));
        return 'cancelled';
      }
      let staged: string | null = null;
      try {
        staged = await dataApi.stageTextExport(fileName, contents);
        // `null` is a cancelled picker
        const target = await save({ defaultPath: fileName });
        return target === null ? 'cancelled' : 'saved';
      } finally {
        // A stage that failed has nothing to discard: Rust leaves no partial file
        if (staged !== null) await dataApi.discardTextExport(staged);
      }
    },
    [t]
  );

  return { exportText };
}
