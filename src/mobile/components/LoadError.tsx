import type { ReactNode } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { retryFailedQueries, useActiveQueryFailed } from '@/mobile/hooks/use-failed-queries';

/**
 * The phone's one "could not be loaded" (audit C6): the sentence of what failed and "Zkusit znovu",
 * in place of what could not be shown, announced as an alert. Never an empty state or "not found":
 * those would tell the person something untrue. The retry loads again every query on the screen
 * that failed, unless the caller has its own.
 */
export function LoadError({ message, onRetry }: { message: string; onRetry?: () => void }) {
  const { t } = useTranslation('reports');
  const client = useQueryClient();
  return (
    <div role="alert" className="mx-0.5 flex flex-col items-center gap-3 py-8 text-center">
      <p className="m-0 text-balance text-m-body text-ink-3">{message}</p>
      <Button
        variant="secondary"
        className="h-auto min-h-12 whitespace-normal rounded-[10px] px-5 py-2 text-m-row [&_svg]:size-4"
        onClick={() => (onRetry ? onRetry() : void retryFailedQueries(client))}
      >
        <RefreshCw aria-hidden />
        {t('readiness.retry')}
      </Button>
    </div>
  );
}

/**
 * A detail whose record is not there: "… nenalezen" (`children`) when the load answered without
 * it, `LoadError` when a query of the screen failed instead.
 */
export function MissingOrFailed({ children }: { children: ReactNode }) {
  const { t } = useTranslation('mobile');
  const failed = useActiveQueryFailed();
  if (failed) return <LoadError message={t('loadError.detail')} />;
  return <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{children}</p>;
}
