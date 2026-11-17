import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { dataApi, deviceSyncApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { backupFileName as fileNameOf } from '@/utils/sync-status';
import { BACKUP_FILE_FILTER } from '@/mobile/hooks/use-restore-backup';
import { exportsOnThisDevice, type ExportOutcome } from '@/mobile/lib/export-file';
import { saveStagedExport } from '@/mobile/lib/picked-files';

/**
 * "Save a copy to Files" for a sync backup (audit H4: the app's data folder is kept out of the
 * phone's own backups, so a backup made before sync replaced data was out of reach). Rust copies
 * the listed backup into Documents (`stageSyncBackupExport`), the Files sheet takes it from there
 * and Rust removes the staged copy again. The staged file's own name is the one offered (Rust names
 * it by the backup's day). `finishBackupExport` is told `false` on purpose: `true` would record
 * the copy as a backup of the person's data and silence the "backup is due" reminder.
 *
 * `canExport` is false where the Files picker cannot write a copy (anywhere but iOS): the action is
 * left out there. `exporting` is the path being copied, if any.
 */
export function useSyncBackupExport(): {
  canExport: boolean;
  exporting: string | null;
  exportCopy: (path: string) => Promise<ExportOutcome>;
} {
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [exporting, setExporting] = useState<string | null>(null);
  const canExport = exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '');

  const exportCopy = useCallback(
    async (path: string): Promise<ExportOutcome> => {
      if (!canExport) return 'cancelled';
      setExporting(path);
      try {
        const staged = await deviceSyncApi.stageSyncBackupExport(path);
        let exported = false;
        try {
          exported = await saveStagedExport(staged, {
            defaultPath: fileNameOf(staged),
            filters: BACKUP_FILE_FILTER,
          });
        } finally {
          // The copy is saved (or not) either way: a failed removal is not the person's problem, and
          // the startup sweep removes a staged copy that is left
          await dataApi.finishBackupExport(staged, false).catch((failure: unknown) => {
            console.debug('Could not remove the staged sync backup copy', failure);
          });
        }
        toast(exported ? tm('backup.syncCopySaved') : tm('backup.notSaved'));
        return exported ? 'saved' : 'cancelled';
      } catch (error) {
        toast.error(tc('status.error'), {
          description: translateApiError(error instanceof Error ? error : String(error), tc),
        });
        return 'cancelled';
      } finally {
        setExporting(null);
      }
    },
    [canExport, tc, tm]
  );

  return { canExport, exporting, exportCopy };
}
