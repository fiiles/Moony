import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { NavIconButton } from '@/mobile/shell/NavBar';

interface YearBarProps {
  year: number;
  minYear: number;
  maxYear: number;
  onStep: (year: number) => void;
}

/**
 * The year stepper of the plans list (the Cashflow `PeriodBar`'s stepper, `.m-stepper`): two 44 pt
 * icon buttons around the year, "previous" and "next" disabled at the ends of the range.
 */
export function YearBar({ year, minYear, maxYear, onStep }: YearBarProps) {
  const { t } = useTranslation('spending_plans');
  return (
    <div
      role="group"
      aria-label={t('year.label')}
      className="mb-1.5 mt-1 grid grid-cols-[44px_minmax(0,1fr)_44px] items-center"
    >
      <NavIconButton
        aria-label={t('year.prev')}
        disabled={year <= minYear}
        onClick={() => onStep(year - 1)}
        className="disabled:pointer-events-none disabled:opacity-45"
      >
        <ChevronLeft strokeWidth={1.75} aria-hidden />
      </NavIconButton>
      <span
        aria-live="polite"
        className="min-w-0 truncate text-center text-m-row font-650 text-ink num"
      >
        {year}
      </span>
      <NavIconButton
        aria-label={t('year.next')}
        disabled={year >= maxYear}
        onClick={() => onStep(year + 1)}
        className="disabled:pointer-events-none disabled:opacity-45"
      >
        <ChevronRight strokeWidth={1.75} aria-hidden />
      </NavIconButton>
    </div>
  );
}
