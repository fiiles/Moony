import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

type Tone = 'neutral' | 'gain' | 'loss';

interface StatProps {
  label: ReactNode;
  value: ReactNode;
  /** 10/500 line under the value ("+ 76 200 Kč tento měsíc"). */
  note?: ReactNode;
  /** Colors the value (and the note when `noteTone` is unset) — only for a change. */
  tone?: Tone;
  noteTone?: Tone;
  /** Small element at the right end of the label (series dot, info glyph). */
  aside?: ReactNode;
  className?: string;
}

const toneText: Record<Tone, string> = {
  neutral: '',
  gain: 'text-gain',
  loss: 'text-loss',
};

/** Stat card (design system §6, `.stat`, E1): label 11, value 22, note 10. */
export function Stat({
  label,
  value,
  note,
  tone = 'neutral',
  noteTone,
  aside,
  className,
}: StatProps) {
  return (
    <div
      className={cn(
        'rounded-r3 border border-line bg-stat-grad px-[19px] py-[18px] shadow-e1',
        className
      )}
    >
      <span className="flex items-center justify-between gap-2 text-caption font-600 text-ink-3">
        <span>{label}</span>
        {aside}
      </span>
      <strong className={cn('mt-[10px] block text-stat text-ink num', toneText[tone])}>
        {value}
      </strong>
      {note && (
        <span
          className={cn(
            'mt-1.5 block text-micro font-500 text-ink-4',
            toneText[noteTone ?? (tone === 'neutral' ? 'neutral' : tone)]
          )}
        >
          {note}
        </span>
      )}
    </div>
  );
}

/** Three or four stats in a row (`.stats`, `.stats--3`), never more. */
export function Stats({
  columns = 4,
  className,
  children,
}: {
  columns?: 3 | 4;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        'mb-[17px] grid gap-[14px]',
        columns === 3 ? 'grid-cols-3' : 'grid-cols-4',
        className
      )}
    >
      {children}
    </div>
  );
}

/** Skeleton in the shape of a stat card. */
export function StatSkeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn(
        'rounded-r3 border border-line bg-stat-grad px-[19px] py-[18px] shadow-e1',
        className
      )}
    >
      <div className="skeleton h-3 w-2/5" />
      <div className="skeleton mt-3 h-6 w-3/5" />
      <div className="skeleton mt-2.5 h-2.5 w-1/2" />
    </div>
  );
}

/** Small series dot used next to stat labels and in legends. */
export function SeriesDot({ className, color }: { className?: string; color?: string }) {
  return (
    <i
      aria-hidden
      className={cn('inline-block size-1.5 rounded-full bg-s1', className)}
      style={color ? { background: color } : undefined}
    />
  );
}
