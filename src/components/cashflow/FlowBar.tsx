import { useState } from 'react';
import { ChartTip, FloatingTip } from '@/components/charts/ChartTip';
import { cn } from '@/lib/utils';
import { FLOW_REST, FLOW_SERIES } from '@/components/cashflow/flow-colors';

export interface FlowSegment {
  key: string;
  label: string;
  value: number;
  /** Share of the whole bar, 0–1. */
  share: number;
  /** `rest` draws the hatched remainder ("Zbývá k investování"). */
  kind?: 'rest';
}

/**
 * 100 % flow bar (design system §11, replaces the Sankey): expense groups in
 * s1…s4 + Ostatní, the hatched remainder is what stays free. Hovering a
 * segment dims the others and shows its amount and share.
 */
export function FlowBar({
  segments,
  formatValue,
  formatShare,
  className,
}: {
  segments: FlowSegment[];
  formatValue: (value: number) => string;
  formatShare: (share: number) => string;
  className?: string;
}) {
  const [hover, setHover] = useState<{ index: number; x: number } | null>(null);
  const [width, setWidth] = useState(0);
  let colorIndex = 0;

  return (
    <div className={cn('relative', className)}>
      <div
        className="group/flow flex h-[34px] overflow-hidden rounded-r2 bg-well-2"
        ref={(el) => {
          if (el && el.clientWidth !== width) setWidth(el.clientWidth);
        }}
        onMouseLeave={() => setHover(null)}
      >
        {segments.map((s, i) => {
          const color =
            s.kind === 'rest'
              ? FLOW_REST
              : FLOW_SERIES[Math.min(colorIndex++, FLOW_SERIES.length - 1)];
          return (
            <i
              key={s.key}
              className={cn(
                'block h-full transition-opacity duration-fast [box-shadow:inset_-2px_0_0_var(--paper)] last:[box-shadow:none]',
                hover !== null && hover.index !== i && 'opacity-55'
              )}
              style={{ width: `${Math.max(0, s.share) * 100}%`, background: color }}
              onMouseEnter={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                const parent = e.currentTarget.parentElement?.getBoundingClientRect();
                setHover({ index: i, x: rect.left - (parent?.left ?? 0) + rect.width / 2 });
              }}
              aria-label={`${s.label} ${formatValue(s.value)} · ${formatShare(s.share)}`}
              role="img"
            />
          );
        })}
      </div>
      {hover !== null && segments[hover.index] && (
        <FloatingTip x={hover.x} y={-2} width={width}>
          <ChartTip
            title={segments[hover.index].label}
            lines={[
              `${formatValue(segments[hover.index].value)} · ${formatShare(segments[hover.index].share)}`,
            ]}
          />
        </FloatingTip>
      )}
    </div>
  );
}
