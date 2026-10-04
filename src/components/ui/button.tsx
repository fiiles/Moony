import * as React from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Buttons (design system §6, `.btn`): 36 px (32 small), radius 8, 11/650 label
 * that names the outcome. One primary per page. `danger` is text-only; the solid
 * `destructive` belongs inside confirm dialogs only. Loading spins the icon and
 * keeps the label.
 */
const buttonVariants = cva(
  'inline-flex items-center justify-center gap-[7px] whitespace-nowrap rounded-r2 border text-caption font-650 transition-[background,box-shadow,color,border-color] duration-fast focus-visible:outline-none focus-visible:border-focus focus-visible:shadow-focus disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:size-3.5 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        /** Primary: dark material. */
        default:
          'border-transparent bg-dark-grad text-ink-inverse shadow-dark hover:bg-dark-grad-hover active:bg-none active:bg-dark-lo [&_svg]:text-ink-inverse',
        /** Secondary: paper with a line and a 1 px shadow (the design's plain `.btn`). */
        outline:
          'border-line-strong bg-btn-grad text-ink-2 shadow-btn hover:bg-none hover:bg-paper hover:text-ink hover:shadow-btn-hover active:bg-well active:shadow-none [&_svg]:text-ink-3 [&:hover_svg]:text-ink-2',
        secondary:
          'border-line-strong bg-btn-grad text-ink-2 shadow-btn hover:bg-none hover:bg-paper hover:text-ink hover:shadow-btn-hover active:bg-well active:shadow-none [&_svg]:text-ink-3 [&:hover_svg]:text-ink-2',
        /** Text button for cancel and link-like actions. */
        ghost:
          'border-transparent text-ink-2 hover:bg-ink-hover hover:text-ink [&_svg]:text-ink-3 [&:hover_svg]:text-ink-2',
        /** Text-only red-brown: delete actions outside confirm dialogs. */
        danger: 'border-transparent text-loss hover:bg-loss-soft [&_svg]:text-loss',
        /** Solid red-brown: the confirming action of a destructive dialog. */
        destructive:
          'border-transparent bg-loss text-ink-inverse shadow-destructive hover:bg-loss-hover [&_svg]:text-ink-inverse',
        link: 'h-auto border-transparent px-0 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline',
      },
      size: {
        default: 'h-control px-[13px]',
        sm: 'h-control-sm px-[11px] text-micro [&_svg]:size-[13px]',
        lg: 'h-10 px-4 text-table',
        icon: 'size-control p-0 [&_svg]:size-4',
        'icon-sm': 'size-control-sm p-0 [&_svg]:size-[13px]',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  /** Spins the leading icon and blocks clicks; the label stays readable. */
  loading?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, loading = false, disabled, ...props }, ref) => {
    const Comp = asChild ? Slot : 'button';
    return (
      <Comp
        className={cn(
          buttonVariants({ variant, size, className }),
          loading && 'pointer-events-none [&_svg]:animate-spin'
        )}
        ref={ref}
        aria-busy={loading || undefined}
        disabled={disabled}
        {...props}
      />
    );
  }
);
Button.displayName = 'Button';

export { Button, buttonVariants };
