import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, ChevronRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import type { BankAccount, CsvImportResult } from '@shared/schema';
import { cn } from '@/lib/utils';
import {
  balanceUpdatePayload,
  formatBalance,
  nextStepsFor,
  regionName,
  type NextStepId,
} from './import-config';
import { useEnableRulePack } from './use-csv-import-mutations';

const STEP_ORDER: readonly NextStepId[] = ['show', 'review', 'rulePack', 'balance'];

interface NextStepsProps {
  accountId: string;
  result: CsvImportResult;
  /** Current account; its balance decides whether "set balance" is offered. */
  account: BankAccount | null | undefined;
  /** Locale for the country name and the balance, e.g. `cs-CZ`. */
  locale: string;
  /** Name of the imported file, for the "show" row. */
  fileName: string;
  /** Present when the page can show a date range: offers "Show imported transactions". */
  onShowImported?: () => void;
  /** Present when the page can filter to uncategorized rows. */
  onReviewUncategorized?: () => void;
  /** Runs the page's auto-categorization (after a rule pack was enabled). */
  onAutoCategorize?: () => void | Promise<void>;
}

interface RowSpec {
  label: string;
  hint?: string;
  action: string;
  primary?: boolean;
  busy?: boolean;
  onClick: () => void;
}

/** What to do after an import, as the prototype's "Co dál" list. */
export function NextSteps({
  accountId,
  result,
  account,
  locale,
  fileName,
  onShowImported,
  onReviewUncategorized,
  onAutoCategorize,
}: NextStepsProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { updateAccount } = useBankAccountMutations();
  const enableRulePack = useEnableRulePack();
  const [done, setDone] = useState<ReadonlySet<NextStepId>>(new Set());

  const markDone = (id: NextStepId) => setDone((current) => new Set(current).add(id));

  const suggested = nextStepsFor(result, {
    canShowImported: onShowImported != null,
    canReviewUncategorized: onReviewUncategorized != null,
    accountBalance: account?.balance ?? null,
  });
  // A step that was just carried out stays on screen, marked done, even though
  // it no longer applies (the balance now matches, the pack is enabled).
  const visible = STEP_ORDER.filter((id) => suggested.includes(id) || done.has(id));
  if (visible.length === 0) return null;

  const packId = result.suggestedRulePack;
  const country = packId ? regionName(packId, locale) : '';

  const handleRulePack = async () => {
    if (!packId) return;
    try {
      await enableRulePack.mutateAsync(packId);
    } catch (e: unknown) {
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
      return;
    }
    markDone('rulePack');
    toast(t('csvImport.next.rulePackEnabled', { country }));
    try {
      await onAutoCategorize?.();
    } catch (e) {
      console.error('Auto-categorization after enabling a rule pack failed:', e);
    }
  };

  const handleBalance = () => {
    if (!account || result.lastBalance == null) return;
    updateAccount.mutate(
      { id: account.id, data: balanceUpdatePayload(account, result.lastBalance) },
      {
        onSuccess: () => {
          markDone('balance');
          queryClient.invalidateQueries({ queryKey: ['bank-account', accountId] });
        },
      }
    );
  };

  const rows: Record<NextStepId, RowSpec> = {
    show: {
      label: t('csvImport.next.show'),
      hint: t('csvImport.nextRows.showHint', { count: result.importedCount, file: fileName }),
      action: t('csvImport.nextRows.showAction'),
      onClick: () => onShowImported?.(),
    },
    review: {
      label: t('csvImport.next.review', { count: result.uncategorizedCount }),
      hint: t('csvImport.next.reviewHint'),
      action: t('csvImport.nextRows.reviewAction'),
      onClick: () => onReviewUncategorized?.(),
    },
    rulePack: {
      label: t('csvImport.next.rulePack', { country }),
      hint: t('csvImport.next.rulePackHint'),
      action: t('csvImport.nextRows.rulePackAction'),
      busy: enableRulePack.isPending,
      onClick: () => void handleRulePack(),
    },
    balance: {
      label: t('csvImport.next.setBalance', {
        balance: formatBalance(result.lastBalance ?? '0', account?.currency, locale),
      }),
      hint: account
        ? t('csvImport.nextRows.setBalanceHint', {
            current: formatBalance(account.balance, account.currency, locale),
          })
        : undefined,
      action: t('csvImport.nextRows.setBalanceAction'),
      primary: true,
      busy: updateAccount.isPending,
      onClick: handleBalance,
    },
  };

  return (
    <div>
      <h4 className="mb-1.5 text-caption font-700 text-ink-2">{t('csvImport.done.whatNext')}</h4>
      <ul className="m-0 list-none p-0">
        {visible.map((id) => {
          const row = rows[id];
          const isDone = done.has(id);
          return (
            <li
              key={id}
              className="flex items-center gap-3 border-b border-line-soft py-[11px] text-table last:border-0"
            >
              {isDone ? (
                <Check className="size-[15px] shrink-0 text-gain" aria-hidden />
              ) : (
                <ChevronRight className="size-[15px] shrink-0 text-ink-4" aria-hidden />
              )}
              <span className="min-w-0 flex-1">
                <b className={cn('block font-600', isDone ? 'text-ink-3' : 'text-ink')}>
                  {row.label}
                </b>
                {row.hint && (
                  <small className="mt-px block text-micro text-ink-4">{row.hint}</small>
                )}
              </span>
              {!isDone && (
                <Button
                  type="button"
                  size="sm"
                  variant={row.primary ? 'default' : 'outline'}
                  loading={row.busy}
                  disabled={row.busy}
                  onClick={row.onClick}
                >
                  {row.action}
                </Button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
