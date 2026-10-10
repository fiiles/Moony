import type { QueryClient } from '@tanstack/react-query';
import { categorizationApi, portfolioApi } from '@/lib/tauri-api';

/** One refresh contract for local writes and external bank events. */
export async function refreshMoneyFlows(client: QueryClient, refreshIdentities = false) {
  const keys = [
    'bank-accounts',
    'bank-account',
    'bank-account-zones',
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
  ];
  await Promise.all(keys.map((key) => client.invalidateQueries({ queryKey: [key] })));
  if (refreshIdentities) await categorizationApi.loadOwnIbans();
  await portfolioApi.recordSnapshot();
  await client.invalidateQueries({ queryKey: ['portfolio-history'] });
}
