import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { cn } from '@/lib/utils';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { exportsOnThisDevice } from '@/mobile/lib/export-file';
import { StepHead } from './StepHead';
import { caption, primaryButton, textLink } from './styles';
import { phoneCopyKey } from '@/mobile/lib/platform-copy';

/**
 * The last step. A new profile has (almost) nothing to back up yet, so the step only says where the
 * data lives and how to keep a copy; "Hotovo" ends the run.
 *
 * With sync built in it is "Ochraňte svá data" and sync comes first (owner, 2026-11-17): the
 * primary "Nastavit synchronizaci" ends the run and opens Nastavení → Synchronizace. Sync needs a
 * server most people do not have yet, so the backup follows as the alternative (where this phone
 * can make one) and a quiet "Teď ne" ends the run in one tap; the Přehled's getting-started card
 * asks again later.
 */
export function BackupStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { t: tSync } = useTranslation('deviceSync');
  const syncFeature = useDeviceSyncFeature();
  const canBackup = exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '');
  const finishing = flow.pending.finishing;

  if (!syncFeature) {
    return (
      <div data-testid="first-run-backup">
        <StepHead title={t('firstRun.backupTitle')} lead={t(phoneCopyKey('firstRun.backupLead'))} />
        <Button
          className={primaryButton}
          loading={finishing}
          disabled={finishing}
          onClick={() => void flow.finish()}
        >
          {tc('buttons.done')}
        </Button>
      </div>
    );
  }

  return (
    <div data-testid="first-run-backup">
      <StepHead title={t('firstRun.protectTitle')} lead={t(phoneCopyKey('firstRun.protectLead'))} />
      <Button
        className={primaryButton}
        loading={finishing}
        disabled={finishing}
        onClick={() => void flow.finish('sync')}
        data-testid="first-run-set-up-sync"
      >
        {tSync('intro.start')}
      </Button>
      {canBackup && <p className={cn(caption, 'mt-4')}>{t('firstRun.backupInstead')}</p>}
      <button
        type="button"
        className={cn(textLink, 'mt-3')}
        disabled={finishing}
        onClick={() => void flow.finish()}
        data-testid="first-run-not-now"
      >
        {t('biometrics.notNow')}
      </button>
    </div>
  );
}
