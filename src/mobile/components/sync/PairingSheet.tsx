import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle } from 'lucide-react';
import type { PairingView } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { PairingExpiryNote } from '@/components/settings/sync/PairingExpiryNote';
import { usePairingExpired } from '@/hooks/use-pairing-expired';
import { useTypedCode } from '@/hooks/use-typed-code';
import { translateApiError } from '@/lib/translate-api-error';
import { formatComparisonCode, isCompleteCode } from '@/utils/pairing-code';
import { TypedCodeField } from '@/components/settings/sync/TypedCodeField';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { Sheet } from '@/mobile/components/Sheet';
import { pairingSheetStep } from '@/mobile/lib/sync-scan';

/** A failed join: its text, and whether the backend kept the pairing (so the same join can run again). */
export interface JoinFailure {
  message: string;
  retryable: boolean;
  /** The refusal was a half-finished setup on this phone (`syncJoinSetupPending`): it can be cancelled. */
  setupPending: boolean;
}

interface PairingSheetProps {
  /** Shown while a pairing is on; hidden while the join replaces the data (the busy cover is up). */
  open: boolean;
  view: PairingView;
  /** A failed poll or a confirm that ended the pairing (expired, cancelled, interfered). */
  pollError: Error | null;
  /** The last confirm's answer when the pairing goes on (a wrong code, a momentary failure). */
  confirmError: Error | null;
  /** Confirm: the holder's typed six digits, or the joiner's Continue (no argument). */
  onConfirm: (typedCode?: string) => void;
  confirming: boolean;
  /** Cancel, Close and every other way out: the caller gives the pairing up. */
  onClose: () => void;
  joinFailure: JoinFailure | null;
  /** After a failed join: the same join again (`retryable`), or a fresh pairing from the start. */
  onRetryJoin: () => void;
  onStartAgain: () => void;
  /** Cancels the half-finished setup that blocks the join; the caller then runs the join again. */
  onCancelSetup: () => void;
  cancellingSetup: boolean;
  /** `received`, before the join: what it puts at stake (`ReplaceImpactBlock`). */
  impact: ReactNode;
  /** The confirm "Join and replace" is on once the numbers it shows are in. */
  canJoin: boolean;
  /** "Join and replace": the join with the consent of the numbers on screen. */
  onJoin: () => void;
  /** What the person saw no longer held and was read again: said above the new numbers. */
  impactChanged: string | null;
}

/** The sheet's form for the holder's typed digits, so the footer's Confirm submits it. */
const TYPED_FORM = 'pairing-typed-code-form';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The pairing after this phone scanned the other device's code (the scanner, either role): "Connecting
 * securely…" while the answer is written and the other device's acknowledgement read, then the
 * compare step by role: the holder types the six digits the joining device shows (Confirm, with the
 * attempts left), the joiner shows its digits large with "Type this code on your other device.
 * Continue here once it says the code is right." and Continue. Then waiting, and for the holder
 * "Sent. Finish on the other device." The joiner's join runs as soon as the sync details arrive (the
 * caller hides this sheet behind the busy cover meanwhile; a failure brings it back with "Try again"
 * or "Start again"). A contested scan (another device answered the code first, review PF4) shows
 * only the warning and Cancel, and no digits. A pairing that ended (a failed poll, an expired code)
 * offers "Start again".
 */
