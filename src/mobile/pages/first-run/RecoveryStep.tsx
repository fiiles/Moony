import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Download, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import {
  pickVerificationGroups,
  recoveryKeyFileContents,
  recoveryKeyGroups,
  verifyGroups,
} from '@/utils/recovery-key-check';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';
import { RECOVERY_KEY_FILE, exportsOnThisDevice } from '@/mobile/lib/export-file';
import { StepHead } from './StepHead';
import { caption, field48, primaryButton, textLink } from './styles';

/**
 * Klíč: the recovery key as the dark box with "Zkopírovat" and, on iOS, "Uložit do Souborů" (the
 * key as a text file through the Files picker, staged in Rust), then the two groups the person
 * retypes from their copy. The encrypted database is created only once both match. The way back
 * discards the prepared key (the next "Pokračovat" prepares a new one), as on the desktop.
 */
export function RecoveryStep({ flow, recoveryKey }: { flow: FirstRunFlow; recoveryKey: string }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const { exportText } = useExportTextFile();
  const [saving, setSaving] = useState(false);
  const confirming = flow.pending.confirm;

  const verifyIndexes = useMemo<[number, number]>(
    () => pickVerificationGroups(recoveryKeyGroups(recoveryKey).length),
    [recoveryKey]
  );
  const [typed, setTyped] = useState<[string, string]>(['', '']);
  const verified = verifyGroups(recoveryKey, verifyIndexes, typed);
  const showMismatch = !verified && typed[0].length >= 4 && typed[1].length >= 4;

  const saveToFiles = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const outcome = await exportText(
        RECOVERY_KEY_FILE,
        recoveryKeyFileContents(recoveryKey, {
          title: t('onboarding.recovery.fileTitle'),
          note: t('onboarding.recovery.fileNote'),
        })
      );
      if (outcome === 'saved') toast(t('onboarding.recovery.saved'));
    } catch (error) {
      toast.error(tc('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), tc),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div data-testid="first-run-recovery">
      <StepHead title={t('onboarding.recovery.title')} lead={tm('firstRun.recoveryLead')} />

      <RecoveryKeyBox recoveryKey={recoveryKey} touch />
      {/* Where the Files picker does not export (not iOS) the key is copied; no button that only apologises */}
      {exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '') && (
        <Button
          type="button"
          variant="secondary"
          className={cn(primaryButton, 'mt-3 [&_svg]:size-[18px]')}
          loading={saving}
          disabled={saving || confirming}
          onClick={() => void saveToFiles()}
        >
          <Download aria-hidden />
          {tm('firstRun.saveToFiles')}
        </Button>
      )}

      <div className="mt-5 space-y-1.5">
        <Label>
          {t('onboarding.recovery.verifyLabel', {
            a: verifyIndexes[0] + 1,
            b: verifyIndexes[1] + 1,
          })}
        </Label>
        <div className="grid grid-cols-2 gap-3">
          {verifyIndexes.map((groupIndex, i) => (
            <Input
              key={groupIndex}
              aria-label={t('onboarding.recovery.groupLabel', { n: groupIndex + 1 })}
              value={typed[i]}
              maxLength={6}
              inputMode="text"
              autoCapitalize="characters"
              autoComplete="off"
              autoCorrect="off"
              spellCheck={false}
              enterKeyHint={i === 0 ? 'next' : 'done'}
              readOnly={confirming}
              className={cn(field48, 'uppercase tracking-[0.06em] num')}
              onChange={(event) => {
                const next: [string, string] = [typed[0], typed[1]];
                next[i] = event.target.value;
                setTyped(next);
              }}
            />
          ))}
        </div>
        <p
          role="status"
          className={cn(
            'm-0 px-0.5 pt-0.5 text-m-sub text-ink-4',
            showMismatch && 'font-semibold text-loss',
            verified && 'font-semibold text-gain'
          )}
        >
          {verified
            ? t('onboarding.recovery.verified')
            : showMismatch
              ? t('onboarding.recovery.mismatch')
              : t('onboarding.recovery.verifyHint')}
        </p>
      </div>

      <div className="mt-5">
        <Button
          className={primaryButton}
          disabled={!verified || confirming}
          loading={confirming}
          onClick={flow.createDatabase}
        >
          {t('onboarding.recovery.create')}
        </Button>
        <div aria-live="polite" className="empty:hidden">
          {confirming && (
            <p className={cn(caption, 'mt-3 flex items-start justify-center gap-2 text-ink-3')}>
              <RefreshCw className="mt-px size-3.5 shrink-0 animate-spin" aria-hidden />
              <span>{t('onboarding.recovery.creating')}</span>
            </p>
          )}
        </div>
      </div>
      <button type="button" className={textLink} onClick={flow.goBack} disabled={confirming}>
        {tc('buttons.back')}
      </button>
    </div>
  );
}
