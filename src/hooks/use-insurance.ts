import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertInsurancePolicy, InsurancePolicy } from '@shared/schema';
import { todayUtcDay } from '@shared/calculations/loan-amortization';
import { insuranceApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import {
  insuranceMetrics,
  insuranceRow,
  policyToInsert,
  type InsuranceMetrics,
  type InsuranceRow,
} from '@/utils/insurance';

/** All policies with the derived rows (premiums, coverage, next dates) and totals. */
export function useInsurance(): {
  policies: InsurancePolicy[];
  rows: InsuranceRow[];
  metrics: InsuranceMetrics;
  isLoading: boolean;
  today: number;
} {
  const { data: policies = [], isLoading } = useQuery<InsurancePolicy[]>({
    queryKey: ['insurance'],
    queryFn: () => insuranceApi.getAll(),
  });
  const today = todayUtcDay();
  const rows = useMemo(() => policies.map((p) => insuranceRow(p, today)), [policies, today]);
  const metrics = useMemo(() => insuranceMetrics(rows, today), [rows, today]);
  return { policies, rows, metrics, isLoading, today };
}

/**
 * Policy mutations. Insurance is not part of the net worth, so no snapshot;
 * the cashflow report and the real-estate links read policies, so they refresh.
 */
export function useInsuranceMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');

  const refresh = () => {
    queryClient.invalidateQueries({ queryKey: ['insurance'] });
    queryClient.invalidateQueries({ queryKey: ['available-insurances'] });
    queryClient.invalidateQueries({ queryKey: ['real-estate-insurances'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
  };
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const update = useMutation({
    mutationFn: ({ id, data }: { id: string; data: InsertInsurancePolicy }) =>
      insuranceApi.update(id, data),
    onSuccess: () => {
      refresh();
      toast(t('toast.updated'));
    },
    onError,
  });

  /** "Ukončit" writes the end date and the inactive status; "Obnovit" clears both. */
  const setStatus = useMutation({
    mutationFn: ({
      policy,
      status,
      endDate,
    }: {
      policy: InsurancePolicy;
      status: 'active' | 'inactive';
      endDate?: number;
    }) =>
      insuranceApi.update(policy.id, {
        ...policyToInsert(policy),
        status,
        endDate: status === 'active' ? undefined : endDate,
      }),
    onSuccess: (_policy, variables) => {
      refresh();
      toast(variables.status === 'active' ? t('toast.reactivated') : t('toast.ended'));
    },
    onError,
  });

  const remove = useMutation({
    mutationFn: (id: string) => insuranceApi.delete(id),
    onSuccess: () => {
      refresh();
      toast(t('toast.deleted'));
    },
    onError,
  });

  return { update, setStatus, remove };
}
