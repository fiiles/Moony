import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type {
  InsertRealEstateOneTimeCost,
  RealEstate,
  RealEstateOneTimeCost,
  RecurringCost,
} from '@shared/schema';
import { todayUtcDay } from '@shared/calculations';
import { ALL_REAL_ESTATE_VALUATIONS_KEY } from '@/hooks/use-real-estate';
import { useRealEstateMutations } from '@/hooks/use-real-estate-mutations';
import { portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { realEstateUpdatePayload } from '@/utils/real-estate-payload';
import {
  deletePhotoLive,
  uploadToTodayBatch,
  type UploadProgress,
} from '@/mobile/lib/photo-upload';
import { withoutRecurring } from '@/mobile/lib/property-detail';

/** What a link or an unlink acts on: a loan or a policy of the property. */
export interface LinkTarget {
  kind: 'loan' | 'insurance';
  itemId: string;
}

/**
 * The property's writes: delete, and link or unlink a loan or a policy (the costs and the photos
 * have their own hooks below). The caller leaves the
 * screen when a delete has succeeded and then drops the property's own queries
 * (`PropertyDetail`, `useDeleteAndLeave`: removing them while the screen is still up would rebuild
 * them empty).
 *
 * A delete follows the mutation contract (the domain keys, the metrics and the cashflow, then a
 * fresh snapshot and the history) and frees the loans and policies that were linked. A link or an
 * unlink refreshes what the desktop detail does (`invalidateLinks`): the property's own lists, the
 * lists of what can still be linked, the loans and policies, and the cashflow report.
 */
export function usePropertyMutations(id: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  /** What a change of the links touches outside the property's own lists. */
  const invalidateLinked = () => {
    queryClient.invalidateQueries({ queryKey: ['available-insurances'] });
    queryClient.invalidateQueries({ queryKey: ['available-loans'] });
    queryClient.invalidateQueries({ queryKey: ['loans'] });
    queryClient.invalidateQueries({ queryKey: ['insurance'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
  };
  const invalidateLinks = () => {
    queryClient.invalidateQueries({ queryKey: ['real-estate-insurances', id] });
    queryClient.invalidateQueries({ queryKey: ['real-estate-loans', id] });
    invalidateLinked();
  };

  const remove = useMutation({
    mutationFn: () => realEstateApi.delete(id!),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['real-estate', id] });
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', id] });
      queryClient.invalidateQueries({ queryKey: ALL_REAL_ESTATE_VALUATIONS_KEY });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // Its loans and policies are no longer linked to anything
      invalidateLinked();
      // The delete already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
    },
    onError,
  });

  const link = useMutation({
    mutationFn: ({ kind, itemId }: LinkTarget) =>
      kind === 'loan'
        ? realEstateApi.linkLoan(id!, itemId)
        : realEstateApi.linkInsurance(id!, itemId),
    onSuccess: (_, { kind }) => {
      invalidateLinks();
      toast(t(kind === 'loan' ? 'toast.loanLinked' : 'toast.insuranceLinked'));
    },
    onError,
  });

  const unlink = useMutation({
    mutationFn: ({ kind, itemId }: LinkTarget) =>
      kind === 'loan'
        ? realEstateApi.unlinkLoan(id!, itemId)
        : realEstateApi.unlinkInsurance(id!, itemId),
    onSuccess: (_, { kind }) => {
      invalidateLinks();
      toast(t(kind === 'loan' ? 'toast.loanUnlinked' : 'toast.insuranceUnlinked'));
    },
    onError,
  });

  return { remove, link, unlink };
}

/** The key of the property's photo batches (the desktop gallery's own). */
const photoBatchesKey = (id: string | undefined) => ['real-estate-photo-batches', id];

/** A one-time cost to write: a new one, or the one being edited with the data that replaces it. */
export interface OneTimeCostWrite {
  /** The stored cost to replace, or `null` for a new one. */
  cost: RealEstateOneTimeCost | null;
  data: InsertRealEstateOneTimeCost;
}

/**
 * The writes of "Náklady" (spec §8.8): a recurring cost is added through the desktop's
 * `addRecurringCost` (it lives on the property, which the update replaces whole), one that is
 * deleted is the property's update without it (`withoutRecurring`, `realEstateUpdatePayload`),
 * and a one-time cost is created, replaced or deleted through its own commands.
 *
 * A cost changes the yearly balance, the cashflow plan and the net worth, so every write follows
 * the mutation contract in full — unlike the desktop's one-time cost dialogs, which refresh less:
 * the costs, the property and its lists, the metrics and the cashflow report, then a fresh snapshot
 * and the history. A property's update also refreshes its estimates, as the desktop's hook does
 * (a save can write one). Each says what it did in a toast; a refusal is the translated error.
 */
export function usePropertyCostMutations(id: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const { addRecurringCost } = useRealEstateMutations();

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const refresh = async ({ estimates = false }: { estimates?: boolean } = {}) => {
    queryClient.invalidateQueries({ queryKey: ['real-estate-costs', id] });
    queryClient.invalidateQueries({ queryKey: ['real-estate'] });
    queryClient.invalidateQueries({ queryKey: ['real-estate', id] });
    if (estimates) {
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', id] });
      queryClient.invalidateQueries({ queryKey: ALL_REAL_ESTATE_VALUATIONS_KEY });
    }
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot must not turn it into an error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };

  // The desktop's hook refreshes the property, the metrics, the cashflow and the snapshot and says
  // what it did; the costs' own list is refreshed here as well, so that every cost write leaves
  // the same trace. Its failure is its own toast; this mutation only waits for it.
  const addRecurring = useMutation({
    mutationFn: (variables: { realEstate: RealEstate; cost: RecurringCost }) =>
      addRecurringCost.mutateAsync(variables),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['real-estate-costs', id] }),
  });

  const saveOneTime = useMutation({
    mutationFn: ({ cost, data }: OneTimeCostWrite) =>
      cost ? realEstateApi.updateCost(cost.id, data) : realEstateApi.createCost(data),
    onSuccess: async (_saved, { cost }) => {
      await refresh();
      toast(t(cost ? 'toast.costUpdated' : 'toast.costAdded'));
    },
    onError,
  });

  const removeOneTime = useMutation({
    mutationFn: (costId: string) => realEstateApi.deleteCost(costId),
    onSuccess: async () => {
      await refresh();
      toast(t('toast.costDeleted'));
    },
    onError,
  });

  const removeRecurring = useMutation({
    mutationFn: ({ property, index }: { property: RealEstate; index: number }) =>
      realEstateApi.update(
        property.id,
        realEstateUpdatePayload(property, {
          recurringCosts: withoutRecurring(property.recurringCosts ?? [], index),
        })
      ),
    onSuccess: async () => {
      await refresh({ estimates: true });
      toast(t('toast.costDeleted'));
    },
    onError,
  });

  return { addRecurring, saveOneTime, removeOneTime, removeRecurring };
}

