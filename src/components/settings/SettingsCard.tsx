import { forwardRef, type ReactNode } from 'react';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

interface SettingsCardProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'title'> {
  title: ReactNode;
  description?: ReactNode;
  /** Small control at the right of the head (an "Add" button, a collapse toggle). */
  action?: ReactNode;
  /** The danger zone: reddish hairline instead of the normal one. */
  danger?: boolean;
  children: ReactNode;
}

/**
 * Settings card (design system, prototype `settings.html`, `.settings-card`):
 * a flat E1 card with a 16 px title, the sub-line under it and stacked rows.
 */
export const SettingsCard = forwardRef<HTMLDivElement, SettingsCardProps>(function SettingsCard(
  { title, description, action, danger = false, className, children, ...props },
  ref
) {
  return (
    <Card
      ref={ref}
      variant="flat"
      className={cn('mb-[18px] last:mb-0', danger && 'border-loss-line', className)}
      {...props}
    >
      <div className="flex items-start justify-between gap-4 px-[21px] pb-1.5 pt-5">
        <div className="min-w-0">
          <h2 className="m-0 text-[16px] font-650 leading-[1.25] tracking-[-0.035em] text-ink">
            {title}
          </h2>
          {description && <p className="mt-[5px] text-micro font-500 text-ink-4">{description}</p>}
        </div>
        {action && <div className="flex shrink-0 items-center gap-2">{action}</div>}
      </div>
      <div className="px-[21px] pb-5 pt-[10px]">{children}</div>
    </Card>
  );
});

interface SettingsRowProps {
  label: ReactNode;
  /** One sentence under the label, 11 px in ink-4. */
  hint?: ReactNode;
  /** Renders the label as a <label> for the control with this id. */
  htmlFor?: string;
  /** The control(s) on the right. */
  children?: ReactNode;
  className?: string;
}

/** `.row`: label and hint left, control right, hairline between rows. */
export function SettingsRow({ label, hint, htmlFor, children, className }: SettingsRowProps) {
  const labelClass = 'block text-body font-600 text-ink';
  return (
    <div
      className={cn(
        'flex items-center justify-between gap-6 border-b border-line-soft py-3 last:border-b-0',
        className
      )}
    >
      <div className="min-w-0">
        {htmlFor ? (
          <label htmlFor={htmlFor} className={labelClass}>
            {label}
          </label>
        ) : (
          <b className={labelClass}>{label}</b>
        )}
        {hint && <small className="mt-0.5 block text-caption text-ink-4">{hint}</small>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}
