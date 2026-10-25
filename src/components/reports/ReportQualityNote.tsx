import { useTranslation } from 'react-i18next';
import type { ConversionQuality, ReportQuality } from '@shared/schema';
import { cn } from '@/lib/utils';

export function ReportQualityNote({
  quality,
  planning = false,
  className,
}: {
  quality: ReportQuality;
  planning?: boolean;
  /** Extra classes for the note; the phone passes a `text-m-*` role (default: none). */
  className?: string;
}) {
  const { t } = useTranslation('reports');
  if (quality.missingFxCount === 0 && quality.invalidAmountCount === 0) return null;
  return (
    <div className={cn('mb-5 space-y-1 text-micro text-ink-4', className)} role="note">
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
export function ReportError({
  onRetry,
  className,
}: {
  onRetry: () => void;
  /** Extra classes for the box; the phone passes a `text-m-*` role (default: none). */
  className?: string;
}) {
  const { t } = useTranslation('reports');
  return (
    <div
      className={cn('mb-5 rounded-r3 bg-loss-soft p-4 text-table text-loss', className)}
      role="alert"
    >
      {t('readiness.reportError')}{' '}
      <button type="button" className="ml-3 font-650 underline" onClick={onRetry}>
        {t('readiness.retry')}
      </button>
    </div>
  );
}

/**
 * The compact report warning for a response's `fxQuality`: which currencies could not be
 * converted into `currency` and that the totals leave them out, and whether some amounts
 * use today's rate in place of their day's. Nothing when complete.
 */
export function ConversionQualityNote({
  quality,
  currency,
  className = 'mb-5',
}: {
  /** A response's quality, or just the currencies a view could not convert. */
  quality:
    | (Pick<ConversionQuality, 'missingCurrencies'> & Partial<Pick<ConversionQuality, 'estimated'>>)
    | undefined;
  currency: string;
  /** Spacing around the note (default: a gap below). */
  className?: string;
}) {
  const { t } = useTranslation('common');
  const missing = quality?.missingCurrencies ?? [];
  const estimated = quality?.estimated === true;
  if (missing.length === 0 && !estimated) return null;
  return (
    <div className={cn('space-y-1 text-micro', className)} role="note">
      {missing.length > 0 && (
        <p className="text-loss">
          {t('fxQuality.missing', { currency, currencies: missing.join(', ') })}
        </p>
      )}
      {estimated && <p className="text-ink-4">{t('fxQuality.estimated')}</p>}
    </div>
  );
}
