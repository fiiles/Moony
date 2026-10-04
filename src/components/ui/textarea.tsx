import * as React from 'react';

import { cn } from '@/lib/utils';

/** `.textarea`: same material as the input, min 88 px, vertical resize. */
const Textarea = React.forwardRef<HTMLTextAreaElement, React.ComponentProps<'textarea'>>(
  ({ className, ...props }, ref) => {
    return (
      <textarea
        className={cn(
          'flex min-h-[88px] w-full resize-y rounded-r2 border border-line-strong bg-paper px-[11px] py-[10px] text-body text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 hover:border-line-hover focus:border-focus focus:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:bg-well disabled:text-ink-4 aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring',
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Textarea.displayName = 'Textarea';

export { Textarea };
