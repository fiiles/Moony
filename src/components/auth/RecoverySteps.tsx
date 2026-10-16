import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';

/** Three dots: which part of the recovery flow the user is in (lock screen, desktop and phone). */
export function RecoverySteps({ current }: { current: 1 | 2 | 3 }) {
  const { t } = useTranslation('auth');
  const labels = [t('recover.steps.key'), t('recover.steps.password'), t('recover.steps.newKey')];
  return (
    <ol className="mb-[18px] flex gap-1.5 text-micro font-600 text-ink-4">
      {labels.map((label, i) => {
        const on = i + 1 <= current;
        return (
          <li key={label} className={cn('flex items-center gap-1.5', on && 'text-ink')}>
            <i className={cn('block size-1.5 rounded-full bg-well-3', on && 'bg-dark')} />
            {label}
          </li>
        );
      })}
    </ol>
  );
}
