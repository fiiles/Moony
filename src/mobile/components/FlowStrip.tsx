import { useLayoutEffect, useRef } from 'react';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { monthLabelDate, type MonthFlow } from '@/utils/bank-activity';

/** Height of the bar area (spec §10.4: bars 110). */
const BARS = 110;

interface FlowStripProps {
  /** The months in order, oldest first (`monthlyFlows`); amounts in the account currency. */
  flows: readonly MonthFlow[];
  /** The month the caption describes; the others are muted. `null`: none muted. */
  selected: number | null;
  onSelect: (month: number) => void;
  /** Amounts in each month's accessible name. */
  formatValue: (value: number) => string;
  labels: { income: string; expense: string };
  /** Accessible name of the strip ("Toky"). */
  label: string;
}

/**
 * The flows strip of the account detail (spec §8.4 "Toky", README §13 `.m-flows`): one 44 pt wide
 * button per month with its incoming (`s1`) and outgoing (`s3`) bars side by side over the month's
 * short name, scaled to the strip's largest month; January carries the year. The strip scrolls
 * sideways (the page never does) and opens scrolled to its end, the newest month at the right. A
 * tap selects the month; the others fade to 45 %.
 */
export function FlowStrip({
  flows,
  selected,
  onSelect,
  formatValue,
  labels,
  label,
}: FlowStripProps) {
  const fmt = useFormat();
  const stripRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const strip = stripRef.current;
    if (strip) strip.scrollLeft = strip.scrollWidth;
  }, []);

  const max = Math.max(0, ...flows.map((f) => Math.max(f.income, f.expense)));
  const height = (value: number) =>
    value > 0 && max > 0 ? Math.max(2, Math.round((value / max) * BARS)) : 0;

  return (
    <div
      ref={stripRef}
      role="group"
      aria-label={label}
      className="flex overflow-x-auto overscroll-x-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
    >
      {flows.map((flow) => {
        const date = monthLabelDate(flow.month);
        const january = date.getUTCMonth() === 0;
        const name = fmt.month(date, { month: 'long', year: 'numeric' });
        return (
          <button
            key={flow.month}
            type="button"
            aria-pressed={flow.month === selected}
            aria-label={`${name}: ${labels.income} ${formatValue(flow.income)}, ${labels.expense} ${formatValue(flow.expense)}`}
            onClick={() => onSelect(flow.month)}
            className={cn(
              'flex min-h-11 w-11 flex-none flex-col items-center rounded-[10px] pt-1 transition-opacity duration-fast [-webkit-touch-callout:none] focus-visible:outline-none focus-visible:shadow-focus',
              selected !== null && flow.month !== selected && 'opacity-45'
            )}
          >
            <span aria-hidden className="flex items-end gap-0.5" style={{ height: BARS }}>
              <span
                className="w-2.5 rounded-[2px] bg-[var(--s1)]"
                style={{ height: height(flow.income) }}
              />
              <span
                className="w-2.5 rounded-[2px] bg-[var(--s3)]"
                style={{ height: height(flow.expense) }}
              />
            </span>
            <span aria-hidden className="mt-1 whitespace-nowrap text-micro font-medium text-ink-4">
              {fmt.month(date, january ? { month: 'short', year: '2-digit' } : { month: 'short' })}
            </span>
          </button>
        );
      })}
    </div>
  );
}
