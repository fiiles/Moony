import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import type { PeriodTimeframe } from '@/utils/period';
import { Segment } from '@/mobile/components/Segment';
import { NavIconButton } from '@/mobile/shell/NavBar';

const TIMEFRAMES: readonly PeriodTimeframe[] = ['monthly', 'quarterly', 'yearly'];

interface PeriodBarProps {
  timeframe: PeriodTimeframe;
  /** Periods back from the current one: 0 is the current period. */
  offset: number;
  /** "Říjen 2026", "Q4 2026" or "2026". */
  label: string;
  onTimeframeChange: (timeframe: PeriodTimeframe) => void;
  /** -1 steps to the previous period, 1 to the next. */
  onStep: (delta: -1 | 1) => void;
}

/**
 * Period bar of the Cashflow tab (spec §8.7, README §13, `.m-period`): the full-width 36 pt
 * Měsíc · Čtvrtletí · Rok segment (the shared `Segment`, the hero's) and under it the stepper
 * (`.m-stepper`) ‹ Říjen 2026 ›: two 44 pt icon buttons around the label, "next" disabled at the
 * current period.
 */
export function PeriodBar({ timeframe, offset, label, onTimeframeChange, onStep }: PeriodBarProps) {
  const { t } = useTranslation('common');
  return (
    <div className="mb-1.5 mt-1">
      <Segment
        value={timeframe}
        options={TIMEFRAMES.map((tf) => ({ value: tf, label: t(`periods.timeframe.${tf}`) }))}
        onChange={onTimeframeChange}
        label={t('periods.label')}
        className="w-full"
      />
      <div className="mt-0.5 grid grid-cols-[44px_minmax(0,1fr)_44px] items-center">
        <NavIconButton aria-label={t(`periods.prev.${timeframe}`)} onClick={() => onStep(-1)}>
          <ChevronLeft strokeWidth={1.75} aria-hidden />
        </NavIconButton>
        <span
          aria-live="polite"
          className="min-w-0 truncate text-center text-m-row font-650 text-ink num"
        >
          {label}
        </span>
        <NavIconButton
          aria-label={t(`periods.next.${timeframe}`)}
          disabled={offset >= 0}
          onClick={() => onStep(1)}
          className="disabled:pointer-events-none disabled:opacity-45"
        >
          <ChevronRight strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </div>
    </div>
  );
}
