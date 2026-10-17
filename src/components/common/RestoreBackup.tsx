import { useState, type ReactNode } from 'react';
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
import { dataApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import type { BackupInspection } from '@shared/schema';

interface RestoreBackupProps {
  /** Renders the trigger; `pick` opens the file dialog, `busy` is true while picking or restoring. */
  children: (pick: () => void, busy: boolean) => ReactNode;
  /** First run: there is no current account to move aside, so the dialog says so. */
  fresh?: boolean;
  onBusyChange?: (busy: boolean) => void;
}

/**
 * "Restore from backup": pick a Moony ZIP, confirm what it holds, replace the
 * account. Works locked and on the first run too — the backend never needs
 * the database open for it. The app reloads to the lock screen afterwards,
 * where the backup's password unlocks it.
 */
export function RestoreBackup({ children, fresh = false, onBusyChange }: RestoreBackupProps) {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [busy, setBusyState] = useState(false);
  const [pending, setPending] = useState<BackupInspection | null>(null);

  const setBusy = (value: boolean) => {
    setBusyState(value);
    onBusyChange?.(value);
  };

  const showError = (error: unknown) => {
    const message =
      error instanceof Error ? translateApiError(error, tc) : translateApiError(String(error), tc);
    toast.error(tc('status.error'), { description: message });
  };

  const pick = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Moony backup', extensions: ['zip'] }],
      });
      if (!selected || Array.isArray(selected)) return;
      setBusy(true);
      setPending(await dataApi.inspectBackup(selected));
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    if (!pending) return;
    try {
      setBusy(true);
      await dataApi.restoreBackup(pending.archivePath);
      toast(t('data.restored'), { description: t('data.restoredDescription') });
      setPending(null);
      // The backend already locked the app; reload to the lock screen.
      window.setTimeout(() => window.location.reload(), 800);
    } catch (error) {
      showError(error);
      setBusy(false);
    }
  };

  const details = pending && {
    date: fmt.dateTime(pending.manifest.createdAt),
    version: pending.manifest.appVersion,
    attachments: pending.attachmentCount,
    size: formatBytes(pending.archiveBytes, fmt.locale),
  };

  return (
    <>
      {children(() => void pick(), busy)}
      <AlertDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open && !busy) setPending(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {fresh ? t('data.restoreFreshTitle') : t('data.restoreConfirmTitle')}
            </AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2">
                {details && (
                  <p>
                    {fresh
                      ? t('data.restoreFreshDescription', details)
                      : t('data.restoreConfirmDescription', details)}
                  </p>
                )}
                {pending?.newerThanApp && (
                  <p className="font-600 text-loss">{t('data.restoreNewer')}</p>
                )}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>{tc('buttons.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                void confirm();
              }}
              disabled={busy || pending?.newerThanApp}
            >
              {busy ? t('data.restoring') : t('data.restoreConfirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
