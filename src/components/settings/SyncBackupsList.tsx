import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { RestoreBackup } from '@/components/common/RestoreBackup';
import { useSyncBackups } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import type { SyncBackupEntry } from '@shared/schema';

/**
 * "Backups made before sync replaced data": what a join, repair or re-sync saved first. A backup
 * marked kept holds the only copy of something, so the app never removes it; the person can,
 * behind a confirmation that says so. Restoring goes through the usual inspect-and-confirm flow.
 */
export function SyncBackupsList({ disabled = false }: { disabled?: boolean }) {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const backups = useSyncBackups();
  const { deleteBackup } = useDeviceSyncMutations();
  const [toDelete, setToDelete] = useState<SyncBackupEntry | null>(null);

  const list = backups.data ?? [];
  if (list.length === 0) return null;

  const confirmDelete = async () => {
    if (!toDelete) return;
    try {
      await deleteBackup.mutateAsync(toDelete.path);
      toast(t('data.syncBackups.deleted'));
      setToDelete(null);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <div className="grid gap-2 px-5 py-4" data-testid="sync-backups">
      <h3 className="m-0 text-body font-600 text-ink">{t('data.syncBackups.title')}</h3>
      <RestoreBackup>
        {(_pick, busy, pickPath) => (
          <ul className="m-0 grid list-none gap-2 p-0">
            {list.map((backup) => (
              <li
                key={backup.path}
                className="flex flex-wrap items-center justify-between gap-2 text-body text-ink-2"
              >
                <span className="num">
                  {fmt.dateTime(backup.createdAt)} · {formatBytes(backup.bytes, fmt.locale)}
                  {backup.kept && (
                    <span className="ml-2 rounded-full bg-loss/10 px-2 py-0.5 text-micro font-600 text-loss">
                      {t('data.syncBackups.kept')}
                    </span>
                  )}
                </span>
                <span className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy || disabled}
                    onClick={() => pickPath(backup.path)}
                  >
                    {t('data.syncBackups.restore')}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy || disabled}
                    onClick={() => setToDelete(backup)}
                  >
                    {t('data.syncBackups.delete')}
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </RestoreBackup>
      <AlertDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open && !deleteBackup.isPending) setToDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('data.syncBackups.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {toDelete?.kept
                ? t('data.syncBackups.deleteBodyKept')
                : t('data.syncBackups.deleteBody')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleteBackup.isPending}>
              {tc('buttons.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault();
                void confirmDelete();
              }}
              disabled={deleteBackup.isPending}
            >
              {t('data.syncBackups.delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
