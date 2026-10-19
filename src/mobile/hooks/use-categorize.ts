import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type {
  BankTransaction,
  ChangedRow,
  RecategorizationPreview,
  TransactionCategory,
} from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { learnIdentity, pickOutcome } from '@/mobile/lib/category-pick';

/** What a pick does besides setting the row's category (the desktop's `SelectOptions`). */
export interface PickOptions {
  /** Learn from this pick (or, when a custom rule decides, retarget that rule). */
  remember: boolean;
  /** Also give the category to the other payments the rewritten rule covers. */
  applyHistory: boolean;
  /** Display name of a category `categories` does not hold yet (one created a moment ago). */
  categoryName?: string;
  /**
   * Called once the learning has written, with the other payments it categorized (empty when it
   * took none along). Not called when there was nothing to learn or the learning failed. It runs
   * after `pick` has resolved, never before.
   */
  onApplied?: (changed: ChangedRow[]) => void;
  /**
   * The caller reports the learning itself (the queue's expansion): no toast for it, and so no
   * undo toast either. A failure is still said.
   */
  quiet?: boolean;
}

export interface Categorize {
  /** What decides this payee's category today and how many other payments a remembered pick covers. */
  preview: RecategorizationPreview | undefined;
  /** The preview is on its way and there is nothing to show yet. */
  previewLoading: boolean;
  /**
   * Sets the row's category (the write refreshes the money flows) and resolves `true` once it is
   * saved — or was already this category; `false` when the write failed (its toast has been
   * shown) and nothing else was done. The learning that follows ("remember", "also categorize N
   * others") runs on its own after that and reports itself in toasts, with an undo.
   */
  pick: (categoryId: string, options: PickOptions) => Promise<boolean>;
  /** The category is being saved. */
  busy: boolean;
}

/**
 * The phone's category pick with learning, ported from the desktop's `CategorySelector.handleSelect`
 * (spec §8.4): save the category, then — unless "remember" is off, the payee is an own account or
 * has no identity — learn the pick for this account and direction, or re-point the custom rule
 * that decides it, optionally recategorizing the other payments it covers; the toast offers an
 * undo for those. `tx` is `null` while the picker is closed, which keeps the preview from loading.
 */
export function useCategorize(
  tx: BankTransaction | null,
  categories: readonly TransactionCategory[]
): Categorize {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const categoryName = useCategoryName();
  const { setCategory } = useLedgerMutations();

  const txId = tx?.id;
  const previewKey = ['recategorization-preview', txId];
  const loadPreview = () => categorizationApi.previewRecategorization(txId as string);
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: loadPreview,
    enabled: txId !== undefined,
    staleTime: 0,
  });

  const showError = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  // The change is already saved when this runs, so a failed refresh is reported on its own and
  // never hides the toast (and its undo) of the change that did happen.
  const refreshAfterWrite = async () => {
    try {
      await refreshMoneyFlows(queryClient);
    } catch (error) {
      showError(error);
    }
  };

  const historyToast = (
    title: string,
    description: string | undefined,
    changed: ChangedRow[],
    quiet = false
  ) => {
    if (quiet) return;
    if (changed.length === 0) {
      toast(title, { description });
      return;
    }
    toast(title, {
      description,
      duration: 8000,
      action: {
        label: t('categorization.undoHistory'),
        onClick: () => {
          void (async () => {
            try {
              const restored = await categorizationApi.restoreTransactionCategories(changed);
              toast(t('categorization.historyRestored', { count: restored }));
            } catch (error) {
              showError(error);
              return;
            }
            await refreshAfterWrite();
          })();
        },
      },
    });
  };

  const nameOf = (id: string) => {
    const category = categories.find((c) => c.id === id);
    return category ? categoryName(category) : id;
  };

  /** The learning branch of a pick, after the category is saved. Never throws: it says what went wrong. */
  const learnFrom = async (
    row: BankTransaction,
    categoryId: string,
    options: PickOptions,
    known: RecategorizationPreview | undefined
  ) => {
    // A one-time categorization (switch off): nothing to look up
    if (!options.remember) return;
    const pickedName = options.categoryName ?? nameOf(categoryId);
    const identity = learnIdentity(row);
    try {
      // What decides this payee decides where the pick goes, so it must not depend on whether the
      // sheet's preview happened to load in time: fetch it now when it is missing.
      const info =
        known ?? (await queryClient.fetchQuery({ queryKey: previewKey, queryFn: loadPreview }));
      const outcome = pickOutcome(info, options.remember, identity !== null);
      if (outcome === 'skip') return;

      const rule = outcome === 'retarget' ? info?.rule : null;
      if (rule) {
        const changed = await categorizationApi.retargetCustomRule(
          rule.id,
          categoryId,
          options.applyHistory
        );
        historyToast(
          changed.length > 0
            ? t('categorization.ruleRetargetedApplied', {
                rule: rule.name,
                category: pickedName,
                count: changed.length,
              })
            : t('categorization.ruleRetargeted', { rule: rule.name, category: pickedName }),
          undefined,
          changed,
          options.quiet
        );
        options.onApplied?.(changed);
        await refreshAfterWrite();
        return;
      }

      // The new rule is limited to this own account, direction and reliable identity
      const payee = identity?.payee ?? null;
      const iban = identity?.iban ?? null;
      const changed = await categorizationApi.learn(
        {
          payee,
          counterpartyIban: iban,
          categoryId,
          bankAccountId: row.bankAccountId,
          isCredit: row.type === 'credit',
          counterpartyBankCode: row.counterpartyBankCode ?? null,
          counterpartyCountry: row.counterpartyCountry ?? null,
        },
        options.applyHistory
      );
      const learnKey = payee ?? iban ?? 'unknown';
      historyToast(
        changed.length > 0
          ? t('categorization.learnedApplied', { count: changed.length })
          : t('categorization.learned'),
        changed.length > 0 ? `"${learnKey}" → ${pickedName}` : `"${learnKey}"`,
        changed,
        options.quiet
      );
      options.onApplied?.(changed);
      await refreshAfterWrite();
    } catch (error) {
      // The preview could not be fetched or the write failed: nothing was remembered, the row stays saved
      showError(error);
    }
  };

  const pick = async (categoryId: string, options: PickOptions) => {
    if (!tx) return false;
    if (categoryId === tx.categoryId) return true;
    try {
      await setCategory.mutateAsync({ id: tx.id, categoryId });
    } catch {
      return false; // the mutation has shown its error
    }
    void learnFrom(tx, categoryId, options, preview.data);
    return true;
  };

  return {
    preview: preview.data,
    previewLoading: preview.isFetching && !preview.data,
    pick,
    busy: setCategory.isPending,
  };
}
