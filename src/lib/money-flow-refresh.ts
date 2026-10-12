import type { QueryClient } from '@tanstack/react-query';
import { categorizationApi, portfolioApi } from '@/lib/tauri-api';

export const MONEY_FLOW_KEYS = [
  'bank-accounts',
  'bank-account',
  'bank-account-zones',
  'bank-balance-records',
  'bank-transactions',
  'import-batches',
  'transaction-categories',
  'transaction-category-usage',
  'budgeting-report',
  'budget-goals',
  'budgeting-category-transactions',
  'effective-transaction-flows',
  'cashflow-report',
  'learnedPayees',
  'recategorization-preview',
  'customRules',
  'ruleHits',
  'categorizationOverview',
  'rulePacks',
  'packRules',
  'categorization-stats',
  'categorization-overview',
  'recent-moves',
  'portfolio-metrics',
] as const;

/** One refresh contract for local writes and external bank events. */
export async function refreshMoneyFlows(client: QueryClient, refreshIdentities = false) {
  await Promise.all(MONEY_FLOW_KEYS.map((key) => client.invalidateQueries({ queryKey: [key] })));
  if (refreshIdentities) await categorizationApi.loadOwnIbans();
  await portfolioApi.recordSnapshot();
  await client.invalidateQueries({ queryKey: ['portfolio-history'] });
}
