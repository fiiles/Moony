import { useTranslation } from 'react-i18next';

/**
 * Caption under a trend chart that draws backfilled days dashed:
 * a dashed swatch plus one sentence on what "estimated" means.
 */
export default function EstimatedLegend() {
  const { t } = useTranslation('dashboard');
  return (
    <p className="mt-2 flex items-center gap-2 text-xs text-ink-3">
      <svg width="24" height="4" aria-hidden="true" className="shrink-0">
        <line
          x1="0"
          y1="2"
          x2="24"
          y2="2"
          stroke="currentColor"
          strokeWidth="2"
          strokeDasharray="5 3"
        />
      </svg>
      <span>{t('charts.estimatedLegend')}</span>
    </p>
  );
}
