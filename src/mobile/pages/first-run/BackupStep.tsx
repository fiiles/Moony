import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { cn } from '@/lib/utils';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { useBackup } from '@/mobile/hooks/use-backup';
import { StepHead } from './StepHead';
import { caption, primaryButton, textLink } from './styles';

/**
 * Záloha, the last step: the phone holds the only copy of the data, so the first encrypted backup
 * is made now (spec D7, recommended; there is no automatic backup). "Zálohovat nyní" stages the
 * archive, opens the Files picker and records the backup when it was saved. The first run then ends
 * whatever came of it: a cancelled picker says so in a toast and the Údržba card keeps asking.
 * Where backups cannot be exported (not iOS yet) the button is off and says why. With sync built
 * in, one quieter line under "Teď ne" (spec 2026-11-14 §5.2): "Nastavte synchronizaci" ends the run
 * without a backup and opens Nastavení → Synchronizace.
 */
export function BackupStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { createBackup, busy, canExport } = useBackup();
  const syncFeature = useDeviceSyncFeature();
  const backingUp = busy === 'backup';
  const finishing = flow.pending.finishing;

  const backUpAndFinish = async () => {
    // `createBackup` has said what went wrong or that nothing was saved
    await createBackup();
    await flow.finish();
  };

  return (
    <div data-testid="first-run-backup">
      <StepHead title={t('firstRun.backupTitle')} lead={t('firstRun.backupLead')} />
      <Button
        className={primaryButton}
        loading={backingUp}
        disabled={!canExport || backingUp || finishing}
        onClick={() => void backUpAndFinish()}
      >
        {ts('data.backupNow')}
      </Button>
      {!canExport && <p className={`${caption} mt-3 text-center`}>{t('backup.unavailableHere')}</p>}
      <button
        type="button"
        className={textLink}
        disabled={backingUp || finishing}
        onClick={() => void flow.finish()}
      >
        {t('biometrics.notNow')}
      </button>
      {syncFeature && (
        <>
          <p className={cn(caption, 'mt-4 text-center')}>{t('firstRun.syncLead')}</p>
          <button
            type="button"
            className={cn(textLink, 'mt-0')}
            disabled={backingUp || finishing}
            onClick={() => void flow.finish('sync')}
            data-testid="first-run-set-up-sync"
          >
            {t('firstRun.syncLink')}
          </button>
        </>
      )}
    </div>
  );
}
