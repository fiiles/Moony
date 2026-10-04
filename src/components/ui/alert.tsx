import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Inline alert (design system §6, `.alert`): next to its cause, says what
 * happened and offers an action (put a small <Button> in <AlertActions>).
 * `destructive` is the error variant on `loss-soft`.
 */
const alertVariants = cva(
  'relative w-full rounded-[10px] border px-[14px] py-3 text-table text-ink-2 [&>svg]:absolute [&>svg]:left-[14px] [&>svg]:top-[13px] [&>svg]:size-[15px] [&>svg~*]:pl-[25px]',
  {
    variants: {
      variant: {
        default: 'border-line bg-well [&>svg]:text-ink-3',
        destructive: 'border-loss-line bg-loss-soft [&>svg]:text-loss',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

const Alert = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof alertVariants>
>(({ className, variant, ...props }, ref) => (
  <div ref={ref} role="alert" className={cn(alertVariants({ variant }), className)} {...props} />
));
Alert.displayName = 'Alert';

const AlertTitle = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h5 ref={ref} className={cn('mb-0.5 font-650 leading-[1.4] text-ink', className)} {...props} />
  )
);
AlertTitle.displayName = 'AlertTitle';

const AlertDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn('text-table leading-[1.5] [&_p]:leading-[1.5]', className)}
    {...props}
  />
));
AlertDescription.displayName = 'AlertDescription';

/** Actions sit at the right end of the alert ("Zkusit znovu"). */
const AlertActions = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn('mt-2 flex gap-2 pl-[25px]', className)} {...props} />
);
AlertActions.displayName = 'AlertActions';

export { Alert, AlertTitle, AlertDescription, AlertActions };
