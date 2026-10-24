import { useMemo } from 'react';
import { useQueries, useQuery } from '@tanstack/react-query';
import type {
  AssetValuation,
  InsurancePolicy,
  Loan,
  LoanSchedule,
  RealEstate,
  RealEstateOneTimeCost,
} from '@shared/schema';
import { todayUtcDay } from '@shared/calculations';
import { useDatedMainConvert, useMainConvert } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { insuranceApi, loansApi, realEstateApi } from '@/lib/tauri-api';
import {
  propertyRow,
  yearlyBalance,
  type PropertyRow,
  type YearlyBalance,
} from '@/utils/real-estate';
import {
  costsMissing,
  linkCandidates,
  propertyMissing,
  propertyTrace,
  type PropertyTrace,
} from '@/mobile/lib/property-detail';

/** Stable empty lists: a new `[]` per render would keep the memos below from ever settling. */
const NO_VALUATIONS: AssetValuation[] = [];
const NO_COSTS: RealEstateOneTimeCost[] = [];
const NO_LOANS: Loan[] = [];
const NO_POLICIES: InsurancePolicy[] = [];

/** The loan schedules' data in the order of the queries; structurally shared, so it settles. */
const scheduleData = (results: { data: LoanSchedule | undefined }[]) =>
  results.map((result) => result.data);

export interface PropertyData {
  /** The property; `null` until loaded, and when there is no such property. */
  property: RealEstate | null;
  /** The list's derived numbers (`propertyRow`): value, purchase, yield, equity, LTV. */
  row: PropertyRow | null;
  /** The yearly balance in the main currency (`yearlyBalance`). */
  balance: YearlyBalance | null;
  /** Dated estimates, oldest first. */
  valuations: AssetValuation[];
  /** The linked loans and policies. */
  loans: Loan[];
  policies: InsurancePolicy[];
  /** The one-time costs (the desktop's `['real-estate-costs', id]`; the costs section draws them). */
  costs: RealEstateOneTimeCost[];
  /** The value line and its marks. */
  trace: Pick<PropertyTrace, 'points' | 'events'>;
  /** Currencies converted without a rate (the quality note); none while the quotes load. */
  missing: string[];
  /** The property or the quotes are still loading: a null conversion would read as a missing rate. */
  isLoading: boolean;
  /** Loaded, and there is no such property (a deep link to a deleted one). */
  notFound: boolean;
}

/**
 * Everything the property detail shows (spec §8.8), from the queries of the desktop's
 * `RealEstateDetail` so the cache is shared: the property, its estimates, one-time costs, linked
 * loans and policies, and the loans' schedules (which split the next year's payments into interest
 * and principal). The numbers are the desktop's, in the main currency: today's amounts at the
 * current quotes, the purchase, the costs and the estimates at the rates of their own day; an
 * amount without a rate stays unknown (never 0) and its currency is named in `missing`.
 *
 * The result and each of its parts are memoized, so a screen can keep it (`useRetained`) without
 * chasing a new object on every render.
 */
