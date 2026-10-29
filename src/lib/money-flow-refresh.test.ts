import { QueryClient, QueryObserver } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { refreshMoneyFlows } from './money-flow-refresh';

vi.mock('@/lib/tauri-api', () => ({
  portfolioApi: { recordSnapshot: async () => undefined },
  categorizationApi: { loadOwnIbans: async () => undefined },
}));

const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach((client) => client.clear()));
function client() {
  const result = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } });
  clients.push(result);
  return result;
}

describe('money flow refresh', () => {
  it('invalidates the cached opening month after bank transactions change', async () => {
    const cache = client();
    cache.setQueryData(['budgeting-latest-transaction'], 100);
    await refreshMoneyFlows(cache);
    expect(cache.getQueryState(['budgeting-latest-transaction'])?.isInvalidated).toBe(true);
  });

  it('refreshes an observed budget report and invalidates other periods and dialogs', async () => {
    const cache = client();
    let limit = '100';
    const key = ['budgeting-report', '2026-01', 'EUR'];
    const queryFn = async () => ({ limit });
    await cache.fetchQuery({ queryKey: key, queryFn });
    const observer = new QueryObserver(cache, { queryKey: key, queryFn });
    const stop = observer.subscribe(() => undefined);
    const inactive = [
      ['budgeting-report', '2026', 'EUR'],
      ['budget-goals'],
      ['budget-goal-history'],
      ['budgeting-category-transactions', 'groceries'],
      ['portfolio-history'],
      ['portfolio-metrics'],
      ['cashflow-report'],
    ];
    inactive.forEach((key) => cache.setQueryData(key, {}));
    limit = '200';
    try {
      await refreshMoneyFlows(cache);
      expect(cache.getQueryData(key)).toEqual({ limit: '200' });
      inactive.forEach((key) => expect(cache.getQueryState(key)?.isInvalidated).toBe(true));
    } finally {
      stop();
    }
  });

  it('invalidates the spending plan views, whose sums read the ledger', async () => {
    const cache = client();
    const keys = [
      ['spending-plans', 'index', 0],
      ['spending-plans', 2026, 'CZK', 0],
      ['spending-plan', 'plan-1', 0],
      ['spending-plan-transactions', 'plan-1'],
      ['spending-plan-candidates', { query: '' }],
    ];
    keys.forEach((key) => cache.setQueryData(key, {}));
    await refreshMoneyFlows(cache);
    keys.forEach((key) => expect(cache.getQueryState(key)?.isInvalidated).toBe(true));
  });
});
