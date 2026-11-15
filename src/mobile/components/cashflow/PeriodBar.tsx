import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronLeft, ChevronRight } from 'lucide-react';
import type { PeriodTimeframe } from '@/utils/period';
import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuRadioItem } from '@/mobile/components/RowMenu';
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
  /** Under the label: whose money the report is ("Všechny účty", "3 účty"); a tap opens the choice. */
  scope?: { label: string; onOpen: () => void };
  /**
   * The period is not decided yet (the tab waits for the month of the newest transaction): the
   * label keeps its place but is not shown, and the stepper waits too.
   */
  pending?: boolean;
}

/**
 * Period bar of the Cashflow tab (spec §8.7, README §13, `.m-period`, audit 2026-11-15 D17): one row
 * ‹ Říjen 2026 ⌄ ›, two 44 pt icon buttons around the label, "next" disabled at the current period.
 * The label is a menu button that picks Měsíc · Čtvrtletí · Rok, and under it the account scope is
 * a small text button that opens the accounts sheet — what used to be three rows of controls (a
 * segment, the stepper and a sentence) is one. While `pending`, the label is hidden and the
 * steppers are off.
 */
export function PeriodBar({
  timeframe,
  offset,
  label,
  onTimeframeChange,
  onStep,
  scope,
  pending = false,
}: PeriodBarProps) {
  const { t } = useTranslation('common');
  return (
    <div className="mb-2.5 grid grid-cols-[44px_minmax(0,1fr)_44px] items-center">
      <NavIconButton
        aria-label={t(`periods.prev.${timeframe}`)}
        disabled={pending}
        onClick={() => onStep(-1)}
        className="disabled:pointer-events-none disabled:opacity-45"
      >
        <ChevronLeft strokeWidth={1.75} aria-hidden />
      </NavIconButton>
      <div className="flex min-w-0 flex-col items-center">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              disabled={pending}
              aria-label={`${t('periods.label')}: ${label}`}
              className={cn(
                'relative inline-flex min-h-11 max-w-full items-center gap-1 rounded-[10px] px-2 text-m-row font-650 text-ink [-webkit-touch-callout:none] active:bg-well-2 focus-visible:outline-none focus-visible:shadow-focus num',
                pending && 'invisible'
              )}
            >
              <span aria-live="polite" className="min-w-0 truncate">
                {label}
              </span>
              <ChevronDown aria-hidden strokeWidth={1.75} className="size-4 flex-none text-ink-3" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" className="min-w-[200px]">
            <DropdownMenuRadioGroup
              value={timeframe}
              onValueChange={(next) => onTimeframeChange(next as PeriodTimeframe)}
            >
              {TIMEFRAMES.map((tf) => (
                <RowMenuRadioItem key={tf} value={tf}>
                  {t(`periods.timeframe.${tf}`)}
                </RowMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        {scope && (
          <button
            type="button"
            onClick={scope.onOpen}
            className="relative -mt-1.5 max-w-full truncate text-m-caption font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-2.5 after:content-[''] active:text-ink"
          >
            {scope.label}
          </button>
        )}
      </div>
      <NavIconButton
        aria-label={t(`periods.next.${timeframe}`)}
        disabled={pending || offset >= 0}
        onClick={() => onStep(1)}
        className="disabled:pointer-events-none disabled:opacity-45"
      >
        <ChevronRight strokeWidth={1.75} aria-hidden />
      </NavIconButton>
    </div>
  );
}
