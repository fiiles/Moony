import { bankAccountsApi } from '@/lib/tauri-api';
import type { BankTransaction, TransactionFilters, TransactionQueryResult } from '@shared/schema';
/** Complete ledger for summary/chart calculations, requested in bounded server pages. */
export async function getCompleteBankHistory(
  accountId: string,
  filters: TransactionFilters = {}
): Promise<TransactionQueryResult> {
  const transactions: BankTransaction[] = [];
  let total: number;
  do {
    const page = await bankAccountsApi.getTransactions(accountId, {
      ...filters,
      limit: 1000,
      offset: transactions.length,
    });
    total = page.total;
    if (!page.transactions.length && transactions.length < total)
      throw new Error('validation.paginationInvalid');
    transactions.push(...page.transactions);
  } while (transactions.length < total);
  return { transactions, total };
}
