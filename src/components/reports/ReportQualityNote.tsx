import { useRatesUpdatedAt } from '@/hooks/use-rates-updated-at';
import { useFormat } from '@/lib/use-format';
import { useTranslation } from 'react-i18next';
import type { ReportQuality } from '@shared/schema';
import { useReportMoney } from '@/lib/use-report-money';

export function ReportQualityNote({
  quality,
  averageMonths,
  estimate = false,
}: {
  quality: ReportQuality;
  averageMonths?: number;
  estimate?: boolean;
}) {
  const { t } = useTranslation('reports');
  const ratesUpdatedAt = useRatesUpdatedAt();
  const fmt = useFormat();
  const { displayCurrency, unsupported, converted } = useReportMoney();
  return (
    <div className="mb-5 space-y-1 text-micro text-ink-4" role="note">
      <p>
        {t(estimate ? 'readiness.planningEstimate' : 'readiness.historicalCzk')}{' '}
        {unsupported
          ? t('readiness.unsupportedCurrency')
          : converted
            ? t('readiness.currentDisplayRate', {
                currency: displayCurrency,
                date: ratesUpdatedAt
                  ? fmt.dateTime(ratesUpdatedAt, { hour: undefined, minute: undefined })
                  : t('readiness.rateDateUnknown'),
              })
            : null}
      </p>
      {!estimate && (
        <p>
          {t('readiness.observation', {
            completed: quality.observedCompletedMonths.length,
            partial: quality.partialMonthCount,
            missing: quality.unobservedMonthCount,
          })}
        </p>
      )}
      {averageMonths != null && (
        <p>
          {averageMonths === 0
            ? t('readiness.averageUnavailable')
            : t('readiness.averageSample', { count: averageMonths })}
        </p>
      )}
      {quality.missingFxCount > 0 && (
        <p className="text-loss">
          {t('readiness.missingFx', {
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