export function useProperty(id: string | undefined): PropertyData {
  const { fxReady } = useCurrency();

  const { data: property = null, isLoading } = useQuery<RealEstate | null>({
    queryKey: ['real-estate', id],
    queryFn: () => realEstateApi.get(id!),
    enabled: !!id,
  });
  const { data: valuations = NO_VALUATIONS } = useQuery<AssetValuation[]>({
    queryKey: ['real-estate-valuations', id],
    queryFn: () => realEstateApi.getValuations(id!),
    enabled: !!id,
  });
  const { data: costs = NO_COSTS } = useQuery<RealEstateOneTimeCost[]>({
    queryKey: ['real-estate-costs', id],
    queryFn: () => realEstateApi.getCosts(id!),
    enabled: !!id,
  });
  const { data: loans = NO_LOANS } = useQuery<Loan[]>({
    queryKey: ['real-estate-loans', id],
    queryFn: () => realEstateApi.getLoans(id!),
    enabled: !!id,
  });
  const { data: policies = NO_POLICIES } = useQuery<InsurancePolicy[]>({
    queryKey: ['real-estate-insurances', id],
    queryFn: () => realEstateApi.getInsurances(id!),
    enabled: !!id,
  });
  // Schedules split the next year's payments into interest and principal
  const schedules = useQueries({
    queries: loans.map((loan) => ({
      queryKey: ['loan-schedule', loan.id],
      queryFn: () => loansApi.getSchedule(loan.id),
      staleTime: 60 * 1000,
    })),
    combine: scheduleData,
  });

  // Today's amounts at the current quotes (the row and the yearly balance); the purchase price, the
  // one-time costs and the estimates at the rates of their own day.
  const convert = useMainConvert();
  const firstDay = Math.min(
    property ? (property.purchaseDate ?? property.createdAt) : Infinity,
    ...valuations.map((v) => v.valuedAt),
    ...costs.map((c) => c.date)
  );
  const dated = useDatedMainConvert(Number.isFinite(firstDay) ? firstDay : undefined);

  const row = useMemo(
    () => (property ? propertyRow(property, loans, false, convert, dated.convertAt) : null),
    [property, loans, convert, dated.convertAt]
  );
  const today = todayUtcDay();
  const balance = useMemo(
    () => (property ? yearlyBalance(property, loans, schedules, policies, today, convert) : null),
    [property, loans, schedules, policies, today, convert]
  );

  // The line ends on today's value, at today's rates, like the hero. Whole minutes: a trace that
  // changed on every render would redraw the chart for nothing.
  const nowSec = Math.floor(Date.now() / 60_000) * 60;
  const purchaseDay = property?.purchaseDate ?? null;
  const purchase = row?.purchase ?? null;
  const value = row?.value ?? null;
  const fullTrace = useMemo(
    () =>
      propertyTrace(
        valuations,
        { day: purchaseDay, value: purchase },
        nowSec,
        value,
        dated.convertAt
      ),
    [valuations, purchaseDay, purchase, value, nowSec, dated.convertAt]
  );
  const trace = useMemo(
    () => ({ points: fullTrace.points, events: fullTrace.events }),
    [fullTrace]
  );

  const unconvertedCosts = useMemo(
    () => costsMissing(costs, dated.convertAt),
    [costs, dated.convertAt]
  );
  const missing = useMemo(
    () =>
      propertyMissing({
        fxReady,
        datedReady: dated.ready,
        now: [...(row?.missing ?? []), ...(balance?.missing ?? [])],
        dated: [...(row?.missingPurchase ?? []), ...unconvertedCosts, ...fullTrace.missing],
      }),
    [fxReady, dated.ready, row, balance, unconvertedCosts, fullTrace]
  );

  const loading = isLoading || !fxReady;
  const notFound = !isLoading && property === null;
  return useMemo(
    () => ({
      property,
      row,
      balance,
      valuations,
      loans,
      policies,
      costs,
      trace,
      missing,
      isLoading: loading,
      notFound,
    }),
    [property, row, balance, valuations, loans, policies, costs, trace, missing, loading, notFound]
  );
}

/**
 * The loans the Připojit sheet offers: those that are not linked to any property (`get_available_
 * loans`; linking one that belongs to another property would silently move it), without the ones
 * already linked here. Loaded only while the sheet is open.
 */
export function useLinkableLoans(linked: readonly Loan[], enabled: boolean) {
  const { data, isLoading } = useQuery<Loan[]>({
    queryKey: ['available-loans'],
    queryFn: () => loansApi.getAvailable(),
    enabled,
  });
  return { candidates: linkCandidates(data ?? NO_LOANS, linked), isLoading };
}

/** The policies the Připojit sheet offers: the same rule as `useLinkableLoans` (`get_available_insurances`). */
export function useLinkablePolicies(linked: readonly InsurancePolicy[], enabled: boolean) {
  const { data, isLoading } = useQuery<InsurancePolicy[]>({
    queryKey: ['available-insurances'],
    queryFn: () => insuranceApi.getAvailable(),
    enabled,
  });
  return { candidates: linkCandidates(data ?? NO_POLICIES, linked), isLoading };
}
