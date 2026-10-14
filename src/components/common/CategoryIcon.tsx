import { Tag } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { cn } from '@/lib/utils';
import { ICON_MAP } from './category-icons-map';

/** Render icon component from name */
export function CategoryIcon({ iconName, className }: { iconName: string; className?: string }) {
  const IconComponent = ICON_MAP[iconName] || Tag;
  return <IconComponent className={className} />;
}

/** The category's icon in a round chip tinted with its color (neutral without a category). */
export function CategoryIconChip({
  category,
  className,
}: {
  category?: Pick<TransactionCategory, 'icon' | 'color'> | null;
  className?: string;
}) {
  const color = category?.color || '#9E9E9E';
  return (
    <span
      aria-hidden="true"
      className={cn(
        'grid size-7 shrink-0 place-items-center rounded-full [&_svg]:size-[15px]',
        className
      )}
      style={{ backgroundColor: `${color}26`, color }}
    >
      <CategoryIcon iconName={category?.icon || 'tag'} />
    </span>
  );
}
