import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Badges (design system §6, `.badge`): 20 px, no border. Green only for a
 * positive state, red-brown only for a negative one; dark for a type such as
 * "Nákup", dashed outline for "Prodej" or "Nezařazeno".
 */
const badgeVariants = cva(
  'inline-flex h-5 items-center gap-[5px] whitespace-nowrap rounded-r1 px-[7px] text-micro font-650 [&_svg]:size-[11px] [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default: 'bg-well-2 text-ink-2',
        secondary: 'bg-well-2 text-ink-2',
        gain: 'bg-gain-soft text-gain',
        loss: 'bg-loss-soft text-loss',
        dark: 'bg-dark text-ink-inverse',
        outline: 'border border-dashed border-line-strong bg-transparent text-ink-3',
        /** Legacy aliases kept until the last page is restyled. */
        destructive: 'bg-loss-soft text-loss',
        warning: 'bg-well-2 text-ink',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
