import { useTranslation } from 'react-i18next';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';

/**
 * Placeholder for a trend chart whose history is still loading: a block of the
 * chart's height instead of an empty axis or a few isolated markers. Render the real chart only
 * once the line data has arrived.
 */
export function ChartSkeleton({
  height = 300,
  className,
}: {
  height?: number;
  className?: string;
}) {
  const { t } = useTranslation('common');
  return (
    <Skeleton
      role="status"
      aria-busy="true"
      aria-label={t('status.loading')}
      className={cn('w-full', className)}
      style={{ height }}
    />
  );
}

/**
 * Shown instead of a chart that has fewer than two points: one point draws no line, so say why
 * rather than render an empty axis.
 */
export function ChartNotEnoughHistory({
  height = 300,
  className,
}: {
  height?: number;
  className?: string;
}) {
  const { t } = useTranslation('common');
  return (
    <div
      className={cn(
        'flex w-full items-center justify-center px-6 text-center text-sm text-ink-3',
        className
      )}
      style={{ height }}
    >
      {t('charts.notEnoughHistory')}
    </div>
  );
}
