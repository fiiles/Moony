import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import { impactLines, type ReplaceFlow } from '@/utils/replace-impact';
import type { ReplaceImpactView } from '@shared/schema';

interface ReplaceImpactNoticeProps {
  flow: ReplaceFlow;
  impact: ReplaceImpactView | undefined;
  /** Reading the impact failed: its text is shown instead of the numbers. */
  error: Error | null;
}

/**
 * What a replace puts at stake, with the true counts, for the join, repair and re-sync
 * confirmations. Lines with a zero count are left out; the confirm stays off until `impact` is in.
 */
export function ReplaceImpactNotice({ flow, impact, error }: ReplaceImpactNoticeProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  if (error) {
    return (
      <p className="m-0 text-micro font-500 text-loss" role="alert">
        {translateApiError(error, tc)}
      </p>
    );
  }
  if (!impact) return <p className="m-0 text-body text-ink-3">{t('replace.loading')}</p>;
  // Unsent changes are one thing; what is on this device (documents and photos too) is another
  const lines = impactLines(flow, impact);
  const unsent = lines.filter((line) => line.key === 'items' || line.key === 'batches');
  const onDevice = lines.filter((line) => line.key !== 'items' && line.key !== 'batches');
  const hasAttachments = impact.documents + impact.photos > 0;
  const hasUnsent = flow === 'resync' && impact.pendingItems + impact.pendingBatches > 0;
  return (
    <div className="grid gap-2 text-body text-ink-2" data-testid="replace-impact">
      {unsent.length > 0 && (
        <ul className="m-0 list-disc pl-5 num">
          {unsent.map((line) => (
            <li key={line.key}>{t(`replace.lines.${line.key}`, { count: line.count })}</li>
          ))}
        </ul>
      )}
      {onDevice.length > 0 && (
        <div className="grid gap-1">
          <p className="m-0 font-600">{t('replace.onThisDevice')}</p>
          <ul className="m-0 list-disc pl-5 num">
            {onDevice.map((line) => (
              <li key={line.key}>{t(`replace.lines.${line.key}`, { count: line.count })}</li>
            ))}
          </ul>
        </div>
      )}
      {hasUnsent && <p className="m-0">{t('replace.unsent')}</p>}
      {hasAttachments && <p className="m-0">{t('replace.attachments')}</p>}
      <p className="m-0">{t('replace.backupWhere')}</p>
    </div>
  );
}
