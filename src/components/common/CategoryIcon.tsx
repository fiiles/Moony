import type { CSSProperties } from 'react';
import { Tag } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { cn } from '@/lib/utils';
import { ICON_MAP } from './category-icons-map';

/** Render icon component from name */
export function CategoryIcon({ iconName, className }: { iconName: string; className?: string }) {
  const IconComponent = ICON_MAP[iconName] || Tag;
  return <IconComponent className={className} />;
}

const CHIP_SIZE = {
  sm: { box: 'size-5 [&_svg]:size-3', square: 'rounded-[6px]' },
  md: { box: 'size-7 [&_svg]:size-[15px]', square: 'rounded-[9px]' },
  lg: { box: 'size-11 [&_svg]:size-[22px]', square: 'rounded-[13px]' },
} as const;

/** The category's icon in a chip tinted with its color (neutral without a category). */
export function CategoryIconChip({
  category,
  className,
  shape = 'round',
  size = 'md',
}: {
  category?: Pick<TransactionCategory, 'icon' | 'color'> | null;
  className?: string;
  shape?: 'round' | 'square';
  size?: 'sm' | 'md' | 'lg';
}) {
  const color = category?.color || '#9E9E9E';
  const dims = CHIP_SIZE[size];
  return (
    <span
      aria-hidden="true"
      className={cn(
        'cat-chip grid shrink-0 place-items-center',
        dims.box,
        shape === 'square' ? dims.square : 'rounded-full',
        className
      )}
      style={{ '--cat': color, backgroundColor: `${color}26` } as CSSProperties}
    >
      <CategoryIcon iconName={category?.icon || 'tag'} />
    </span>
  );
}