/**
 * The writes of "Galerie" (spec §8.8): picked photos go into today's batch one by one
 * (`uploadToTodayBatch`; `progress` is the photo on its way, for "Nahrávám 2 / 5"), and a photo is
 * deleted by its id (`deletePhotoLive`): what goes with it — the whole batch, if the photo is still
 * its only one, in the one transaction of `deletePhotoBatch` — is decided on the batches as they are
 * when the delete is confirmed, not as they were when the photo was opened. Photos carry no money,
 * so — like the desktop gallery — only the property's batches are refreshed, not the metrics or the
 * snapshot.
 */
export function usePropertyPhotoMutations(id: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('realEstate');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [progress, setProgress] = useState<UploadProgress | null>(null);

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };
  const refresh = () => queryClient.invalidateQueries({ queryKey: photoBatchesKey(id) });

  const upload = useMutation({
    mutationFn: (files: File[]) =>
      uploadToTodayBatch(realEstateApi, id!, files, todayUtcDay(), setProgress),
    onSuccess: ({ skipped }) => {
      toast(t('gallery.photosAdded'), {
        description: skipped > 0 ? tm('property.photosSkipped', { count: skipped }) : undefined,
      });
    },
    onError,
    onSettled: () => {
      setProgress(null);
      return refresh();
    },
  });

  const removePhoto = useMutation({
    mutationFn: (photoId: string) => deletePhotoLive(realEstateApi, id!, photoId),
    onSuccess: async () => {
      await refresh();
      toast(tm('property.photoDeleted'));
    },
    onError,
  });

  return { upload, progress, removePhoto };
}
