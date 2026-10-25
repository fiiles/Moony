import { useCallback, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { save } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { queryClient } from '@/lib/queryClient';
import { dataApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { todayIsoUtc } from '@/utils/period';
import { BACKUP_FILE_FILTER, useRestoreBackup } from '@/mobile/hooks/use-restore-backup';
import { backupFileName, exportsOnThisDevice, type ExportOutcome } from '@/mobile/lib/export-file';
import type { BackupInspection } from '@shared/schema';

/** What the person is waiting for: the archive being written and exported, or a backup being read or restored. */
type Busy = 'backup' | 'restore' | null;

/** Query key of the onboarding progress, whose `lastBackupAt` Nastavení and its Záloha page show. */
const PROGRESS_KEY = ['onboarding-progress'] as const;

/**
 * Backup and restore on the phone (spec §8.9, Záloha a přenos), for a profile that is open.
 *
 * `createBackup` stages the archive in the app's Documents folder (Rust), exports it through the
 * system's Files picker and removes the staged copy; the backup is recorded (the last-backup date,
 * the "backup is due" reminder) only when the picker saved it. Failures are said in a toast here.
 * Only iOS exports this way (`canExport`): elsewhere nothing is staged, the person is told, and the
 * outcome is `'cancelled'`.
 *
 * `pickRestore` and `restore` are `useRestoreBackup`'s, which the first run uses without a profile.
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
  const [backingUp, setBackingUp] = useState(false);
  const { pickRestore, restore, restoring } = useRestoreBackup();
  const canExport = exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '');

  const progress = useQuery({ queryKey: PROGRESS_KEY, queryFn: () => onboardingApi.getProgress() });

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
    setBackingUp(true);
    try {
      const name = backupFileName(todayIsoUtc());
      const staged = await dataApi.stageBackupExport(name);
      let exported = false;
      try {
        exported = (await save({ defaultPath: name, filters: BACKUP_FILE_FILTER })) !== null;
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
      setBackingUp(false);
    }
  }, [canExport, showError, t, tm]);

  return {
    createBackup,
    pickRestore,
    restore,
    busy: backingUp ? 'backup' : restoring ? 'restore' : null,
    lastBackupAt: progress.data?.lastBackupAt ?? null,
    canExport,
  };
}
