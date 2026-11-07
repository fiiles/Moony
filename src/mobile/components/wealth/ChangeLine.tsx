import type { ReactNode } from 'react';
import type { MoneyOptions } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { changeDirection } from '@/utils/change';
import { Money } from '@/mobile/components/Money';
import type { PeriodChange } from '@/mobile/lib/change';

interface ChangeLineProps {
  /** The change over the period; nothing is rendered without it (no base, or an unknown value). */
  change: PeriodChange | null;
  /** The currency the delta is in. */
  currency: string;
  /** Formatting of the amount; whole signed amounts unless told otherwise. */
  options?: MoneyOptions;
  /** Stands where the amount would: a date ("5. 10."), "dnes". */
  trail?: ReactNode;
  /** What the share measures, for VoiceOver only (a list that names it once for the eye). */
  srLabel?: string;
  className?: string;
}

/**
 * The change line of a row's trailing sub (README §13, `.m-change`): the badge — ▲ / ▼ and the
 * share, green or red, neutral when the share rounds to zero (`changeDirection`), absent without
 * a ratio — then the signed amount (through `<Money>`) or `trail`. The share is never colour
 * alone: the glyph carries the direction for the eye and a spoken sign for VoiceOver (the glyph is
 * hidden from it, the desktop's `ChangeBadge`).
 */
export function ChangeLine({
  change,
  currency,
  options,
  trail,
  srLabel,
  className,
}: ChangeLineProps) {
  const fmt = useFormat();
  if (change === null) return null;
  const { ratio } = change;
  // The direction of the share as it is written (one decimal): "0,0 %" is never green or red
  const direction = changeDirection(ratio, 1);
  const up = direction === 'up';
  const down = direction === 'down';
  return (
    <span className={cn('inline-flex items-center gap-1 whitespace-nowrap num', className)}>
      {ratio !== null && (
        <span
          className={cn(
            'inline-flex min-h-[18px] flex-none items-center rounded-r1 px-1.5 py-0.5 text-m-caption font-semibold leading-none',
            up ? 'bg-gain-soft text-gain' : down ? 'bg-loss-soft text-loss' : 'bg-well-2 text-ink-2'
          )}
        >
          {(up || down) && (
            <>
              <span aria-hidden>{up ? '▲ ' : '▼ '}</span>
              <span className="sr-only">{up ? '+' : '−'}</span>
            </>
          )}
          {fmt.percent(Math.abs(ratio), 1)}
        </span>
      )}
      {trail ?? (
        <Money
          value={change.delta}
          currency={currency}
          options={{ signed: true, decimals: 0, ...options }}
        />
      )}
      {srLabel && <span className="sr-only">{srLabel}</span>}
    </span>
  );
}
