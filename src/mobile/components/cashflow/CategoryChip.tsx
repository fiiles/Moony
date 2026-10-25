import type { CSSProperties } from 'react';
import { Tag, type LucideIcon } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { cn } from '@/lib/utils';

interface CategoryChipProps {
  /** The category's icon and hue; without one the chip is neutral. */
  category?: Pick<TransactionCategory, 'icon' | 'color'> | null;
  /** The glyph of a neutral chip without a category (uncategorized rows). */
  fallback?: LucideIcon;
}

/**
 * The category's chip on an entity row (README §13, `.m-row__chip--cat`): the 36 pt round chip
 * with the category's icon (16 pt) in its own hue at 15 % over paper, as the desktop's
 * `CategoryIconChip` (`cat-chip` lifts the glyph in the dark theme). A category without a hue
 * keeps its icon on `well-2`; without a category the `fallback` icon (a tag) stands there.
 */
export function CategoryChip({ category, fallback: Fallback = Tag }: CategoryChipProps) {
  const color = category?.color;
  const shape =
    'flex size-9 flex-none items-center justify-center rounded-full [&_svg]:size-4 [&_svg]:[stroke-width:1.75]';
  if (!category || !color) {
    return (
      <span aria-hidden className={cn(shape, 'bg-well-2 text-ink-2')}>
        {category ? (
          <CategoryIcon iconName={category.icon || 'tag'} />
        ) : (
          <Fallback strokeWidth={1.75} />
        )}
      </span>
    );
  }
  return (
    <span
      aria-hidden
      className={cn(shape, 'cat-chip')}
      style={{ '--cat': color, backgroundColor: `${color}26` } as CSSProperties}
    >
      <CategoryIcon iconName={category.icon || 'tag'} />
    </span>
  );
}
