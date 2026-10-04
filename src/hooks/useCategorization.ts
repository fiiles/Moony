/**
 * React hook for transaction categorization
 *
 * Provides categorization functionality with automatic learning and caching.
 */

import { useState, useCallback, useRef } from 'react';
import {
  categorizationApi,
  type TransactionInput,
  type CategorizationResult,
  type CategorizationStats,
} from '@/lib/tauri-api';
import type { BankTransaction } from '../../shared/schema';

/**
 * Convert BankTransaction to TransactionInput for categorization
 */
function toTransactionInput(tx: BankTransaction): TransactionInput {
  return {
    id: tx.id,
    description: tx.description ?? undefined,
    counterparty: tx.counterpartyName ?? undefined,
    counterpartyIban: tx.counterpartyIban ?? undefined,
    amount: parseFloat(tx.amount),
    isCredit: tx.type === 'credit',
    bankAccountId: tx.bankAccountId,
  };
}

/**
 * Extract category ID from categorization result
 */
function getCategoryId(result: CategorizationResult): string | null {
  if (result.type === 'Match') {
    return result.data.categoryId;
  }
  return null;
}

/**
 * Check if result is a definitive match
 */
function isMatch(result: CategorizationResult): boolean {
  return result.type === 'Match';
}

interface UseCategorization {
  // Categorize a single transaction
  categorize: (transaction: BankTransaction) => Promise<CategorizationResult>;
  // Categorize multiple transactions
  categorizeBatch: (transactions: BankTransaction[]) => Promise<CategorizationResult[]>;
  // Learn from user's manual categorization (hierarchical: payee + iban)
  learn: (
    payee: string | null,
    counterpartyIban: string | null,
    categoryId: string
  ) => Promise<void>;
  // Get engine statistics
  getStats: () => Promise<CategorizationStats>;
  // Clear the categorization cache (call before re-running auto-categorize)
  clearCache: () => void;
  // Loading state
  isLoading: boolean;
  // Error state
  error: string | null;
  // Clear error
  clearError: () => void;
}

/**
 * Hook for transaction categorization
 *
 * @example
 * ```tsx
 * function TransactionRow({ transaction }) {
 *   const { categorize, learn, isLoading } = useCategorization();
 *
 *   useEffect(() => {
 *     categorize(transaction).then(result => {
 *       if (result.type === 'Match') {
 *         // apply result.data.categoryId
 *       }
 *     });
 *   }, [transaction.id]);
 *
 *   const handleManualCategory = async (categoryId: string) => {
 *     // User selected a category - learn from it
 *     await learn(transaction.counterpartyName, categoryId);
 *   };
 * }
 * ```
 */
export function useCategorization(): UseCategorization {
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Cache to avoid repeated categorization calls
  const cacheRef = useRef<Map<string, CategorizationResult>>(new Map());

  const categorize = useCallback(
    async (transaction: BankTransaction): Promise<CategorizationResult> => {
      // Check cache first
      const cached = cacheRef.current.get(transaction.id);
      if (cached) {
        return cached;
      }

      setIsLoading(true);
      setError(null);

      try {
        const input = toTransactionInput(transaction);
        const result = await categorizationApi.categorize(input);

        // Cache the result
        cacheRef.current.set(transaction.id, result);

        return result;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        return { type: 'None' };
      } finally {
        setIsLoading(false);
      }
    },
    []
  );

  const categorizeBatch = useCallback(
    async (transactions: BankTransaction[]): Promise<CategorizationResult[]> => {
      // Filter out transactions that are already cached
      const uncached = transactions.filter((tx) => !cacheRef.current.has(tx.id));

      if (uncached.length === 0) {
        // All cached, return from cache
        return transactions.map((tx) => cacheRef.current.get(tx.id) || { type: 'None' });
      }

      setIsLoading(true);
      setError(null);

      try {
        const inputs = uncached.map(toTransactionInput);
        const results = await categorizationApi.categorizeBatch(inputs);

        // Cache the results
        uncached.forEach((tx, i) => {
          cacheRef.current.set(tx.id, results[i]);
        });

        // Return in original order, using cache
        return transactions.map((tx) => cacheRef.current.get(tx.id) || { type: 'None' });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        return transactions.map(() => ({ type: 'None' }));
      } finally {
        setIsLoading(false);
      }
    },
    []
  );

  const learn = useCallback(
    async (
      payee: string | null,
      counterpartyIban: string | null,
      categoryId: string
    ): Promise<void> => {
      if (!categoryId) return;
      // Need at least payee or iban to learn
      if (!payee && !counterpartyIban) return;

      try {
        await categorizationApi.learn(payee, counterpartyIban, categoryId);

        // Invalidate cache for transactions with this payee/iban
        // This is a simple approach - could be optimized
        cacheRef.current.clear();
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
      }
    },
    []
  );

  const getStats = useCallback(async (): Promise<CategorizationStats> => {
    return await categorizationApi.getStats();
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  const clearCache = useCallback(() => {
    cacheRef.current.clear();
  }, []);

  return {
    categorize,
    categorizeBatch,
    learn,
    getStats,
    clearCache,
    isLoading,
    error,
    clearError,
  };
}

// Export utility functions
export { toTransactionInput, getCategoryId, isMatch };

export type { TransactionInput, CategorizationResult, CategorizationStats };
