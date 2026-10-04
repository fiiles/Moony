import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Cards (design system §4/§6, `.card`): E2 by default (paper gradient, sh-2),
 * `flat` for E1 stat-like cards, `table` for the E3 workspace (plain paper,
 * sh-3, clipped corners). Depth comes from the surface, not from borders.
 */
const cardVariants = cva('rounded-r4 border border-line text-ink', {
  variants: {
    variant: {
      default: 'bg-paper-grad shadow-e2',
      flat: 'bg-paper-grad shadow-e1',
      table: 'overflow-hidden bg-paper shadow-e3',
    },
  },
  defaultVariants: { variant: 'default' },
});

export interface CardProps
  extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof cardVariants> {}

const Card = React.forwardRef<HTMLDivElement, CardProps>(
  ({ className, variant, ...props }, ref) => (
    <div ref={ref} className={cn(cardVariants({ variant }), className)} {...props} />
  )
);
Card.displayName = 'Card';

/** `.card__head`: title left, tools right; 20 × 21 px padding. */
const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn('flex items-center justify-between gap-4 px-[21px] py-5', className)}
      {...props}
    />
  )
);
CardHeader.displayName = 'CardHeader';

/** `.card__title`: 18 / 650 / −0.045 em. */
const CardTitle = React.forwardRef<HTMLHeadingElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h2 ref={ref} className={cn('m-0 text-h2 text-ink', className)} {...props} />
  )
);
CardTitle.displayName = 'CardTitle';

/** `.card__sub`: 10 / 500 in ink-4, under the title. */
const CardDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p ref={ref} className={cn('mt-[5px] text-micro font-500 text-ink-4', className)} {...props} />
));
CardDescription.displayName = 'CardDescription';

/** `.card__body`: 20 × 21 px; no top padding when it follows a header. */
const CardContent = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('px-[21px] pb-5 pt-0', className)} {...props} />
  )
);
CardContent.displayName = 'CardContent';

const CardFooter = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('flex items-center px-[21px] pb-5 pt-0', className)} {...props} />
  )
);
CardFooter.displayName = 'CardFooter';

export { Card, CardHeader, CardFooter, CardTitle, CardDescription, CardContent, cardVariants };
