import { useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { bankAccountsApi } from '@/lib/tauri-api';
import { categoryDisplayName, type NamedCategory } from '@/utils/category-name';

/** All transaction categories (system and custom), in display order. */
export function useCategories() {
  const { data: categories = [], isLoading } = useQuery({
    queryKey: ['transaction-categories'],
    queryFn: () => bankAccountsApi.getCategories(),
  });

  return { categories, isLoading };
}

/** How many rows reference each category (what a delete would have to move). */
export function useCategoryUsage() {
  const { data: usage = [], isLoading } = useQuery({
    queryKey: ['transaction-category-usage'],
    queryFn: () => bankAccountsApi.getCategoryUsage(),
  });

  return { usage, isLoading };
}

/**
 * Returns a function that gives the display name of a category: seeded system
 * categories are translated, custom and renamed ones show their stored name.
 */
export function useCategoryName() {
  const { t } = useTranslation('bank_accounts');
  return useCallback((category: NamedCategory) => categoryDisplayName(category, t), [t]);
}
