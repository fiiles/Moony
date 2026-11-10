import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle, Check, Copy } from 'lucide-react';
import type { PairingView } from '@shared/schema';
import { translateApiError } from '@/lib/translate-api-error';
import { usePairingExpired } from '@/hooks/use-pairing-expired';
import { formatComparisonCode } from '@/utils/pairing-code';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

interface PairingStepsProps {
  view: PairingView;
  /** The text shown under the QR: where to scan it (differs per role). */
  scanHint: string;
  /** The developer tools are on (`status.devTools`): the code is offered as text too. */
  devTools: boolean;
  /** A failed poll or action (expired, cancelled by the other device, unreachable). */
  error: Error | null;
  /** "They match": confirms the six digits. */
  onMatch: () => void;
  matching: boolean;
  /** "They don't match": the caller cancels the pairing and closes. */
  onNoMatch: () => void;
}

/**
 * The part of a pairing both dialogs share: the QR while the code is awaited, the six digits to
 * compare, then waiting for the other device. Only a contested view (another device answered
 * first) shows no choice: the dialog's Cancel is the only way on.
 */
export function PairingSteps({
  view,
  scanHint,
  devTools,
  error,
  onMatch,
  matching,
  onNoMatch,
}: PairingStepsProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const expired = usePairingExpired(view);
  const [copied, setCopied] = useState(false);

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

  switch (view.stage) {
    case 'announcing':
      return <p className="m-0 text-body text-ink-3">{t('pairing.preparing')}</p>;
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
          <p className="m-0 text-center text-micro font-500 text-ink-4">{t('pairing.validFor')}</p>
          {devTools && view.pairingCode && (
            <Button type="button" variant="outline" size="sm" onClick={copyCode}>
              {copied ? <Check /> : <Copy />}
              {t('pairing.copyCodeDev')}
            </Button>
          )}
        </div>
      );
    case 'compare':
      return (
        <div className="grid justify-items-center gap-3">
          <div
            className="rounded-r3 bg-dark px-6 py-4 text-[32px] font-650 tracking-[0.12em] text-ink-inverse num"
            data-testid="pairing-comparison-code"
          >
            {formatComparisonCode(view.comparisonCode ?? '')}
          </div>
          <p className="m-0 text-center text-body text-ink-2">{t('pairing.compareOnlyIfSame')}</p>
          <div className="flex gap-2">
            <Button type="button" variant="outline" onClick={onMatch} loading={matching}>
              {t('pairing.match')}
            </Button>
            {/* Focus starts here, so a reflexive Enter cannot confirm the digits. */}
            <Button
              type="button"
              variant="outline"
              onClick={onNoMatch}
              disabled={matching}
              autoFocus
            >
              {t('pairing.noMatch')}
            </Button>
          </div>
        </div>
      );
    case 'waitingForOther':
      return <p className="m-0 text-body text-ink-3">{t('pairing.waiting')}</p>;
    case 'sent':
      return <p className="m-0 text-body text-ink-2">{t('pairing.sent')}</p>;
    case 'received':
      return <p className="m-0 text-body text-ink-3">{t('pairing.joining')}</p>;
  }
}
