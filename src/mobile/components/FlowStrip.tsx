import { useLayoutEffect, useRef, useState } from 'react';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { monthLabelDate, type MonthFlow } from '@/utils/bank-activity';
import { ScaledChartLegend } from '@/mobile/components/Scaled';
import { flowColumnWidth } from '@/mobile/lib/flow-strip';

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
 * The flows strip of the account detail (spec §8.4 "Toky", README §13 `.m-flows`): one button per
 * month with its incoming (`s1`) and outgoing (`s3`) bars side by side over the month's short
 * name, scaled to the strip's largest month; January carries the two-digit year on a second line.
 * The strip scrolls sideways (the page never does) by whole months and opens scrolled to its end,
 * the newest month at the right. The columns share the strip's width out evenly (`flowColumnWidth`,
 * at least 44 pt), so no month is cut at either edge (audit D9). A tap selects the month: its name
 * is marked on the axis and the others fade to 45 %. The legend under the strip names the bars.
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
  const [column, setColumn] = useState<number | null>(null);

  // The column width follows the strip's width (a rotation, a larger text size)
  useLayoutEffect(() => {
    const strip = stripRef.current;
    if (!strip) return;
    const measure = () => setColumn(flowColumnWidth(strip.clientWidth, flows.length));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(strip);
    return () => observer.disconnect();
  }, [flows.length]);
  // Opens on the newest month, once the columns have their width
  useLayoutEffect(() => {
    const strip = stripRef.current;
    if (strip && column !== null) strip.scrollLeft = strip.scrollWidth;
  }, [column]);

  const max = Math.max(0, ...flows.map((f) => Math.max(f.income, f.expense)));
  const height = (value: number) =>
    value > 0 && max > 0 ? Math.max(2, Math.round((value / max) * BARS)) : 0;

  return (
    <>
      <div
        ref={stripRef}
        role="group"
        aria-label={label}
        className="flex snap-x snap-mandatory overflow-x-auto overscroll-x-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {flows.map((flow) => {
          const date = monthLabelDate(flow.month);
          const january = date.getUTCMonth() === 0;
          const name = fmt.month(date, { month: 'long', year: 'numeric' });
          const chosen = flow.month === selected;
          return (
            <button
              key={flow.month}
              type="button"
              aria-pressed={chosen}
              aria-label={`${name}: ${labels.income} ${formatValue(flow.income)}, ${labels.expense} ${formatValue(flow.expense)}`}
              onClick={() => onSelect(flow.month)}
              style={{ width: column ?? 44 }}
              className={cn(
                'flex min-h-11 flex-none snap-start flex-col items-center rounded-[10px] pt-1 transition-opacity duration-fast [-webkit-touch-callout:none] focus-visible:outline-none focus-visible:shadow-focus',
                selected !== null && !chosen && 'opacity-45'
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
              {/* `fmt.month` adds the year unless told not to, and with a year Czech writes the long
                  month ("únor 2026"): the label is the bare short month ("Úno"), January's year a
                  second line ("26"). Neither may outgrow the column. The chosen month's name sits
                  on a dark pill, the axis's mark of what the line under the strip describes. */}
              <span
                aria-hidden
                className={cn(
                  'mt-1 max-w-full truncate rounded-full px-1.5 text-m-note',
                  chosen ? 'bg-dark font-semibold text-ink-inverse' : 'text-ink-4'
                )}
              >
                {fmt.month(date, { month: 'short', year: undefined })}
              </span>
              {january && (
                <span aria-hidden className="max-w-full truncate text-m-note text-ink-4">
                  {fmt.month(date, { month: undefined, year: '2-digit' })}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <ScaledChartLegend
        className="mt-2"
        items={[
          { label: labels.income, swatch: { kind: 'block', color: 'var(--s1)' } },
          { label: labels.expense, swatch: { kind: 'block', color: 'var(--s3)' } },
        ]}
      />
    </>
  );
}
