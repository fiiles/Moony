import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

export interface StatItem {
  label: string;
  value: ReactNode;
  /** 10 / 500 line under the value (a change, "na účtech"). */
  note?: ReactNode;
}

/** The value's size limits: `m-fs-stat` and `m-fs-sub` (README §13). */
const MAX_PX = 17;
const MIN_PX = 12;
/** The contract's rule (`.m-stat__v`): 17 px down to the 86 px box of a third, then with the card. */
const CONTAINER_SIZE = 'clamp(var(--m-fs-sub), 19.7cqi, var(--m-fs-stat))';

/**
 * Stat row (spec §10.3, README §13, `.m-stats`): three (or two) stats, label 11 (may wrap,
 * hyphenated), value 17, note 10; the values sit on one line across the row. A value never wraps:
 * the contract's container rule shrinks it with a narrow card, and a value longer than its card
 * shrinks the whole row to fit (every stat in a row shares one size, floor 12 px).
 */
export function StatRow({ stats }: { stats: StatItem[] }) {
  const rowRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState<number | null>(null);

  useLayoutEffect(() => {
    const row = rowRef.current;
    if (!row) return;
    const measure = () => {
      let size = MAX_PX;
      for (const box of row.querySelectorAll<HTMLElement>('[data-stat-value]')) {
        const text = box.firstElementChild;
        if (!(text instanceof HTMLElement)) continue;
        const current = parseFloat(getComputedStyle(box).fontSize) || MAX_PX;
        const natural = text.getBoundingClientRect().width;
        // Text width scales with the font size: the size at which this value just fits.
        if (natural > 0 && box.clientWidth > 0) {
          size = Math.min(size, (box.clientWidth * current) / natural);
        }
      }
      // Half-pixel steps, so a sub-pixel resize does not re-render the row.
      setFit(Math.max(MIN_PX, Math.min(MAX_PX, Math.floor(size * 2) / 2)));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(row);
    void document.fonts?.ready.then(measure);
    return () => observer.disconnect();
  }, [stats]);

  return (
    <div
      ref={rowRef}
      className={cn('mb-3.5 grid gap-2.5', stats.length === 2 ? 'grid-cols-2' : 'grid-cols-3')}
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
            data-stat-value
            className="mt-auto block overflow-hidden text-ellipsis whitespace-nowrap text-m-stat text-ink num"
            style={{ fontSize: fit === null ? CONTAINER_SIZE : `min(${fit}px, ${CONTAINER_SIZE})` }}
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
