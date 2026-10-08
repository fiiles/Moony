import { useTranslation } from 'react-i18next';
import type { ReportQuality } from '@shared/schema';

export function ReportQualityNote({
  quality,
  planning = false,
}: {
  quality: ReportQuality;
  planning?: boolean;
}) {
  const { t } = useTranslation('reports');
  if (quality.missingFxCount === 0 && quality.invalidAmountCount === 0) return null;
  return (
    <div className="mb-5 space-y-1 text-micro text-ink-4" role="note">
      {quality.missingFxCount > 0 && (
        <p className="text-loss">
          {t(planning ? 'readiness.missingPlanningFx' : 'readiness.missingFx', {
            count: quality.missingFxCount,
            currencies: quality.missingFxCurrencies.join(', '),
          })}
        </p>
      )}
      {quality.invalidAmountCount > 0 && (
        <p className="text-loss">
          {t('readiness.invalidAmounts', { count: quality.invalidAmountCount })}
        </p>
      )}
    </div>
  );
}
export function ReportError({ onRetry }: { onRetry: () => void }) {
  const { t } = useTranslation('reports');
  return (
    <div className="mb-5 rounded-r3 bg-loss-soft p-4 text-table text-loss" role="alert">
      {t('readiness.reportError')}{' '}
      <button type="button" className="ml-3 font-650 underline" onClick={onRetry}>
        {t('readiness.retry')}
      </button>
    </div>
  );
}
