import { forwardRef, type ButtonHTMLAttributes, type HTMLAttributes } from 'react';
import { useCategoryName } from '@/hooks/use-categories';
import { cn } from '@/lib/utils';
import type { NamedCategory } from '@/utils/category-name';
import { CategoryIcon } from './CategoryIcon';

/**
 * The pill shape shared by the ledger's plan chip and the category pill: 26 px high, fully
 * rounded, a hairline border on paper, micro 650 text. The border color is the caller's.
 */
export const PILL =
  'inline-flex h-[26px] max-w-full items-center gap-1.5 rounded-[13px] border bg-paper px-[9px] text-micro font-650';

/** What a pill that opens something adds: hover and keyboard focus. */
export const PILL_INTERACTIVE =
  'transition-[color,border-color,opacity] duration-fast hover:border-line-hover hover:text-ink focus-visible:outline-none focus-visible:border-focus focus-visible:shadow-focus';

type PillCategory = NamedCategory & { icon?: string | null };

function PillContent({ category }: { category: PillCategory }) {
  const categoryName = useCategoryName();
  return (
    <>
      <CategoryIcon iconName={category.icon || 'tag'} className="size-3 shrink-0" />
      <span className="truncate">{categoryName(category)}</span>
    </>
  );
}

/**
 * A category as a read-only pill: the same pill as the ledger's plan chip, with the category's
 * own icon in ink (no category color, spec O15) and its translated name, truncated.
 */
export function CategoryPill({
  category,
  className,
  ...props
}: { category: PillCategory } & HTMLAttributes<HTMLSpanElement>) {
  return (
    <span className={cn(PILL, 'border-line-strong text-ink-2', className)} {...props}>
      <PillContent category={category} />
    </span>
  );
}

/** The category pill as a button, e.g. the trigger of the ledger's category picker. */
export const CategoryPillButton = forwardRef<
  HTMLButtonElement,
  { category: PillCategory } & ButtonHTMLAttributes<HTMLButtonElement>
>(({ category, className, type = 'button', ...props }, ref) => (
  <button
    ref={ref}
    type={type}
    className={cn(
      PILL,
      PILL_INTERACTIVE,
      'border-line-strong text-ink-2 disabled:cursor-not-allowed disabled:opacity-45',
      className
    )}
    {...props}
  >
    <PillContent category={category} />
  </button>
));
CategoryPillButton.displayName = 'CategoryPillButton';
