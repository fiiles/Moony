import * as React from 'react';
import * as ProgressPrimitive from '@radix-ui/react-progress';

import { cn } from '@/lib/utils';

interface ProgressProps extends React.ComponentPropsWithoutRef<typeof ProgressPrimitive.Root> {
  /** Over the limit: the bar turns red-brown (`.progress--over`). */
  over?: boolean;
  /** Position of an optional mark on the track, 0–100 (e.g. "today" in a month). */
  mark?: number;
}

/**
 * Progress (design system §6, `.progress`): 6 px track in `well-3`, bar in s1,
 * `loss` when over the limit. Pass `className="h-2.5"` for the overview size.
 */
const Progress = React.forwardRef<React.ComponentRef<typeof ProgressPrimitive.Root>, ProgressProps>(
  ({ className, value, over = false, mark, ...props }, ref) => {
    const pct = Math.max(0, Math.min(100, value ?? 0));
    return (
      <ProgressPrimitive.Root
        ref={ref}
        value={pct}
        className={cn('relative h-1.5 w-full rounded-[3px] bg-well-3', className)}
        {...props}
      >
        <ProgressPrimitive.Indicator
          className={cn(
            'h-full rounded-[3px] transition-[width] duration-base ease-moony',
            over ? 'bg-loss' : 'bg-s1'
          )}
          style={{ width: `${pct}%` }}
        />
        {mark !== undefined && (
          <span
            aria-hidden
            className="absolute -top-[3px] h-3 w-0.5 rounded-px bg-paper shadow-[0_0_0_1px_var(--ink-3)]"
            style={{ left: `${Math.max(0, Math.min(100, mark))}%` }}
          />
        )}
      </ProgressPrimitive.Root>
    );
  }
);
Progress.displayName = ProgressPrimitive.Root.displayName;

export { Progress };
