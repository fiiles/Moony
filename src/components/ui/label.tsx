import * as React from 'react';
import * as LabelPrimitive from '@radix-ui/react-label';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Field label (design system §6, `.field__label`): above the field, 11/700 in
 * ink-2. An optional marker goes inline: `<Label>Instituce <LabelHint>nepovinné</LabelHint></Label>`.
 */
const labelVariants = cva(
  'flex items-baseline justify-between gap-2 text-caption font-700 leading-none text-ink-2 peer-disabled:cursor-not-allowed peer-disabled:opacity-70'
);

const Label = React.forwardRef<
  React.ComponentRef<typeof LabelPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof LabelPrimitive.Root> & VariantProps<typeof labelVariants>
>(({ className, ...props }, ref) => (
  <LabelPrimitive.Root ref={ref} className={cn(labelVariants(), className)} {...props} />
));
Label.displayName = LabelPrimitive.Root.displayName;

/** The faint "nepovinné" marker at the right end of a label. */
const LabelHint = ({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) => (
  <span className={cn('font-500 text-ink-5', className)} {...props} />
);
LabelHint.displayName = 'LabelHint';

export { Label, LabelHint };
