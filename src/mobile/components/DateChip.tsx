import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

/**
 * Width of the date chip (`.m-dchip`, prototype `.dchip`); the row's `chipWidth` for it: 54 pt at
 * the default text size, wider with the text.
 */
export const DATE_CHIP_WIDTH = 'calc(54px * var(--m-text-k))';

interface DateChipProps {
  /** The day the chip stands for (UTC unix seconds); shown as "20. 10." (day and month). */
  day: number;
  /** The line under the day: its distance ("za 4 dny"), its weekday ("so") or its year ("2026"). */
  caption: string;
  /** Dark material: a deadline at most 7 days away. */
  urgent?: boolean;
  /** Written instead of the day ("Dnes" for a target that is met today). */
  label?: string;
}

/**
 * The 54 × 42 date chip of a dated row (README §13, `.m-dchip`; spec §8.3 item 4): the day 12 / 650
 * over its caption in the note role (10 / 500), on `well` (radius 9), in the contrast material
 * (`dark` with `ink-inverse`) for an urgent date. Both sizes are those of the default text size: the
 * chip widens with the text role and is at least 42 pt tall, never fixed around its text. The agenda
 * row, the loan's events and the premiums coming up use it as the leading element of an `EntityRow`
 * (`chipWidth={DATE_CHIP_WIDTH}`). It is not hidden from screen readers: the row's sub-line leaves
 * the date out, the chip is where it is said. The caption is cut with an ellipsis as a last guard,
 * so nothing is painted outside the chip (a distance in words, "in 2 Monaten", is wider than 54 pt
 * in most languages: the agenda and the premiums coming up put the weekday here and the distance in
 * the row's sub-line).
 */
export function DateChip({ day, caption, urgent = false, label }: DateChipProps) {
  const fmt = useFormat();
  return (
    <span
      className={cn(
        'flex min-h-[42px] flex-none flex-col items-center justify-center rounded-[9px] py-0.5 leading-tight',
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
          'mt-px max-w-full overflow-hidden text-ellipsis whitespace-nowrap text-m-note',
          urgent ? 'text-ink-inverse-2' : 'text-ink-4'
        )}
      >
        {caption}
      </small>
    </span>
  );
}
