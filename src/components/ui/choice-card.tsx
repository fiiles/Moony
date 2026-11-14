import * as React from 'react';

import { cn } from '@/lib/utils';
import { RadioGroupItem } from '@/components/ui/radio-group';

interface ChoiceCardProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'> {
  /** The radio's value inside the surrounding <RadioGroup>. */
  value: string;
  /** Used for the radio and its label; unique on the page. */
  id: string;
  title: React.ReactNode;
  /** A <Badge> beside the title, pushed to the end of its line. */
  badge?: React.ReactNode;
  description?: React.ReactNode;
  selected?: boolean;
  disabled?: boolean;
  /** Content under the description, shown inside the card (a field, an alert). */
  children?: React.ReactNode;
}

/**
 * Choice card (design system §6, `.choice`): one option of a <RadioGroup> as a bordered card with
 * a radio dot, a title (and badge), a description and, when selected, the content that belongs to
 * it. The selected card has the focus border and ring; a disabled one is dimmed.
 */
const ChoiceCard = React.forwardRef<HTMLDivElement, ChoiceCardProps>(
  (
    { value, id, title, badge, description, selected, disabled, className, children, ...props },
    ref
  ) => (
    <div
      ref={ref}
      data-state={selected ? 'selected' : 'idle'}
      className={cn(
        'flex gap-3 rounded-[11px] border border-line-strong bg-paper px-[15px] py-[14px] data-[state=selected]:border-focus data-[state=selected]:shadow-focus',
        className
      )}
      {...props}
    >
      <RadioGroupItem value={value} id={id} disabled={disabled} className="mt-px shrink-0" />
      <div className="min-w-0 flex-1">
        <div className={cn(disabled && 'opacity-60')}>
          <label
            htmlFor={id}
            className={cn(
              'flex items-center justify-between gap-2 text-body font-650 text-ink',
              disabled ? 'cursor-not-allowed' : 'cursor-pointer'
            )}
          >
            <span>{title}</span>
            {badge}
          </label>
          {description && <p className="m-0 mt-[3px] text-table text-ink-3">{description}</p>}
        </div>
        {children}
      </div>
    </div>
  )
);
ChoiceCard.displayName = 'ChoiceCard';

export { ChoiceCard };
