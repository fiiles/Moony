import * as React from 'react';
import * as ToggleGroupPrimitive from '@radix-ui/react-toggle-group';

import { cn } from '@/lib/utils';

/**
 * Segmented control (design system §6, `.seg`): the period switch
 * (1M · 3M · 1R · Vše) and any 2–4-value filter. One value is always
 * selected; clicking the active segment keeps it. Built on Radix ToggleGroup
 * for roving keyboard focus. `size="lg"` is the page-head variant (30 px).
 */
export interface SegmentedOption<T extends string> {
  value: T;
  label: React.ReactNode;
  disabled?: boolean;
}

export interface SegmentedProps<T extends string> extends Omit<
  React.ComponentPropsWithoutRef<typeof ToggleGroupPrimitive.Root>,
  'type' | 'value' | 'onValueChange' | 'defaultValue'
> {
  value: T;
  onValueChange: (value: T) => void;
  options: readonly SegmentedOption<T>[];
  size?: 'sm' | 'lg';
  /** Segments share the width equally (type switches inside forms). */
  fullWidth?: boolean;
}

function SegmentedInner<T extends string>(
  {
    value,
    onValueChange,
    options,
    size = 'sm',
    fullWidth = false,
    className,
    ...props
  }: SegmentedProps<T>,
  ref: React.ForwardedRef<HTMLDivElement>
) {
  return (
    <ToggleGroupPrimitive.Root
      ref={ref}
      type="single"
      value={value}
      onValueChange={(next) => {
        if (next) onValueChange(next as T);
      }}
      className={cn(
        'inline-flex items-center gap-px rounded-r2 bg-well-2 p-[3px]',
        fullWidth && 'flex w-full',
        className
      )}
      {...props}
    >
      {options.map((option) => (
        <ToggleGroupPrimitive.Item
          key={option.value}
          value={option.value}
          disabled={option.disabled}
          className={cn(
            'inline-flex items-center justify-center whitespace-nowrap rounded-r1 font-600 text-ink-3 transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none disabled:opacity-45 data-[state=on]:bg-paper data-[state=on]:text-ink data-[state=on]:shadow-seg',
            size === 'lg' ? 'h-[30px] px-3 text-caption' : 'h-[26px] px-[10px] text-micro',
            fullWidth && 'flex-1'
          )}
        >
          {option.label}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}

const Segmented = React.forwardRef(SegmentedInner) as <T extends string>(
  props: SegmentedProps<T> & { ref?: React.ForwardedRef<HTMLDivElement> }
) => React.ReactElement;

export { Segmented };
