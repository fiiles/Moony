import * as React from 'react';
import * as ProgressPrimitive from '@radix-ui/react-progress';

import { cn } from '@/lib/utils';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface ProgressProps extends React.ComponentPropsWithoutRef<typeof ProgressPrimitive.Root> {
  /** Over the limit: the bar turns red-brown (`.progress--over`). */
  over?: boolean;
  /** Position of an optional mark on the track, 0–100 (e.g. "today" in a month). */
  mark?: number;
  /** Tooltip of the mark ("Dnes · uplynulo 83 % období"). */
  markLabel?: string;
}

/**
 * Progress (design system §6, `.progress`): 6 px track in `well-3`, bar in s1,
 * `loss` when over the limit. Pass `className="h-2.5"` for the overview size.
 */
const Progress = React.forwardRef<React.ComponentRef<typeof ProgressPrimitive.Root>, ProgressProps>(
  ({ className, value, over = false, mark, markLabel, ...props }, ref) => {
    const pct = Math.max(0, Math.min(100, value ?? 0));
    const markSpan = mark !== undefined && (
      <span
        aria-hidden
        className={cn(
          'absolute -top-[3px] h-3 w-0.5 rounded-px bg-paper shadow-[0_0_0_1px_var(--ink-3)]',
          // A wider hover target than the 2 px mark itself.
          markLabel && "before:absolute before:-inset-x-[6px] before:-inset-y-1 before:content-['']"
        )}
        style={{ left: `${Math.max(0, Math.min(100, mark))}%` }}
      />
    );
    const markNode =
      markSpan && markLabel ? (
        <Tooltip>
          <TooltipTrigger asChild>{markSpan}</TooltipTrigger>
          <TooltipContent>{markLabel}</TooltipContent>
        </Tooltip>
      ) : (
        markSpan
      );
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
        {markNode}
      </ProgressPrimitive.Root>
    );
  }
);
Progress.displayName = ProgressPrimitive.Root.displayName;

export { Progress };
