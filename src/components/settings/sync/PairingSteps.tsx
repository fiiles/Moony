import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle, Check, Copy } from 'lucide-react';
import type { PairingView } from '@shared/schema';
import { translateApiError } from '@/lib/translate-api-error';
import { usePairingExpired } from '@/hooks/use-pairing-expired';
import { useTypedCode } from '@/hooks/use-typed-code';
import { formatComparisonCode, isCompleteCode } from '@/utils/pairing-code';
import { TypedCodeField } from '@/components/settings/sync/TypedCodeField';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

interface PairingStepsProps {
  view: PairingView;
  /** The text shown under the QR: where to scan it (differs per role). */
  scanHint: string;
  /** The developer tools are on (`status.devTools`): the code is offered as text too. */
  devTools: boolean;
  /** A failed poll or a confirm that ended the pairing (expired, cancelled, interfered). */
  error: Error | null;
  /** The last confirm's answer when the pairing goes on (a wrong code, a momentary failure). */
  confirmError: Error | null;
  /** Confirm: the holder's typed six digits, or the joiner's Continue (no argument). */
  onConfirm: (typedCode?: string) => void;
  confirming: boolean;
  /** `received`: false while the join waits for the person's confirmation (default: running). */
  joining?: boolean;
}

/**
 * The part of a pairing both dialogs share: the QR while the code is awaited, "Connecting
 * securely…", then at `compare` the holder's six-digit field (the person types the digits the
 * joining device shows) or the joiner's large digits with Continue, then waiting for the other
 * device. A contested view (another device answered first) offers no choice: the dialog's Cancel
 * is the only way on. A sync server that is not the default is named under every step.
 */
export function PairingSteps({
  view,
  scanHint,
  devTools,
  error,
  confirmError,
  onConfirm,
  confirming,
  joining = true,
}: PairingStepsProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const expired = usePairingExpired(view);
  const [copied, setCopied] = useState(false);
  const [typed, setTyped] = useTypedCode(confirmError);

  const waitingStage = view.stage !== 'sent' && view.stage !== 'received';
  if (error) {
    return (
      <Alert variant="destructive">
        <AlertTriangle />
        <AlertDescription>{translateApiError(error, tc)}</AlertDescription>
      </Alert>
    );
  }
  if (expired && waitingStage) {
    return (
      <Alert variant="destructive">
        <AlertTriangle />
        <AlertDescription>{tc('validation.syncPairingExpired')}</AlertDescription>
      </Alert>
    );
  }

  const copyCode = async () => {
    if (!view.pairingCode) return;
    try {
      await navigator.clipboard.writeText(view.pairingCode);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (failure) {
      toast.error(tc('status.error'), { description: translateApiError(failure as Error, tc) });
    }
  };

  if (view.contested) {
    return (
      <Alert variant="destructive">
        <AlertTriangle />
        <AlertDescription>{t('pairing.contested')}</AlertDescription>
      </Alert>
    );
  }

  const content = (() => {
    switch (view.stage) {
      case 'announcing':
        return <p className="m-0 text-body text-ink-3">{t('pairing.preparing')}</p>;
      case 'connecting':
        return <p className="m-0 text-body text-ink-3">{t('pairing.connecting')}</p>;
      case 'waitingForScan':
        return (
          <div className="grid justify-items-center gap-3">
            {view.qrSvg && (
              <img
                alt={t('pairing.qrAlt')}
                className="size-[220px] rounded-r2 bg-white p-2"
                src={`data:image/svg+xml;base64,${btoa(view.qrSvg)}`}
              />
            )}
            <p className="m-0 text-center text-body text-ink-2">{scanHint}</p>
            <p className="m-0 text-center text-micro font-500 text-ink-4">
              {t('pairing.validFor')}
            </p>
            {devTools && view.pairingCode && (
              <Button type="button" variant="outline" size="sm" onClick={copyCode}>
                {copied ? <Check /> : <Copy />}
                {t('pairing.copyCodeDev')}
              </Button>
            )}
          </div>
        );
      case 'compare':
        if (view.role === 'holder') {
          return (
            <TypedCodeField
              value={typed}
              onChange={setTyped}
              onSubmit={onConfirm}
              attemptsLeft={view.attemptsLeft}
              errorMessage={confirmError ? translateApiError(confirmError, tc) : null}
              disabled={confirming}
            >
              <div className="flex justify-center">
                <Button type="submit" loading={confirming} disabled={!isCompleteCode(typed)}>
                  {tc('buttons.confirm')}
                </Button>
              </div>
            </TypedCodeField>
          );
        }
        return (
          <div className="grid justify-items-center gap-3">
            <div
              className="rounded-r3 bg-dark px-6 py-4 text-[32px] font-650 tracking-[0.12em] text-ink-inverse num"
              data-testid="pairing-comparison-code"
            >
              {formatComparisonCode(view.comparisonCode ?? '')}
            </div>
            <p className="m-0 text-center text-body text-ink-2">{t('pairing.showPrompt')}</p>
            {confirmError && (
              <p role="alert" className="m-0 text-center text-micro text-loss">
                {translateApiError(confirmError, tc)}
              </p>
            )}
            <Button type="button" onClick={() => onConfirm()} loading={confirming}>
              {tc('buttons.continue')}
            </Button>
          </div>
        );
      case 'waitingForOther':
        return <p className="m-0 text-body text-ink-3">{t('pairing.waiting')}</p>;
      case 'sent':
        return <p className="m-0 text-body text-ink-2">{t('pairing.sent')}</p>;
      case 'received':
        return (
          <p className="m-0 text-body text-ink-3">
            {joining ? t('pairing.joining') : t('pairing.readyToJoin')}
          </p>
        );
    }
  })();

  return (
    <div className="grid gap-3">
      {content}
      {view.serverHost && (
        <p className="m-0 text-center text-micro text-ink-4" data-testid="pairing-server-host">
          {t('pairing.serverHost', { host: view.serverHost })}
        </p>
      )}
    </div>
  );
}