export function PairingSheet({
  open,
  view,
  pollError,
  confirmError,
  onConfirm,
  confirming,
  onClose,
  joinFailure,
  onRetryJoin,
  onStartAgain,
  onCancelSetup,
  cancellingSetup,
  impact,
  canJoin,
  onJoin,
  impactChanged,
}: PairingSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const expired = usePairingExpired(view);
  const step = pairingSheetStep(view, { failed: pollError !== null, expired });
  const [typed, setTyped] = useTypedCode(confirmError);
  const confirmMessage = confirmError ? translateApiError(confirmError, tc) : null;

  const title =
    step === 'compare'
      ? view.role === 'holder'
        ? t('pairing.typeTitle')
        : t('pairing.showTitle')
      : view.role === 'holder'
        ? t('pairing.addTitle')
        : t('phone.scanTitle');

  const digits = (
    <div
      className="mx-auto w-fit rounded-r3 bg-dark px-6 py-4 text-m-display tracking-[0.12em] text-ink-inverse num"
      data-testid="pairing-comparison-code"
    >
      {formatComparisonCode(view.comparisonCode ?? '')}
    </div>
  );

  const body = (() => {
    switch (step) {
      case 'failed':
        return (
          <InlineAlert icon={AlertTriangle}>
            {translateApiError(pollError as Error, tc)}
          </InlineAlert>
        );
      case 'expired':
        return (
          <InlineAlert icon={AlertTriangle}>{tc('validation.syncPairingExpired')}</InlineAlert>
        );
      case 'contested':
        return <InlineAlert icon={AlertTriangle}>{t('pairing.contested')}</InlineAlert>;
      case 'compare':
        return view.role === 'holder' ? (
          <TypedCodeField
            phone
            formId={TYPED_FORM}
            value={typed}
            onChange={setTyped}
            onSubmit={onConfirm}
            attemptsLeft={view.attemptsLeft}
            errorMessage={confirmMessage}
            disabled={confirming}
          />
        ) : (
          <div className="space-y-3">
            {digits}
            <p className="m-0 text-center text-m-lead text-ink-2">{t('pairing.showPrompt')}</p>
            {confirmMessage && <InlineAlert icon={AlertTriangle}>{confirmMessage}</InlineAlert>}
          </div>
        );
      case 'connecting':
        return <p className="m-0 text-m-lead text-ink-3">{t('pairing.connecting')}</p>;
      case 'announcing':
      case 'waitingForScan':
        return <p className="m-0 text-m-lead text-ink-3">{t('phone.connecting')}</p>;
      case 'waitingForOther':
        return <p className="m-0 text-m-lead text-ink-3">{t('pairing.waiting')}</p>;
      case 'sent':
        return <p className="m-0 text-m-lead text-ink-2">{t('pairing.sent')}</p>;
      case 'received':
        return joinFailure ? (
          <div className="space-y-3">
            <InlineAlert icon={AlertTriangle}>{joinFailure.message}</InlineAlert>
            {joinFailure.setupPending && (
              <Button
                variant="secondary"
                className={`${BUTTON} w-full`}
                loading={cancellingSetup}
                onClick={onCancelSetup}
              >
                {t('intro.cancelSetup')}
              </Button>
            )}
          </div>
        ) : (
          // The join waits for the person: what is replaced, with the true counts, then the confirm
          <div className="space-y-3">
            <p className="m-0 text-m-lead text-ink-3">{t('pairing.readyToJoin')}</p>
            {impactChanged && <InlineAlert icon={AlertTriangle}>{impactChanged}</InlineAlert>}
            {impact}
            <PairingExpiryNote view={view} className="text-m-sub" />
          </div>
        );
    }
  })();

  const footer = (() => {
    if (step === 'compare') {
      return (
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            disabled={confirming}
            onClick={onClose}
          >
            {tc('buttons.cancel')}
          </Button>
          {view.role === 'holder' ? (
            <Button
              type="submit"
              form={TYPED_FORM}
              className={`${BUTTON} flex-1`}
              loading={confirming}
              disabled={!isCompleteCode(typed)}
            >
              {tc('buttons.confirm')}
            </Button>
          ) : (
            <Button className={`${BUTTON} flex-1`} loading={confirming} onClick={() => onConfirm()}>
              {tc('buttons.continue')}
            </Button>
          )}
        </>
      );
    }
    const finished = step === 'sent' || step === 'failed' || step === 'expired';
    return (
      <>
        <Button
          variant="ghost"
          className={`${BUTTON} px-[18px] ${step === 'received' ? '' : 'flex-1'}`}
          onClick={onClose}
        >
          {finished ? tc('buttons.close') : tc('buttons.cancel')}
        </Button>
        {(step === 'failed' || step === 'expired') && (
          <Button className={`${BUTTON} flex-1`} onClick={onStartAgain}>
            {t('pairing.startAgain')}
          </Button>
        )}
        {joinFailure && step === 'received' && (
          <Button
            className={`${BUTTON} flex-1`}
            onClick={joinFailure.retryable ? onRetryJoin : onStartAgain}
          >
            {joinFailure.retryable ? t('pairing.tryAgain') : t('pairing.startAgain')}
          </Button>
        )}
        {!joinFailure && step === 'received' && (
          <Button
            variant="destructive"
            className={`${BUTTON} flex-1`}
            disabled={!canJoin}
            onClick={onJoin}
          >
            {t('join.confirm')}
          </Button>
        )}
      </>
    );
  })();

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={title}
      footer={footer}
    >
      {body}
      {view.serverHost && step !== 'failed' && step !== 'expired' && (
        <p className="m-0 mt-3 text-center text-m-sub text-ink-4" data-testid="pairing-server-host">
          {t('pairing.serverHost', { host: view.serverHost })}
        </p>
      )}
    </Sheet>
  );
}
