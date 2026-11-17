import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { cn } from '@/lib/utils';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { StepHead } from './StepHead';
import { caption, primaryButton, textLink } from './styles';
import { phoneCopyKey } from '@/mobile/lib/platform-copy';

/**
 * Záloha, the last step. A new profile has (almost) nothing to back up yet, so the step only says
 * where the data lives and how to keep a copy: an encrypted backup in Nastavení → Záloha a přenos
 * once there is data (the Přehled's getting-started card and the Údržba reminder ask for it later),
 * or sync. "Hotovo" ends the run. With sync built in (spec 2026-11-14 §5.2) the step is "Ochraňte
 * svá data" with a quieter line under the button: "Nastavte synchronizaci" ends the run and opens
 * Nastavení → Synchronizace.
 */
export function BackupStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const syncFeature = useDeviceSyncFeature();
  const finishing = flow.pending.finishing;

  return (
    <div data-testid="first-run-backup">
      {/* With sync built in the step also offers sync, so it is "protect", not only "back up" */}
      <StepHead
        title={syncFeature ? t('firstRun.protectTitle') : t('firstRun.backupTitle')}
        lead={t(phoneCopyKey('firstRun.backupLead'))}
      />
      <Button
        className={primaryButton}
        loading={finishing}
        disabled={finishing}
        onClick={() => void flow.finish()}
      >
        {tc('buttons.done')}
      </Button>
      {syncFeature && (
        <>
          <p className={cn(caption, 'mt-6 text-center')}>{t('firstRun.syncLead')}</p>
          <button
            type="button"
            className={cn(textLink, 'mt-0')}
            disabled={finishing}
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
