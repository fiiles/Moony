import { useId, useRef, useState } from 'react';
import { useIsMutating } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { ClipboardPaste, QrCode, RefreshCw, Type } from 'lucide-react';
import type { ReplaceOutcomeView } from '@shared/schema';
import { Input } from '@/components/ui/input';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { REPLACE_DEVICE_MUTATION_KEY } from '@/hooks/use-device-sync-mutations';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import type { PhoneJoinMethod } from '@/mobile/components/sync/JoinChoiceSheet';
import { RecoveryJoinSheet } from '@/mobile/components/sync/RecoveryJoinSheet';
import { ScanFlow } from '@/mobile/components/sync/ScanFlow';
import { SyncBackupSavedSheet } from '@/mobile/components/sync/SyncBackupSavedSheet';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import type { ReplaceSaved } from '@/mobile/lib/replace-saved';
import { phoneDeviceKey } from '@/mobile/lib/sync-scan';
import { StepHead } from './StepHead';
import { field48, fieldLabel, textLink } from './styles';

/**
 * Připojení, the join path's step after the recovery key (spec 2026-11-14 §5.2): the name this
 * phone joins under and the way in, as in Nastavení → Synchronizace ("Naskenovat kód z počítače",
 * which also takes a Recovery Kit QR, or "Zadat kód záchranné sady"; with the developer tools, a
 * pasted pairing code). The choice runs the flows Settings runs (`ScanFlow`, `RecoveryJoinSheet`)
 * in their first-run mode: the password typed on the account step, no replace warning and no
 * impact list (the profile is brand new).
 *
 * There is no way back: the account exists. "Pokračovat bez synchronizace" continues with the
 * normal steps, but only from the choice: never while a flow is open (a pairing starting, a join
 * running) or once the join is done. While the data is fetched the choice gives way to progress
 * and "Nezavírejte Moony". Without the password in memory (a lock closed the database) only the
 * way on without sync is offered.
 */
export function ConnectStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tSync } = useTranslation('deviceSync');
  const fieldId = useId();
  const devTools = useDeviceSyncStatus().data?.devTools === true;
  const [name, setName] = useState(() =>
    tSync(`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}`)
  );
  const [method, setMethod] = useState<PhoneJoinMethod | null>(null);
  /** A join that kept its backup: the sheet says where it is; the run goes on once it is closed. */
  const [saved, setSaved] = useState<ReplaceSaved | null>(null);
  const replacing = useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0;
  const password = flow.joinPassword;
  const deviceName = name.trim();
  const joinedNow = saved !== null;

  // The password went (a lock): a flow that is not joining yet is given up (unmounting it cancels
  // the camera and the pairing); a running join already has what it needs
  if (method !== null && password === null && !replacing) setMethod(null);

  const firstRun = { password: password ?? '', deviceName };
  const pick = (next: PhoneJoinMethod) => {
    if (deviceName === '' || password === null || replacing) return;
    setMethod(next);
  };

  /**
   * The join is done. A backup that is kept gets the sheet Settings shows (a join into a brand-new
   * profile is not kept, so normally the run just goes on).
   */
  const moved = useRef(false);
  const moveOn = () => {
    if (moved.current) return;
    moved.current = true;
    flow.joined();
  };
  const replaced = (headline: string, outcome: ReplaceOutcomeView) => {
    if (outcome.kept) setSaved({ headline, outcome });
    else moveOn();
  };

  const cannotLeave = method !== null || replacing || joinedNow;

  return (
    <div data-testid="first-run-connect">
      <StepHead title={t('onboarding.connect.title')} lead={t('onboarding.connect.subtitle')} />

      {replacing ? (
        <div
          role="status"
          className="mb-4 flex items-start gap-2.5 rounded-r3 bg-well px-4 py-3.5"
          data-testid="first-run-connect-fetching"
        >
          <RefreshCw
            className="mt-[3px] size-[13px] shrink-0 animate-spin text-ink-3 motion-reduce:animate-none"
            aria-hidden
          />
          <div className="text-m-sub">
            <b className="block font-650 text-ink">{t('onboarding.connect.fetching')}</b>
            <span className="text-ink-3">{t('onboarding.connect.keepOpen')}</span>
          </div>
        </div>
      ) : (
        password !== null &&
        !joinedNow && (
          <>
            <div className="mb-4">
              <label htmlFor={`${fieldId}-name`} className={fieldLabel}>
                {tSync('join.nameLabel')}
              </label>
              <Input
                id={`${fieldId}-name`}
                value={name}
                maxLength={60}
                onChange={(event) => setName(event.target.value)}
                autoComplete="off"
                enterKeyHint="done"
                className={field48}
              />
            </div>
            <EntityRows>
              <SettingsRow
                icon={QrCode}
                title={tSync('phone.choice.scan.title')}
                sub={tSync('phone.choice.scan.lead')}
                disabled={deviceName === ''}
                onClick={() => pick('scan')}
              />
              <SettingsRow
                icon={Type}
                title={tSync('phone.typeCode')}
                disabled={deviceName === ''}
                onClick={() => pick('kit')}
              />
              {devTools && (
                <SettingsRow
                  icon={ClipboardPaste}
                  title={tSync('phone.devPasteTitle')}
                  disabled={deviceName === ''}
                  onClick={() => pick('paste')}
                />
              )}
            </EntityRows>
          </>
        )
      )}

      {!joinedNow && (
        <button
          type="button"
          className={textLink}
          disabled={cannotLeave}
          onClick={flow.declineJoin}
          data-testid="first-run-connect-without-sync"
        >
          {t('onboarding.connect.withoutSync')}
        </button>
      )}

      {/* Mounted only while it runs: unmounting cancels the camera and gives the pairing up */}
      {(method === 'scan' || method === 'paste') && (
        <ScanFlow
          mode="join"
          paste={method === 'paste'}
          devTools={devTools}
          firstRun={firstRun}
          onDone={() => setMethod(null)}
          onReplaced={replaced}
        />
      )}
      <RecoveryJoinSheet
        open={method === 'kit'}
        onOpenChange={(open) => {
          if (!open) setMethod(null);
        }}
        firstRun={firstRun}
        onReplaced={replaced}
      />
      <SyncBackupSavedSheet saved={saved} onClose={moveOn} />
    </div>
  );
}
