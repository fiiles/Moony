import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSyncBackups } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Sheet } from '@/mobile/components/Sheet';
import { useRetained } from '@/mobile/hooks/use-retained';
import { useSyncBackupExport } from '@/mobile/hooks/use-sync-backup-export';
import type { SyncBackupEntry } from '@shared/schema';
import { phoneCopyKey } from '@/mobile/lib/platform-copy';

/** The 48 pt buttons of a row's actions and of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

interface SyncBackupRowsProps {
  /** Reads a listed backup for the restore confirmation (the page's `RestoreBackupSheet`). */
  onRestore: (path: string) => void;
  /** A backup is being made, read or restored: the actions wait. */
  disabled: boolean;
  /** The path whose restore is being read: its Restore button shows the spinner. */
  restoring: string | null;
}

/**
 * Záloha a přenos → the backups a join, repair or re-sync made before replacing data on this phone
 * (audit H4: the app's data folder is kept out of the phone's own backups, so this list and Files
 * are the only way to reach them). Each backup has "Save a copy to Files" (iOS), Restore (the usual
 * inspect-and-confirm flow, on the listed path) and Delete behind a confirmation sheet that names its
 * date and says when it holds changes found nowhere else. Buttons rather than swipes: these are rare
 * acts and the row is not a list row. Left out while nothing is listed.
 */
export function SyncBackupRows({ onRestore, disabled, restoring }: SyncBackupRowsProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const backups = useSyncBackups();
  const { deleteBackup } = useDeviceSyncMutations();
  const { canExport, exporting, exportCopy } = useSyncBackupExport();
  const [toDelete, setToDelete] = useState<SyncBackupEntry | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const shown = useRetained(toDelete);

  // The mutation holds nothing secret, but its state goes with the sheet
  const resetDelete = deleteBackup.reset;
  const closeDelete = () => {
    if (deleteBackup.isPending) return;
    setToDelete(null);
    setDeleteError(null);
    resetDelete();
  };

  const list = backups.data ?? [];
  if (list.length === 0) return null;

  const confirmDelete = async () => {
    if (!toDelete) return;
    setDeleteError(null);
    try {
      await deleteBackup.mutateAsync(toDelete.path);
      toast(ts('data.syncBackups.deleted'));
      setToDelete(null);
      resetDelete();
    } catch (error) {
      // Said in the sheet, where the person is looking
      setDeleteError(translateApiError(error as Error, tc));
    }
  };

  const busy = disabled || exporting !== null;
  return (
    <>
      <SectionHead title={ts('data.syncBackups.title')} />
      <p className="mx-0.5 mb-3 mt-0 text-m-sub text-ink-3">
        {canExport ? t(phoneCopyKey('backup.syncLead')) : t('backup.syncLeadNoFiles')}
      </p>
      {!canExport && (
        <p className="mx-0.5 mb-3 mt-0 text-m-sub text-ink-3" data-testid="sync-backups-no-copy">
          {t('backup.syncNoCopy')}
        </p>
      )}
      <ul className="m-0 grid list-none gap-3 p-0" data-testid="sync-backups">
        {list.map((backup) => (
          <li
            key={backup.path}
            className="grid gap-2.5 rounded-r4 border border-line bg-paper p-3 shadow-e1"
          >
            <div>
              <p className="m-0 text-m-row text-ink num">
                {fmt.dateTime(backup.createdAt)} · {formatBytes(backup.bytes, fmt.locale)}
              </p>
              {backup.kept && (
                <p className="mx-0 mb-0 mt-1 text-m-sub font-semibold text-loss">
                  {ts('data.syncBackups.kept')}
                </p>
              )}
            </div>
            <div className="grid gap-2">
              {canExport && (
                <Button
                  variant="secondary"
                  className={`${BUTTON} w-full`}
                  loading={exporting === backup.path}
                  disabled={busy}
                  onClick={() => void exportCopy(backup.path)}
                >
                  {t(phoneCopyKey('backup.syncSaveCopy'))}
                </Button>
              )}
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  className={`${BUTTON} flex-1`}
                  loading={restoring === backup.path}
                  disabled={busy}
                  onClick={() => onRestore(backup.path)}
                >
                  {ts('data.syncBackups.restore')}
                </Button>
                <Button
                  variant="outline"
                  className={`${BUTTON} flex-1`}
                  disabled={busy}
                  onClick={() => setToDelete(backup)}
                >
                  {ts('data.syncBackups.delete')}
                </Button>
              </div>
            </div>
          </li>
        ))}
      </ul>
      <Sheet
        open={toDelete !== null}
        onOpenChange={(next) => {
          if (!next) closeDelete();
        }}
        title={ts('data.syncBackups.deleteTitle', {
          date: shown ? fmt.dateTime(shown.createdAt) : '',
        })}
        description={
          shown?.kept ? ts('data.syncBackups.deleteBodyKept') : ts('data.syncBackups.deleteBody')
        }
        footer={
          <>
            <Button
              variant="ghost"
              className={`${BUTTON} px-[18px]`}
              disabled={deleteBackup.isPending}
              onClick={closeDelete}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button
              variant="destructive"
              className={`${BUTTON} flex-1`}
              loading={deleteBackup.isPending}
              onClick={() => void confirmDelete()}
            >
              {ts('data.syncBackups.delete')}
            </Button>
          </>
        }
      >
        {deleteError ? <InlineAlert icon={AlertTriangle}>{deleteError}</InlineAlert> : null}
      </Sheet>
    </>
  );
}
