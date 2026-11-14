import { useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { ReplaceOutcomeView } from '@shared/schema';
import type { JoinContext } from '@/utils/join-context';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { replaceOutcomeToast } from '@/components/settings/sync/replace-outcome';
import { JoinChoice, type JoinMethod } from '@/components/settings/sync/join/JoinChoice';
import { PairingJoinFlow } from '@/components/settings/sync/join/PairingJoinFlow';
import { RecoveryJoinFlow } from '@/components/settings/sync/join/RecoveryJoinFlow';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

const SETTINGS: JoinContext = { kind: 'settings' };

const inFooter = (actions: ReactNode) => <DialogFooter>{actions}</DialogFooter>;

/**
 * Settings → Sync, "Connect this device": the device name and the way in (a phone that syncs or
 * the Sync Recovery Kit), then that join's flow. Here the profile may hold data, so the flows ask
 * for the device password and show the replace warning and what is at stake. Mounted only while
 * open; it cannot be dismissed while a pairing starts or a join runs.
 */
export function JoinDeviceDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [screen, setScreen] = useState<'choice' | 'flow'>('choice');
  const [name, setName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));
  const [method, setMethod] = useState<JoinMethod>('pair');
  // Once a pairing exists its QR carries the phone hint: the header leaves its description out
  const [pairingShown, setPairingShown] = useState(false);
  // Set by the running flow: its own way out, a no-op while it is busy
  const dismissFlow = useRef<(() => void) | null>(null);
  const deviceName = name.trim();

  const proceed = (event: FormEvent) => {
    event.preventDefault();
    if (deviceName !== '') setScreen('flow');
  };

  const joined = (outcome: ReplaceOutcomeView) => {
    replaceOutcomeToast(t, t('pairing.joined', { path: outcome.backupPath }), outcome);
    onClose();
  };

  const dismiss = () => {
    if (screen === 'choice') onClose();
    else dismissFlow.current?.();
  };

  const flowProps = {
    context: SETTINGS,
    deviceName,
    onJoined: joined,
    onCancel: onClose,
    footer: inFooter,
    dismissRef: dismissFlow,
  };
  const hint = method === 'kit' ? t('join.kitHint') : pairingShown ? null : t('join.pairHint');
  // A screen without a description says so, or Radix warns about a missing one
  const noDescription = screen === 'flow' && hint === null;
  return (
    <Dialog open onOpenChange={(open) => !open && dismiss()}>
      <DialogContent
        className="max-w-[560px]"
        {...(noDescription ? { 'aria-describedby': undefined } : {})}
      >
        {screen === 'choice' ? (
          <form onSubmit={proceed} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>{t('intro.joinTitle')}</DialogTitle>
              <DialogDescription>{t('intro.joinLead')}</DialogDescription>
            </DialogHeader>
            <JoinChoice name={name} onName={setName} method={method} onMethod={setMethod} />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                {tc('buttons.cancel')}
              </Button>
              <Button type="submit" disabled={deviceName === ''}>
                {tc('buttons.continue')}
              </Button>
            </DialogFooter>
          </form>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>
                {method === 'pair' ? t('join.pairTitle') : t('join.kitTitle')}
              </DialogTitle>
              {hint !== null && <DialogDescription>{hint}</DialogDescription>}
            </DialogHeader>
            {method === 'pair' ? (
              <PairingJoinFlow {...flowProps} onPairingShown={setPairingShown} />
            ) : (
              <RecoveryJoinFlow {...flowProps} />
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
