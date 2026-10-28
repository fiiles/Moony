import { useCallback, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { open } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { biometricApi, dataApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { deviceStorage } from '@/utils/device-storage';
import { cancelAllNotifications } from '@/mobile/hooks/use-notification-schedule';
import { BIOMETRIC_STATUS_KEY, clearEnrolmentAsked } from '@/lib/biometrics';
import { dialogPathToFsPath, restoreBackupPlan } from '@/mobile/lib/export-file';
import type { BackupInspection } from '@shared/schema';

/** The backups the picker offers: the archive Moony makes on the phone and on the computer. */
export const BACKUP_FILE_FILTER = [{ name: 'Moony backup', extensions: ['zip'] }];

/**
 * Restore from a backup on the phone, with or without a profile (Nastavení → Záloha, and the first
 * run's "Máte zálohu?"): nothing here needs the database open, as the backend's restore does not.
 *
 * `pickRestore` opens the picker for an archive made on this phone or by Moony on the computer
 * (the same file, the same password) and reads its manifest; `restore` replaces whatever profile
 * there is with it and reloads to the lock screen, where the password of the restored profile
 * unlocks it. Failures are said in a toast; `restore` throws them again so the caller's
 * confirmation stays open.
 */
export function useRestoreBackup(): {
  pickRestore: () => Promise<BackupInspection | null>;
  restore: (inspection: BackupInspection) => Promise<void>;
  /** The archive is being read or restored. */
  restoring: boolean;
} {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const [restoring, setRestoring] = useState(false);

  const status = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  // A status that cannot be read counts as enrolled: switching biometric unlock off is harmless
  // when it is already off, and a key left behind could not open the restored profile
  const enrolled = status.data?.enrolled ?? true;

  const showError = useCallback(
    (error: unknown) => {
      toast.error(tc('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), tc),
      });
    },
    [tc]
  );

  const pickRestore = useCallback(async (): Promise<BackupInspection | null> => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: BACKUP_FILE_FILTER,
      });
      if (!selected) return null;
      setRestoring(true);
      return await dataApi.inspectBackup(dialogPathToFsPath(selected));
    } catch (error) {
      showError(error);
      return null;
    } finally {
      setRestoring(false);
    }
  }, [showError]);

  const restore = useCallback(
    async (inspection: BackupInspection): Promise<void> => {
      setRestoring(true);
      try {
        for (const step of restoreBackupPlan(enrolled)) {
          switch (step) {
            case 'disable-biometrics':
              try {
                await biometricApi.disable();
              } catch {
                // The restore does not depend on it: the lock screen falls back to the password
                // when a key left behind no longer opens the profile
              }
              break;
            case 'clear-asked':
              // The restored profile is offered biometric unlock again after its first unlock
              clearEnrolmentAsked(deviceStorage() ?? null);
              break;
            case 'restore':
              await dataApi.restoreBackup(inspection.archivePath);
              break;
            case 'reload':
              toast(t('data.restored'), { description: t('data.restoredDescription') });
              // The old profile's notifications must not outlive it; the restored profile is
              // planned again at its next unlock
              await cancelAllNotifications();
              // The backend already locked the app; the reload lands on the lock screen
              window.setTimeout(() => window.location.reload(), 800);
              break;
          }
        }
      } catch (error) {
        showError(error);
        setRestoring(false);
        throw error;
      }
      // After a restore the page reloads: `restoring` stays on until then
    },
    [enrolled, showError, t]
  );

  return { pickRestore, restore, restoring };
}
