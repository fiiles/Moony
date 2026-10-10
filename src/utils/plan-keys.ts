import type { CashflowReportItem } from '@shared/schema';

/**
 * Keys of `cashflow_plan_exclusions` as the backend validates them
 * (`SetCashflowPlanInclusion::validate`): an automatic row is keyed by its
 * source id (`loan:<id>`, `stock:<ticker>`, …), a manual row by `item:<id>`,
 * a whole category by `category:<key>`.
 */
export function planInclusionKey(item: Pick<CashflowReportItem, 'id' | 'sourceId'>): string {
  return item.sourceId ?? `item:${item.id}`;
}

export function categoryInclusionKey(categoryKey: string): string {
  return `category:${categoryKey}`;
}
