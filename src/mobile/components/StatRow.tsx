import { useRef, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { useFitText } from '@/mobile/hooks/use-fit-text';

export interface StatItem {
  label: string;
  /** The shown text (masked while amounts are hidden); a string, so the fit can follow it. */
  value: string;
  /** 10 / 500 line under the value (a change, "na účtech"). */
  note?: ReactNode;
  /** Colours the value for a signed result (`.m-stat__v--gain` / `--loss`); the sign stays in the text. */
  tone?: 'gain' | 'loss';
}

/** The contract's rule (`.m-stat__v`): 17 px down to the 86 px box of a third, then with the card. */
const CONTAINER_SIZE = 'clamp(var(--m-fs-sub), 19.7cqi, var(--m-fs-stat))';

/**
 * Stat row (spec §10.3, README §13, `.m-stats`): three (or two, or one) stats, label 11 (may wrap,
 * hyphenated), value 17, note 10; the values sit on one line across the row. A value never wraps:
 * the contract's container rule shrinks it with a narrow card, and a value longer than its card
 * shrinks the whole row to fit (every stat in a row shares one size, floor `m-fs-sub`).
 */
export function StatRow({ stats }: { stats: StatItem[] }) {
  const rowRef = useRef<HTMLDivElement>(null);
  // The fit follows the shown values: hiding the amounts changes the text, not the numbers.
  const fit = useFitText(rowRef, stats.map((stat) => stat.value).join('\n'));

  return (
    <div
      ref={rowRef}
      className={cn(
        'mb-3.5 grid gap-2.5',
        stats.length === 1 ? 'grid-cols-1' : stats.length === 2 ? 'grid-cols-2' : 'grid-cols-3'
      )}
    >
      {stats.map((stat) => (
        <div
          key={stat.label}
          className="flex min-w-0 flex-col rounded-r3 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-3 py-2.5 shadow-e1 [container-type:inline-size]"
        >
          <span className="mb-[3px] hyphens-auto break-words text-m-caption text-ink-3">
            {stat.label}
          </span>
          <span
            data-fit-text
            className={cn(
              'mt-auto block overflow-hidden text-ellipsis whitespace-nowrap text-m-stat num',
              stat.tone === 'gain' ? 'text-gain' : stat.tone === 'loss' ? 'text-loss' : 'text-ink'
            )}
            style={{
              fontSize:
                fit === null
                  ? CONTAINER_SIZE
                  : `clamp(var(--m-fs-sub), min(${fit}px, 19.7cqi), var(--m-fs-stat))`,
            }}
          >
            <span>{stat.value}</span>
          </span>
          {stat.note !== undefined && stat.note !== null && (
            <span className="mt-[3px] truncate text-micro font-medium text-ink-4">{stat.note}</span>
          )}
        </div>
      ))}
    </div>
  );
}
