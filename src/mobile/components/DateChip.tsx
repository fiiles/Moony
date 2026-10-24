import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

/** Width of the date chip (`.m-dchip`, prototype `.dchip`); the row's `chipWidth` for it. */
export const DATE_CHIP_WIDTH = 54;

interface DateChipProps {
  /** The day the chip stands for (UTC unix seconds); shown as "20. 10." (day and month). */
  day: number;
  /** The line under the day: its distance ("za 4 dny") or its year ("2026"). */
  caption: string;
  /** Dark material: a deadline at most 7 days away. */
  urgent?: boolean;
  /** Written instead of the day ("Dnes" for a target that is met today). */
  label?: string;
}

/**
 * The 54 × 42 date chip of a dated row (README §13, `.m-dchip`; spec §8.3 item 4): the day 12 / 650
 * over its caption 10 / 500, on `well` (radius 9), in dark material for an urgent date. The agenda
 * rows, the loan's events and the premiums coming up use it as the leading element of an
 * `EntityRow` (`chipWidth={DATE_CHIP_WIDTH}`). It is not hidden from screen readers: the row's
 * sub-line leaves the date out, the chip is where it is said.
 */
export function DateChip({ day, caption, urgent = false, label }: DateChipProps) {
  const fmt = useFormat();
  return (
    <span
      className={cn(
        'flex h-[42px] flex-none flex-col items-center justify-center rounded-[9px] leading-tight',
        urgent ? 'bg-dark-grad shadow-dark' : 'bg-well'
      )}
      style={{ width: DATE_CHIP_WIDTH }}
    >
      <b
        className={cn(
          'whitespace-nowrap text-m-sub font-650 tracking-[-0.01em] num',
          urgent ? 'text-ink-inverse' : 'text-ink'
        )}
      >
        {label ?? fmt.day(day, { day: 'numeric', month: 'numeric' })}
      </b>
      <small
        className={cn(
          'mt-px whitespace-nowrap text-micro font-medium',
          urgent ? 'text-ink-inverse-2' : 'text-ink-4'
        )}
      >
        {caption}
      </small>
    </span>
  );
}
