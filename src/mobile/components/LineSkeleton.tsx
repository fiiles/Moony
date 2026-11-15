import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';

/**
 * A line of text still being read (audit 2026-11-15 E1): a skeleton bar in its place, announced as
 * loading. A count or a section never stands as "0" before its query has answered; a zero is a
 * result.
 */
export function LineSkeleton({ className }: { className?: string }) {
  const { t } = useTranslation('common');
  return (
    <span aria-busy="true" className="inline-flex h-[1.2em] items-center align-middle">
      <span aria-hidden className={cn('skeleton block h-[0.75em] w-24', className)} />
      <span className="sr-only">{t('status.loading')}</span>
    </span>
  );
}
