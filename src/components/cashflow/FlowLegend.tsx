import type { FlowSegment } from '@/components/cashflow/FlowBar';
import { flowColor } from '@/components/cashflow/flow-colors';

/**
 * The legend under a `FlowBar`: three columns of swatch, label and "amount · %", the swatch in
 * the segment's own color or else its series color (the hatch for the remainder).
 */
export function FlowLegend({
  segments,
  formatValue,
  formatShare,
}: {
  segments: FlowSegment[];
  formatValue: (value: number) => string;
  formatShare: (share: number) => string;
}) {
  return (
    <div className="grid grid-cols-3 gap-x-6 gap-y-2">
      {segments.map((s, i) => (
        <div
          key={s.key}
          className="grid grid-cols-[8px_1fr_auto] items-center gap-[10px] border-b border-line-soft py-1.5 text-caption text-ink-2"
        >
          <i
            aria-hidden
            className="size-2 rounded-[2px] border border-line-strong"
            style={{
              background: s.kind === 'rest' ? flowColor(i, s.kind) : (s.color ?? flowColor(i)),
            }}
          />
          <span>{s.label}</span>
          <b className="font-650 text-ink num">
            {formatValue(s.value)} · {formatShare(s.share)}
          </b>
        </div>
      ))}
    </div>
  );
}
