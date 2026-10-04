import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { useHistoryRecalculation } from '@/hooks/use-history-recalculation';

/**
 * Header badge shown while the backend rebuilds a stock ticker's history after a transaction
 * change. Same pattern as `SyncStatusBadge`: nothing when idle.
 */
export function HistoryRecalculationBadge() {
  const { t } = useTranslation('common');
  const running = useHistoryRecalculation();

  if (running.length === 0) {
    return null;
  }

  return (
    <Badge
      variant="secondary"
      className="gap-1.5 cursor-default"
      role="status"
      aria-label={t('sync.recalculatingHistory')}
    >
      <Loader2 className="w-3 h-3 animate-spin" />
      <span className="hidden sm:inline">{t('sync.recalculatingHistory')}</span>
    </Badge>
  );
}
