import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface RowBadgeProps {
  children: ReactNode;
  /**
   * The line it sits in: after the title (6 pt from it, raised 2 px: Akcie's "ručně", Dluhopisy'
   * "do roka", an account or a property "mimo součet") or among the sub-line's parts (no margin,
   * raised 1 px: the ledger's "Bez kategorie", "Čekající", "Vratka").
   */
  line?: 'title' | 'sub';
  /** `loss` for the import preview's "Chyba". */
  tone?: 'loss';
  /** Merged last: `flex-none` where the title is a flex row that truncates the name instead. */
  className?: string;
}

/**
 * A short word in an entity row (README §13, `.m-row__badge`): 18 pt, 11 / 600 `ink-3` on
 * `well-2`, radius 4. It sits inside its line, which still ends with an ellipsis. Its text line is
 * the badge's full 18 pt (not `leading-none`), so a caron or an accent above a capital ("Č") stays
 * inside the line, whose `truncate` box clips at the badge's edges.
 */
export function RowBadge({ children, line = 'title', tone, className }: RowBadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex min-h-[18px] items-center rounded-r1 px-1.5 py-px text-m-caption font-semibold leading-[1.3]',
        line === 'title' ? 'ml-1.5 align-[2px]' : 'align-[1px]',
        tone === 'loss' ? 'bg-loss-soft text-loss' : 'bg-well-2 text-ink-3',
        className
      )}
    >
      {children}
    </span>
  );
}
