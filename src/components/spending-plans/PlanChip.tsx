import type { SpendingPlan } from '@shared/schema';
import { CategoryIconChip } from '@/components/common/CategoryIcon';

/**
 * A plan's icon in its color: the category chip's look as a rounded square, so a plan never
 * reads as a category (category chips are round).
 */
export function PlanChip({
  plan,
  size = 'md',
  className,
}: {
  plan: Pick<SpendingPlan, 'icon' | 'color'>;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}) {
  return <CategoryIconChip category={plan} shape="square" size={size} className={className} />;
}
