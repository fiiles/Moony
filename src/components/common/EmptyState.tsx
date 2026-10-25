import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

interface EmptyStateProps {
  icon: React.ReactNode;
  title: string;
  description: string;
  action?: React.ReactNode;
  /** Render without the surrounding E3 card (inside an existing card). */
  bare?: boolean;
  className?: string;
  /** Extra classes for the title; the phone passes a `text-m-*` role (default: none). */
  titleClassName?: string;
  /** Extra classes for the sentence under it; the phone passes a `text-m-*` role (default: none). */
  descriptionClassName?: string;
}

/**
 * Empty state (design system §6, `.empty`): an invitation, not an apology.
 * Icon in ink-5, title, one sentence that says what to do, one button.
 */
export function EmptyState({
  icon,
  title,
  description,
  action,
  bare = false,
  className,
  titleClassName,
  descriptionClassName,
}: EmptyStateProps) {
  const body = (
    <div
      className={cn(
        'grid place-items-center px-6 py-12 text-center text-ink-3 [&>svg]:mb-3 [&>svg]:size-7 [&>svg]:text-ink-5',
        className
      )}
    >
      {icon}
      <h3 className={cn('mb-1.5 text-h3 text-ink', titleClassName)}>{title}</h3>
      <p className={cn('m-0 max-w-[320px] text-table leading-[1.5]', descriptionClassName)}>
        {description}
      </p>
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
  return bare ? body : <Card variant="table">{body}</Card>;
}
