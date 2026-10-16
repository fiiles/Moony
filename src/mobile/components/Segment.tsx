import type { ReactNode } from 'react';
import * as ToggleGroupPrimitive from '@radix-ui/react-toggle-group';
import { cn } from '@/lib/utils';

interface SegmentProps<T extends string> {
  value: T;
  options: readonly { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
  /** Accessible name of the group ("Období"). */
  label: string;
  /** Spacing around the control (`mt-3 mb-2` between a hero's number and its chart). */
  className?: string;
}

/**
 * Segmented control (spec §10.3, README §13, `.m-seg`): 36 pt tall as on the desktop, the active
 * option raised on paper. It scrolls sideways when the options do not fit (a long German period
 * label) instead of wrapping. Radix ToggleGroup underneath: arrow keys move, the active option
 * cannot be switched off.
 */
export function Segment<T extends string>({
  value,
  options,
  onChange,
  label,
  className,
}: SegmentProps<T>) {
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      value={value}
      onValueChange={(next) => {
        if (next) onChange(next as T);
      }}
      aria-label={label}
      className={cn(
        'flex h-9 max-w-full gap-px overflow-x-auto rounded-r2 bg-well-2 p-[3px] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden',
        className
      )}
    >
      {options.map((option) => (
        <ToggleGroupPrimitive.Item
          key={option.value}
          value={option.value}
          className="h-[30px] flex-[1_0_auto] whitespace-nowrap rounded-r1 px-1.5 text-m-caption font-semibold leading-none text-ink-3 transition-colors duration-fast data-[state=on]:bg-paper data-[state=on]:text-ink data-[state=on]:shadow-seg focus-visible:outline-none focus-visible:shadow-focus"
        >
          {option.label}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}
