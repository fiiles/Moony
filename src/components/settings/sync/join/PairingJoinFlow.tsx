import { useEffect, useRef, useState, type FormEvent, type ReactNode, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle } from 'lucide-react';
import type { ReplaceOutcomeView } from '@shared/schema';
import { useDeviceSyncStatus, useReplaceImpact } from '@/hooks/use-device-sync';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { useServerChoice } from '@/hooks/use-server-choice';
import { consentOf } from '@/utils/replace-impact';
import { translateApiError } from '@/lib/translate-api-error';
import { joinPassword, joinPresentation, type JoinContext } from '@/utils/join-context';
import type { JoinFailureAction } from '@/utils/pairing-code';
import { needsJoinAcknowledgement } from '@/utils/sync-setup';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { PairingExpiryNote } from '@/components/settings/sync/PairingExpiryNote';
import { PairingSteps } from '@/components/settings/sync/PairingSteps';
import { InsecureNotice } from '@/components/settings/sync/ServerChoice';
import { IMPACT_CHANGED_KEY } from '@/components/settings/sync/replace-outcome';
import { JoinImpactNotice } from '@/components/settings/sync/join/JoinImpactNotice';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

/** What both join flows take, in Settings (`JoinDeviceDialog`) and in the first run. */
export interface JoinFlowProps {
  context: JoinContext;
  /** Chosen on the JoinChoice screen; trimmed and non-empty. */
  deviceName: string;
  onJoined: (outcome: ReplaceOutcomeView) => void;
  /** Leaves the flow (Settings: closes the dialog; first run: back to the choice). Cancels a pairing first. */
  onCancel: () => void;
  /** Places the flow's buttons: Settings wraps them in `DialogFooter`, the wizard in its nav row. */
  footer: (actions: ReactNode) => ReactNode;
  /**
   * The caller's way to leave the flow from outside it: Settings calls it when the dialog is
   * dismissed (Escape, a click outside, its X), the first run before it leaves the Připojení step
   * ("Continue without sync"). The flow sets it to its own way out (`onCancel`, after giving a
   * pairing up), which does nothing while a pairing starts or a join runs; it answers whether it
   * is leaving.
   */
  dismissRef?: RefObject<(() => boolean) | null>;
}

/** Only the pairing flow: whether a pairing exists yet (its QR and later steps are on screen). */
export interface PairingJoinFlowProps extends JoinFlowProps {
  /** Settings drops the dialog's description once the QR shows its own hint. */
  onPairingShown?: (shown: boolean) => void;
}

/**
 * Joining from another device's QR (the joiner): the address (and in Settings the password)
 * first, then the QR, the six digits to compare and, once the sync details arrive, the join
 * itself (the busy cover shows through the shared mutation key). Mounted only while it runs;
 * leaving gives the pairing up.
 */
export function PairingJoinFlow({
  context,
  deviceName,
  onJoined,
  onCancel,
  footer,
  dismissRef,
  onPairingShown,
}: PairingJoinFlowProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const presentation = joinPresentation(context);
  const status = useDeviceSyncStatus().data;
  const pairing = useDevicePairing();
  const { error, isError, start, confirm, confirmError, cancel, join, afterFailedJoin } = pairing;
  const { turnOff } = useDeviceSyncMutations();
  // A successful join clears the pairing before the flow is left: keep showing its last step.
  const lastView = useRef(pairing.view);
  if (pairing.view) lastView.current = pairing.view;
  const view = pairing.view ?? lastView.current;
  const shown = view !== undefined;
  useEffect(() => onPairingShown?.(shown), [onPairingShown, shown]);
  // Read when the sync details arrive and the confirmation opens; never cached. Read in the first
  // run too, where it is not shown: the join sends it as its consent.
  const impact = useReplaceImpact(view?.stage === 'received');
  const [password, setPassword] = useState('');
  const devicePassword = joinPassword(context, password);
  // The mailbox is on the person's own server: an address or a connection code, checked first
  const server = useServerChoice('addressOrCode', 'server');
  /** The http server this flow already warned about when it was typed: not asked again at join. */
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
    onCancel();
  };

  // The caller's way out (Settings: a dismissal; first run: leaving the step): not while the pairing
  // starts (the password is on its way) or the join runs (a cancelled setup goes on to join)
  const busy = view ? join.isPending || turnOff.isPending : start.isPending;
  useEffect(() => {
    if (!dismissRef) return;
    dismissRef.current = () => {
      if (busy) return false;
      void close();
      return true;
    };
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (devicePassword === '' || deviceName === '' || !server.ready || start.isPending) return;
    setFormError(null);
    try {
      await start.mutateAsync({
        password: devicePassword,
        deviceName,
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
    // The consent is that of the impact read (on screen in Settings); the join waits for the
    // person's confirmation.
    if (!impact.data) return;
    setJoinError(null);
    setImpactChanged(false);
    try {
      const outcome = await join.mutateAsync(consentOf('join', impact.data));
      onJoined(outcome);
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
      <form onSubmit={submit} className="grid gap-4">
        {presentation.showReplaceWarning && <JoinWarning />}
        <JoinFields
          askPassword={presentation.askPassword}
          password={password}
          onPassword={(value) => {
            setPassword(value);
            setFormError(null);
          }}
          server={server}
          serverMode="field"
          error={formError}
        />
        {footer(
          <>
            <Button
              type="button"
              variant="ghost"
              disabled={start.isPending}
              onClick={() => void close()}
            >
              {tc(presentation.cancelKey)}
            </Button>
            <Button
              type="submit"
              loading={start.isPending}
              disabled={devicePassword === '' || deviceName === '' || !server.ready}
            >
              {t('join.pairTitle')}
            </Button>
          </>
        )}
      </form>
    );
  }

  const failed = isError || view.contested;
  const askJoinAcknowledgement = needsJoinAcknowledgement(view, acknowledgedHost);
  return (
    <div className="grid gap-4">
      {presentation.showReplaceWarning && <JoinWarning />}
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
          <JoinImpactNotice
            show={presentation.showImpact}
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
      {footer(
        <>
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
              {context.kind === 'firstRun' ? t('join.connect') : t('join.confirm')}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            disabled={join.isPending || turnOff.isPending}
            onClick={() => void close()}
          >
            {tc(presentation.cancelKey)}
          </Button>
        </>
      )}
    </div>
  );
}
