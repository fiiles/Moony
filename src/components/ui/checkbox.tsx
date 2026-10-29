import * as React from 'react';
import * as CheckboxPrimitive from '@radix-ui/react-checkbox';
import { Check, Minus } from 'lucide-react';

import { cn } from '@/lib/utils';

/** Checkbox (design system §6, `.check`): 16 px, radius 4, dark material when checked. */
const Checkbox = React.forwardRef<
  React.ComponentRef<typeof CheckboxPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>
>(({ className, ...props }, ref) => (
  <CheckboxPrimitive.Root
    ref={ref}
    className={cn(
      'peer grid size-4 shrink-0 place-items-center rounded-[4px] border border-line-strong bg-paper transition-colors duration-fast focus-visible:outline-none focus-visible:border-focus focus-visible:shadow-focus disabled:cursor-not-allowed disabled:opacity-45 data-[state=checked]:border-dark data-[state=checked]:bg-dark data-[state=checked]:text-ink-inverse data-[state=indeterminate]:border-dark data-[state=indeterminate]:bg-dark data-[state=indeterminate]:text-ink-inverse',
      className
    )}
    {...props}
  >
    <CheckboxPrimitive.Indicator className="group flex items-center justify-center text-current">
      {/* The indicator carries the state: a dash for "some selected", a check otherwise */}
      <Check className="size-3 group-data-[state=indeterminate]:hidden" strokeWidth={2.5} />
      <Minus className="hidden size-3 group-data-[state=indeterminate]:block" strokeWidth={2.5} />
    </CheckboxPrimitive.Indicator>
  </CheckboxPrimitive.Root>
));
Checkbox.displayName = CheckboxPrimitive.Root.displayName;

export { Checkbox };
