import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';

/**
 * Why an action that needs the sync server waits: "Server není dostupný. Zkuste to, až bude
 * spojení." Its `id` describes the waiting buttons (`useServerGate`).
 */
export function ServerNeededNote({ id, className }: { id: string; className?: string }) {
  const { t } = useTranslation('deviceSync');
  return (
    <p
      id={id}
      className={cn('m-0 text-micro font-500 text-ink-3', className)}
      data-testid="server-needed"
    >
      {t('status.needsServer')}
    </p>
  );
}
