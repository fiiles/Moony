import { useTranslation } from 'react-i18next';
import { AlertTriangle } from 'lucide-react';
import type { PairingView } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { usePairingExpired } from '@/hooks/use-pairing-expired';
import { translateApiError } from '@/lib/translate-api-error';
import { formatComparisonCode } from '@/utils/pairing-code';
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
  /** A failed poll (expired, cancelled by the other device, unreachable). */
  pollError: Error | null;
  /** "They match": confirms the six digits. */
  onMatch: () => void;
  matching: boolean;
  /** Cancel, "They don't match", Close and every other way out: the caller gives the pairing up. */
  onClose: () => void;
  joinFailure: JoinFailure | null;
  /** After a failed join: the same join again (`retryable`), or a fresh pairing from the start. */
  onRetryJoin: () => void;
  onStartAgain: () => void;
  /** Cancels the half-finished setup that blocks the join; the caller then runs the join again. */
  onCancelSetup: () => void;
  cancellingSetup: boolean;
}

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The pairing after this phone scanned the other device's code (the scanner, either role): "Connecting…"
 * while the answer is written and read again, the six digits to compare, then waiting, and for the
 * holder "Sent. Finish on the other device." The joiner's join runs as soon as the sync details
 * arrive (the caller hides this sheet behind the busy cover meanwhile; a failure brings it back with
 * "Try again" or "Start again").
 *
 * The compare step says "Continue only if the other device shows these same six digits." and offers
 * "They match" and "They don't match" side by side, neither filled and neither focused, so no
 * reflexive tap confirms. A contested scan (another device answered the code first, review PF4)
 * shows this phone's own digits, which will not match, with the warning and only Cancel.
 */
export function PairingSheet({
  open,
  view,
  pollError,
  onMatch,
  matching,
  onClose,
  joinFailure,
  onRetryJoin,
  onStartAgain,
  onCancelSetup,
  cancellingSetup,
}: PairingSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const expired = usePairingExpired(view);
  const step = pairingSheetStep(view, { failed: pollError !== null, expired });
  const comparing = step === 'compare' || step === 'contested';

  const title = comparing
    ? t('pairing.compareTitle')
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
        return (
          <div className="space-y-3">
            {digits}
            <InlineAlert icon={AlertTriangle}>{t('pairing.contested')}</InlineAlert>
          </div>
        );
      case 'compare':
        return (
          <div className="space-y-3">
            {digits}
            <p className="m-0 text-center text-m-lead text-ink-2">
              {t('pairing.compareOnlyIfSame')}
            </p>
          </div>
        );
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
          <p className="m-0 text-m-lead text-ink-3">{t('pairing.joining')}</p>
        );
    }
  })();

  const footer = (() => {
    if (step === 'compare') {
      return (
        <>
          <Button
            variant="outline"
            className={`${BUTTON} flex-1`}
            disabled={matching}
            onClick={onClose}
          >
            {t('pairing.noMatch')}
          </Button>
          <Button
            variant="outline"
            className={`${BUTTON} flex-1`}
            loading={matching}
            onClick={onMatch}
          >
            {t('pairing.match')}
          </Button>
        </>
      );
    }
    const finished = step === 'sent' || step === 'failed' || step === 'expired';
    return (
      <>
        <Button
          variant="ghost"
          className={`${BUTTON} px-[18px] ${joinFailure && step === 'received' ? '' : 'flex-1'}`}
          onClick={onClose}
        >
          {finished ? tc('buttons.close') : tc('buttons.cancel')}
        </Button>
        {joinFailure && step === 'received' && (
          <Button
            className={`${BUTTON} flex-1`}
            onClick={joinFailure.retryable ? onRetryJoin : onStartAgain}
          >
            {joinFailure.retryable ? t('pairing.tryAgain') : t('pairing.startAgain')}
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
    </Sheet>
  );
}
