import { useCallback, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { open, save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { queryClient } from '@/lib/queryClient';
import { biometricApi, dataApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { deviceStorage } from '@/utils/device-storage';
import { todayIsoUtc } from '@/utils/period';
import { cancelAllNotifications } from '@/mobile/hooks/use-notification-schedule';
import { BIOMETRIC_STATUS_KEY, clearEnrolmentAsked } from '@/mobile/lib/biometrics';
import {
  backupFileName,
  dialogPathToFsPath,
  exportsOnThisDevice,
  restoreBackupPlan,
  type ExportOutcome,
} from '@/mobile/lib/export-file';
import type { BackupInspection } from '@shared/schema';

/** What the person is waiting for: the archive being written and exported, or a backup being read or restored. */
type Busy = 'backup' | 'restore' | null;

const ZIP_FILTER = [{ name: 'Moony backup', extensions: ['zip'] }];

/** Query key of the onboarding progress, whose `lastBackupAt` Nastavení and its Záloha page show. */
const PROGRESS_KEY = ['onboarding-progress'] as const;

/**
 * Backup and restore on the phone (spec §8.9, Záloha a přenos).
 *
 * `createBackup` stages the archive in the app's Documents folder (Rust), exports it through the
 * system's Files picker and removes the staged copy; the backup is recorded (the last-backup date,
 * the "backup is due" reminder) only when the picker saved it. Failures are said in a toast here.
 * Only iOS exports this way (`canExport`): elsewhere nothing is staged, the person is told, and the
 * outcome is `'cancelled'`.
 *
 * `pickRestore` opens the picker for an archive made on this phone or by Moony on the computer
 * (the same file, the same password) and reads its manifest; `restore` replaces the profile with
 * it and reloads to the lock screen, where the password of the restored profile unlocks it.
 */
export function useBackup(): {
  createBackup: () => Promise<ExportOutcome>;
  pickRestore: () => Promise<BackupInspection | null>;
  restore: (inspection: BackupInspection) => Promise<void>;
  busy: Busy;
  lastBackupAt: number | null;
  /** Whether this device can export a backup through the Files picker (iOS only for now). */
  canExport: boolean;
} {
  const { t } = useTranslation('settings');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [busy, setBusy] = useState<Busy>(null);
  const canExport = exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '');

  const progress = useQuery({ queryKey: PROGRESS_KEY, queryFn: () => onboardingApi.getProgress() });
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

  const createBackup = useCallback(async (): Promise<ExportOutcome> => {
    if (!canExport) {
      toast(tm('backup.unavailableHere'));
      return 'cancelled';
    }
    setBusy('backup');
    try {
      const name = backupFileName(todayIsoUtc());
      const staged = await dataApi.stageBackupExport(name);
      let exported = false;
      try {
        exported = (await save({ defaultPath: name, filters: ZIP_FILTER })) !== null;
      } finally {
        await dataApi.finishBackupExport(staged, exported);
      }
      if (!exported) {
        toast(tm('backup.notSaved'));
        return 'cancelled';
      }
      void queryClient.invalidateQueries({ queryKey: ['milestones'] });
      void queryClient.invalidateQueries({ queryKey: PROGRESS_KEY });
      toast(t('data.backupSaved'));
      return 'saved';
    } catch (error) {
      showError(error);
      // The failure has been said; there is no saved file to report
      return 'cancelled';
    } finally {
      setBusy(null);
    }
  }, [canExport, showError, t, tm]);

  const pickRestore = useCallback(async (): Promise<BackupInspection | null> => {
    try {
      const selected = await open({ multiple: false, directory: false, filters: ZIP_FILTER });
      if (!selected) return null;
      setBusy('restore');
      return await dataApi.inspectBackup(dialogPathToFsPath(selected));
    } catch (error) {
      showError(error);
      return null;
    } finally {
      setBusy(null);
    }
  }, [showError]);

  /** A failure is toasted here and thrown again, so that the caller keeps its confirmation open. */
  const restore = useCallback(
    async (inspection: BackupInspection): Promise<void> => {
      setBusy('restore');
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
        setBusy(null);
        throw error;
      }
      // After a restore the page reloads: `busy` stays on until then
    },
    [enrolled, showError, t]
  );

  return {
    createBackup,
    pickRestore,
    restore,
    busy,
    lastBackupAt: progress.data?.lastBackupAt ?? null,
    canExport,
  };
}
