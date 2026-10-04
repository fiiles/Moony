import type { ReactNode } from 'react';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

type Tone = 'neutral' | 'gain' | 'loss';

interface HeroValueProps {
  label: ReactNode;
  value: ReactNode;
  /** Signed change with its context: "↗ + 2,8 %" and "+ 77 500 Kč za posledních 30 dní". */
  delta?: ReactNode;
  deltaNote?: ReactNode;
  tone?: Tone;
  /** Secondary number next to the main one (current price), 22 px. */
  compact?: boolean;
  className?: string;
}

const toneText: Record<Tone, string> = { neutral: '', gain: 'text-gain', loss: 'text-loss' };

/** The dominant number of a hero: label 12/600, value 42/580 (22 when compact), delta 12/650. */
export function HeroValue({
  label,
  value,
  delta,
  deltaNote,
  tone = 'neutral',
  compact = false,
  className,
}: HeroValueProps) {
  return (
    <div className={className}>
      <div className="text-table font-600 text-ink-3">{label}</div>
      <span
        className={cn(
          'block text-ink num',
          compact
            ? 'my-2 text-[22px] font-580 leading-none tracking-[-0.05em]'
            : 'mb-2 mt-[7px] text-display'
        )}
      >
        {value}
      </span>
      {(delta || deltaNote) && (
        <div className={cn('text-table font-650', toneText[tone], compact && 'text-caption')}>
          {delta}
          {deltaNote && <span className="ml-[5px] font-450 text-ink-4">{deltaNote}</span>}
        </div>
      )}
    </div>
  );
}

interface HeroCardProps {
  /** Left: one or more <HeroValue>; several values are separated by a hairline. */
  children: ReactNode;
  /** Right of the head: period segment or an insight sentence. */
  aside?: ReactNode;
  /** The chart (and legend) under the head. */
  chart?: ReactNode;
  className?: string;
}

/**
 * Hero card (design system §6, `.hero`, E2): one dominant number, its change
 * with sign and context, a period switch top right, the chart below. The
 * decorative circle top right is part of the material.
 */
export function HeroCard({ children, aside, chart, className }: HeroCardProps) {
  return (
    <Card
      className={cn(
        'relative overflow-hidden px-6 pb-4 pt-6 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-[""]',
        className
      )}
    >
      <div className="relative z-[1] flex items-start justify-between gap-6">
        <div className="flex items-start gap-7 [&>*+*]:border-l [&>*+*]:border-line-soft [&>*+*]:pl-7">
          {children}
        </div>
        {aside}
      </div>
      {chart && <div className="relative z-[1]">{chart}</div>}
    </Card>
  );
}
