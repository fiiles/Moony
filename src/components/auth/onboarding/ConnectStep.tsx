import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useIsMutating } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { REPLACE_DEVICE_MUTATION_KEY } from '@/hooks/use-device-sync-mutations';
import type { ReplaceOutcomeView } from '@shared/schema';
import type { JoinContext } from '@/utils/join-context';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { JoinChoice, type JoinMethod } from '@/components/settings/sync/join/JoinChoice';
import { PairingJoinFlow } from '@/components/settings/sync/join/PairingJoinFlow';
import { RecoveryJoinFlow } from '@/components/settings/sync/join/RecoveryJoinFlow';
import { StepHead } from '@/components/auth/onboarding/StepHead';
import { Button } from '@/components/ui/button';

interface ConnectStepProps {
  /** The device password typed on the account step; empty when it is no longer in memory. */
  password: string;
  onJoined: (outcome: ReplaceOutcomeView) => void;
  /** "Continue without sync": the normal steps follow. */
  onDecline: () => void;
}

/**
 * Připojení, the join path's last step (spec 2026-11-14 §5.1): this device's name and the way in
 * (a phone that syncs or the Sync Recovery Kit), then that join's flow as in Settings, without
 * the replace warning, the impact list and the password field (the profile is brand new and the
 * password was typed a step earlier). It has no way back; "Continue without sync" leaves it, but
 * never while a pairing starts or a join runs.
 */
export function ConnectStep({ password, onJoined, onDecline }: ConnectStepProps) {
  const { t } = useTranslation('auth');
  const { t: tSync } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [screen, setScreen] = useState<'choice' | 'flow'>('choice');
  const [name, setName] = useState(() => tSync(`setup.defaultName.${defaultDeviceKey()}`));
  const [method, setMethod] = useState<JoinMethod>('pair');
  // Set by the running flow: its own way out, which refuses while it is busy
  const leaveFlow = useRef<(() => boolean) | null>(null);
  // A join runs, or it is done and the wizard is finishing: the step cannot be left any more
  const [joined, setJoined] = useState(false);
  const busy = useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0 || joined;
  // The flow's own busy state, which also covers a pairing that is starting
  const [flowBusy, setFlowBusy] = useState(false);
  const cannotLeave = busy || (screen === 'flow' && flowBusy);
  const deviceName = name.trim();
  const context = useMemo<JoinContext>(() => ({ kind: 'firstRun', password }), [password]);

  // Only an unexpected remount loses the password (a finished join drops it too, and stays):
  // without it the step cannot join.
  const noPassword = password === '' && !joined;
  useEffect(() => {
    if (noPassword) onDecline();
  }, [noPassword, onDecline]);

  const proceed = (event: FormEvent) => {
    event.preventDefault();
    if (deviceName !== '') setScreen('flow');
  };

  // The flow gives a pairing up as it is left; the step's unmount cancels it again either way
  const decline = () => {
    if (cannotLeave) return;
    if (screen === 'flow' && leaveFlow.current && !leaveFlow.current()) return;
    onDecline();
  };

  const withoutSync = (
    <Button type="button" variant="ghost" disabled={cannotLeave} onClick={decline}>
      {t('onboarding.connect.withoutSync')}
    </Button>
  );
  const footer = (actions: ReactNode) => (
    <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
      {withoutSync}
      <div className="flex items-center gap-2">{actions}</div>
    </div>
  );

  if (noPassword) return null;
  const Flow = method === 'pair' ? PairingJoinFlow : RecoveryJoinFlow;
  return (
    <div className="onboarding-step" data-testid="onboarding-connect">
      <StepHead title={t('onboarding.connect.title')} lead={t('onboarding.connect.subtitle')} />
      {screen === 'choice' ? (
        <form onSubmit={proceed} className="grid gap-4">
          <JoinChoice name={name} onName={setName} method={method} onMethod={setMethod} />
          {footer(
            <Button
              type="submit"
              disabled={deviceName === ''}
              data-testid="onboarding-connect-continue"
            >
              {tc('buttons.continue')}
            </Button>
          )}
        </form>
      ) : (
        <>
          {busy && (
            <div
              role="status"
              className="mb-4 flex items-start gap-2.5 rounded-r3 bg-well px-4 py-3.5"
              data-testid="onboarding-connect-fetching"
            >
              <RefreshCw
                className="mt-[3px] size-[13px] shrink-0 animate-spin text-ink-3"
                aria-hidden
              />
              <div className="text-caption">
                <b className="block font-650 text-ink">{t('onboarding.connect.fetching')}</b>
                <span className="text-ink-3">{t('onboarding.connect.keepOpen')}</span>
              </div>
            </div>
          )}
          <Flow
            context={context}
            deviceName={deviceName}
            onJoined={(outcome) => {
              setJoined(true);
              onJoined(outcome);
            }}
            onCancel={() => setScreen('choice')}
            onBusyChange={setFlowBusy}
            footer={footer}
            dismissRef={leaveFlow}
          />
        </>
      )}
    </div>
  );
}
