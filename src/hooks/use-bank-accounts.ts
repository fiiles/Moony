import { useQuery } from '@tanstack/react-query';
import { bankAccountsApi } from '@/lib/tauri-api';

/** Every bank account with its institution; the pages derive their own totals. */
export function useBankAccounts() {
  const {
    data: accounts = [],
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ['bank-accounts'],
    queryFn: () => bankAccountsApi.getAll(),
    staleTime: 0, // Always consider stale
    refetchOnWindowFocus: true,
  });

  return {
    accounts,
    isLoading,
    refetch,
  };
}

export function useBankAccount(id: string | undefined) {
  const {
    data: account,
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ['bank-account', id],
    queryFn: () => (id ? bankAccountsApi.get(id) : null),
    enabled: !!id,
  });

  return {
    account,
    isLoading,
    refetch,
  };
}

export function useInstitutions() {
  const { data: institutions = [], isLoading } = useQuery({
    queryKey: ['institutions'],
    queryFn: () => bankAccountsApi.getInstitutions(),
  });

  return {
    institutions,
    isLoading,
  };
}
