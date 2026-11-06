import type { SpendingPlan } from '@shared/schema';
import { PlanChip as SquarePlanChip } from '@/components/spending-plans/PlanChip';

/**
 * A plan's chip on an entity row (README §13, `.m-row__chip`): the desktop's square plan chip at
 * the row's 36 pt size (a rounded square, 16 pt glyph), so a plan never reads as a round category.
 */
export function PlanRowChip({ plan }: { plan: Pick<SpendingPlan, 'icon' | 'color'> }) {
  return (
    <SquarePlanChip
      plan={plan}
      className="size-9 rounded-[10px] [&_svg]:size-4 [&_svg]:[stroke-width:1.75]"
    />
  );
}
