import { useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus, useReplaceImpact } from '@/hooks/use-device-sync';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { useServerChoice } from '@/hooks/use-server-choice';
import { consentOf } from '@/utils/replace-impact';
import { translateApiError } from '@/lib/translate-api-error';
import type { JoinFailureAction } from '@/utils/pairing-code';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { needsJoinAcknowledgement } from '@/utils/sync-setup';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { PairingExpiryNote } from '@/components/settings/sync/PairingExpiryNote';
import { PairingSteps } from '@/components/settings/sync/PairingSteps';
import { InsecureNotice } from '@/components/settings/sync/ServerChoice';
import { ReplaceImpactNotice } from '@/components/settings/sync/ReplaceImpactNotice';
import {
  IMPACT_CHANGED_KEY,
  replaceOutcomeToast,
} from '@/components/settings/sync/replace-outcome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { AlertTriangle } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

/**
 * Joining from another device's QR (the joiner): the device name, address and password first, then
 * the QR, the six digits to compare and, once the sync details arrive, the join itself (the busy
 * cover shows through the shared mutation key). Mounted only while open; closing gives up.
 */
export function JoinSyncDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const status = useDeviceSyncStatus().data;
  const pairing = useDevicePairing();
  const { error, isError, start, confirm, confirmError, cancel, join, afterFailedJoin } = pairing;
  const { turnOff } = useDeviceSyncMutations();
  // A successful join clears the pairing before the dialog closes: keep showing its last step.
  const lastView = useRef(pairing.view);
  if (pairing.view) lastView.current = pairing.view;
  const view = pairing.view ?? lastView.current;
  // Read when the sync details arrive and the confirmation opens; never cached.
  const impact = useReplaceImpact(view?.stage === 'received');
  const [name, setName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));
  const [password, setPassword] = useState('');
  // The mailbox is on the person's own server: an address or a connection code, checked first
  const server = useServerChoice('addressOrCode', 'server');
  /** The http server this dialog already warned about when it was typed: not asked again at join. */
  const [acknowledgedHost, setAcknowledgedHost] = useState<string | null>(null);
  /** The warning at `received` for a server the person did not type (the holder's), ticked. */
  const [joinAcknowledged, setJoinAcknowledged] = useState(false);
  const [impactChanged, setImpactChanged] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  /**
   * A failed join: its text, whether the backend kept the pairing (so the same join can run again)
   * and whether a half-finished setup blocks it ("Cancel setup").
   */
  const [joinError, setJoinError] = useState<({ message: string } & JoinFailureAction) | null>(
    null
  );

  const close = async () => {
    try {
      await cancel();
    } catch {
      // Gone on this device either way.
    }
    onClose();
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password === '' || name.trim() === '' || !server.ready || start.isPending) return;
    setFormError(null);
    try {
      await start.mutateAsync({
        password,
        deviceName: name.trim(),
        relayUrl: server.check?.address,
      });
      setAcknowledgedHost(server.check?.insecure && server.acknowledged ? server.check.host : null);
      setPassword('');
    } catch (failure) {
      start.reset();
      setFormError(translateApiError(failure as Error, tc));
    }
  };

  const runJoin = async () => {
    // The consent is that of the impact on screen; the join waits for the person's confirmation.
    if (!impact.data) return;
    setJoinError(null);
    setImpactChanged(false);
    try {
      const outcome = await join.mutateAsync(consentOf('join', impact.data));
      replaceOutcomeToast(t, t('pairing.joined', { path: outcome.backupPath }), outcome);
      onClose();
    } catch (failure) {
      if (failure instanceof ApiError && failure.key === IMPACT_CHANGED_KEY) {
        // What the person saw no longer holds: show the new numbers and ask again.
        setImpactChanged(true);
        await impact.refetch();
        return;
      }
      // The key alone cannot tell whether the backend kept the received session: ask it.
      const action = await afterFailedJoin(failure);
      setJoinError({ message: translateApiError(failure as Error, tc), ...action });
    }
  };

  // Continue: the joiner's confirm gate. A failure shows under the digits (`confirmError`).
  const handleConfirm = () => {
    confirm.mutate(undefined);
  };

  const retry = async () => {
    await runJoin();
  };

  // A half-finished setup on this device refuses the join: cancel it, then join again
  const cancelSetupAndRetry = async () => {
    try {
      await turnOff.mutateAsync();
    } catch (failure) {
      toast.error(tc('status.error'), { description: translateApiError(failure as Error, tc) });
      return;
    }
    await runJoin();
  };

  // The pairing ended with the failed join: back to the form for a fresh one.
  const startAgain = async () => {
    try {
      await cancel();
    } catch {
      // Gone on this device either way.
    }
    lastView.current = undefined;
    setJoinError(null);
    setJoinAcknowledged(false);
  };

  if (!view) {
    return (
      <Dialog open onOpenChange={(open) => !open && !start.isPending && void close()}>
        <DialogContent className="max-w-[560px]">
          <form onSubmit={submit} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>{t('join.pairTitle')}</DialogTitle>
              <DialogDescription>{t('join.pairHint')}</DialogDescription>
            </DialogHeader>
            <JoinWarning />
            <JoinFields
              name={name}
              onName={setName}
              password={password}
              onPassword={(value) => {
                setPassword(value);
                setFormError(null);
              }}
              server={server}
              serverMode="field"
              error={formError}
            />
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                disabled={start.isPending}
                onClick={() => void close()}
              >
                {tc('buttons.cancel')}
              </Button>
              <Button
                type="submit"
                loading={start.isPending}
                disabled={password === '' || name.trim() === '' || !server.ready}
              >
                {t('join.pairTitle')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    );
  }

  const failed = isError || view.contested;
  const askJoinAcknowledgement = needsJoinAcknowledgement(view, acknowledgedHost);
  return (
    <Dialog open onOpenChange={(open) => !open && !join.isPending && void close()}>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <DialogTitle>{t('join.pairTitle')}</DialogTitle>
          <DialogDescription>{t('join.warning')}</DialogDescription>
        </DialogHeader>
        <PairingSteps
          view={view}
          scanHint={t('join.pairHint')}
          devTools={status?.devTools === true}
          error={isError ? (error as Error) : null}
          confirmError={confirmError}
          onConfirm={handleConfirm}
          confirming={confirm.isPending}
          joining={join.isPending}
        />
        {view.stage === 'received' && !failed && (
          <>
            {impactChanged && (
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertDescription>{tc(IMPACT_CHANGED_KEY)}</AlertDescription>
              </Alert>
            )}
            <ReplaceImpactNotice
              flow="join"
              impact={impact.data}
              error={impact.isError ? (impact.error as Error) : null}
            />
            {askJoinAcknowledgement && view.joinServer && (
              <InsecureNotice
                id="join-insecure"
                acknowledged={joinAcknowledged}
                onChange={setJoinAcknowledged}
                body={t('join.insecureAtJoin', { host: view.joinServer })}
              />
            )}
            <PairingExpiryNote view={view} className="text-micro" />
          </>
        )}
        {joinError && (
          <Alert variant="destructive">
            <AlertTriangle />
            <AlertDescription>{joinError.message}</AlertDescription>
          </Alert>
        )}
        <DialogFooter>
          {joinError?.setupPending && !failed && (
            <Button
              type="button"
              variant="outline"
              onClick={cancelSetupAndRetry}
              loading={turnOff.isPending}
              disabled={join.isPending}
            >
              {t('intro.cancelSetup')}
            </Button>
          )}
          {isError && !joinError && (
            <Button type="button" onClick={() => void startAgain()}>
              {t('pairing.startAgain')}
            </Button>
          )}
          {joinError && !failed && (
            <Button
              type="button"
              onClick={joinError.retryable ? retry : startAgain}
              loading={join.isPending}
              disabled={turnOff.isPending}
            >
              {joinError.retryable ? t('pairing.tryAgain') : t('pairing.startAgain')}
            </Button>
          )}
          {view.stage === 'received' && !failed && !joinError && (
            <Button
              type="button"
              onClick={() => void runJoin()}
              loading={join.isPending}
              disabled={
                !impact.data || impact.isFetching || (askJoinAcknowledgement && !joinAcknowledged)
              }
            >
              {t('join.confirm')}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            disabled={join.isPending || turnOff.isPending}
            onClick={() => void close()}
          >
            {tc('buttons.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
