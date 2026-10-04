import * as React from 'react';

import { cn } from '@/lib/utils';

/**
 * Text input (design system §6, `.input`): 39 px, radius 8, paper surface,
 * focus = focus-border + 3 px ring, invalid (aria-invalid from FormControl)
 * = loss border. Units sit inside the field on the right via <InputWrap>.
 */
const inputClassName =
  'flex h-input w-full rounded-r2 border border-line-strong bg-paper px-[11px] text-body text-ink transition-[border-color,box-shadow] duration-fast file:border-0 file:bg-transparent file:text-table file:font-600 file:text-ink placeholder:text-ink-5 hover:border-line-hover focus:border-focus focus:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:bg-well disabled:text-ink-4 aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring';

const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<'input'>>(
  ({ className, type, ...props }, ref) => {
    return <input type={type} className={cn(inputClassName, className)} ref={ref} {...props} />;
  }
);
Input.displayName = 'Input';

interface InputWrapProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Unit shown inside the field on the right ("Kč", "ks", "USD", "%"). */
  unit?: React.ReactNode;
  /** Leading icon (search fields). */
  icon?: React.ReactNode;
}

/** `.input-wrap`: positions a unit on the right or an icon on the left of an <Input>. */
const InputWrap = React.forwardRef<HTMLDivElement, InputWrapProps>(
  ({ className, unit, icon, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        'relative',
        icon && '[&>input]:pl-[33px]',
        unit && '[&>input]:pr-[42px]',
        className
      )}
      {...props}
    >
      {icon && (
        <span className="pointer-events-none absolute left-[11px] top-1/2 -translate-y-1/2 text-ink-4 [&_svg]:size-3.5">
          {icon}
        </span>
      )}
      {children}
      {unit && (
        <span className="pointer-events-none absolute right-[11px] top-1/2 -translate-y-1/2 text-caption font-600 text-ink-4">
          {unit}
        </span>
      )}
    </div>
  )
);
InputWrap.displayName = 'InputWrap';

export { Input, InputWrap, inputClassName };
