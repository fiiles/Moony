import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import type { PeriodSelection } from '@/hooks/use-period-selection';
import type { PeriodTimeframe } from '@/utils/period';
import { Button } from '@/components/ui/button';
import { Segmented } from '@/components/ui/segmented';

const TIMEFRAMES: readonly PeriodTimeframe[] = ['monthly', 'quarterly', 'yearly'];

/**
 * Month / quarter / year switch with ‹ period › navigation, shared by the
 * report pages (Cashflow, Budgets). "Next" stops at the current period.
 */
export function PeriodNav({ selection }: { selection: PeriodSelection }) {
  const { t } = useTranslation('common');
  const { timeframe, setTimeframe, offset, setOffset, label } = selection;
  return (
    <>
      <Segmented
        value={timeframe}
        onValueChange={setTimeframe}
        options={TIMEFRAMES.map((tf) => ({ value: tf, label: t(`periods.timeframe.${tf}`) }))}
        aria-label={t('periods.label')}
      />
      <div className="mx-1 inline-flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t(`periods.prev.${timeframe}`)}
          onClick={() => setOffset((o) => o - 1)}
        >
          <ChevronLeft />
        </Button>
        <b className="min-w-[110px] text-center text-body font-650 text-ink">{label}</b>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t(`periods.next.${timeframe}`)}
          disabled={offset >= 0}
          onClick={() => setOffset((o) => o + 1)}
        >
          <ChevronRight />
        </Button>
      </div>
    </>
  );
}
