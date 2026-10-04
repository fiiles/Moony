import { cn } from '@/lib/utils';

/**
 * Skeleton (design system §6): shaped like the content it replaces; no page
 * spinners. The shimmer itself is the `.skeleton` utility in index.css.
 */
function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div aria-hidden className={cn('skeleton', className)} {...props} />;
}

export { Skeleton };
