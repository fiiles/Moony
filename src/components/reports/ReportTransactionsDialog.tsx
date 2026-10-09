import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { FlowKind, ReportTransaction } from '@shared/schema';
import { bankAccountsApi, cashflowApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { useCategories } from '@/hooks/use-categories';
import { cn } from '@/lib/utils';
import { formatReportMoney } from '@/utils/report-currencies';
import { rowFlowLabel } from '@/utils/money-flows';
import { isMovedOut, movedCount } from '@/utils/report-drilldown';
import { CategorySelector } from '@/components/bank-accounts/CategorySelector';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { ReportError } from './ReportQualityNote';

export interface ReportTarget {
  name: string;
  flowKind?: FlowKind;
  /** Category id, or `uncategorized` */
  categoryId?: string;
  expectedCount?: number;
}

const PAGE = 100;

/**
 * The rows behind one report line, with a category picker per row. The list is
 * loaded once per opening and stays put while the user recategorizes: a row
 * that leaves the line is dimmed and marked as moved, while the report behind
 * the dialog refreshes right away.
 */
function ReportTransactionsDialogContent({
  target,
  start,
  end,
  accountIds,
  reportCurrency,
  ratesTimestamp,
  onClose,
}: {
  target: ReportTarget | null;
  start: number;
  end: number;
  accountIds?: string[];
  reportCurrency: string;
  ratesTimestamp: number;
  onClose: () => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tBank } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const { categories } = useCategories();
  const [page, setPage] = useState(0);
  const [snapshot, setSnapshot] = useState<ReportTransaction[] | null>(null);
  const [changes, setChanges] = useState<ReadonlyMap<string, string | null>>(new Map());
  // Outside the money-flows refresh keys and dropped on close, so a
  // recategorization never reshuffles the open list.
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['report-drilldown', target, start, end, accountIds, reportCurrency, ratesTimestamp],
    queryFn: () =>
      cashflowApi.getTransactions(start, end, {
        accountIds,
        flowKind: target?.flowKind,
        categoryId: target?.categoryId,
        reportCurrency,
      }),
    enabled: target != null,
    gcTime: 0,
    staleTime: Infinity,
  });
  if (snapshot === null && data) setSnapshot(data);
  const rows = snapshot ?? [];
  const moved = movedCount(rows, changes, target?.categoryId);

  const categoryOf = (tx: ReportTransaction) =>
    changes.has(tx.id) ? (changes.get(tx.id) ?? null) : tx.categoryId;

  const changeCategory = async (tx: ReportTransaction, categoryId: string | null) => {
    if (!categoryId) return;
    try {
      await bankAccountsApi.updateTransactionCategory(tx.id, categoryId);
    } catch (error) {
      toast.error(translateApiError(error as Error, tc));
      throw error;
    }
    setChanges((current) => new Map(current).set(tx.id, categoryId));
    await refreshMoneyFlows(queryClient);
  };

  const flowNote = (tx: ReportTransaction, credit: boolean) => {
    const label = rowFlowLabel(tx, credit, tx.categoryId != null);
    if (!label) return null;
    return label === 'refund' ? tBank('readiness.flowRefund') : tBank(`readiness.flow.${label}`);
  };

  return (
    <Dialog open={!!target} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[880px]">
        <DialogHeader>
          <DialogTitle>{target?.name}</DialogTitle>
          <DialogDescription>
            {fmt.day(start)} – {fmt.day(end)} ·{' '}
            {t('readiness.drilldownUnits', { currency: reportCurrency })}
          </DialogDescription>
        </DialogHeader>
        {isError ? (
          <ReportError onRetry={() => void refetch()} />
        ) : isLoading || snapshot === null ? (
          <p>{t('readiness.loading')}</p>
        ) : (
          <div className="max-h-[60vh] overflow-y-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('readiness.date')}</TableHead>
                  <TableHead>{t('readiness.description')}</TableHead>
                  <TableHead>{t('readiness.category')}</TableHead>
                  <TableHead className="text-right">{t('readiness.native')}</TableHead>
                  <TableHead className="text-right">
                    {t('readiness.contribution', { currency: reportCurrency })}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.slice(page * PAGE, (page + 1) * PAGE).map((tx) => {
                  const credit = tx.txType === 'credit';
                  const category = categoryOf(tx);
                  const isMoved = changes.has(tx.id) && isMovedOut(target?.categoryId, category);
                  const note = isMoved ? t('readiness.moved') : flowNote(tx, credit);
                  return (
                    <TableRow key={tx.id} className={cn(isMoved && 'opacity-50')}>
                      <TableCell className="whitespace-nowrap">{fmt.day(tx.bookingDate)}</TableCell>
                      <TableCell>
                        {tx.description || tx.counterpartyName || '—'}
                        <small className="block text-micro text-ink-4">
                          {tx.bankAccountName}
                          {note && ` · ${note}`}
                        </small>
                      </TableCell>
                      <TableCell>
                        <CategorySelector
                          currentCategoryId={category}
                          counterpartyName={tx.counterpartyName}
                          counterpartyIban={tx.counterpartyIban}
                          counterpartyBankCode={tx.counterpartyBankCode}
                          counterpartyCountry={tx.counterpartyCountry}
                          bankAccountId={tx.bankAccountId}
                          isCredit={credit}
                          categories={categories}
                          onCategoryChange={(categoryId) => changeCategory(tx, categoryId)}
                          rowLabel={tx.description || tx.counterpartyName || undefined}
                          compact
                        />
                      </TableCell>
                      <TableCell className="text-right num">
                        {formatReportMoney(
                          fmt,
                          Number(tx.amount) * (credit ? 1 : -1),
                          tx.currency,
                          { signed: true }
                        )}
                      </TableCell>
                      <TableCell className="text-right num">
                        {tx.signedAmount === null
                          ? t('readiness.fxUnavailable')
                          : formatReportMoney(fmt, Number(tx.signedAmount), tx.reportCurrency, {
                              signed: true,
                            })}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        )}
        <DialogFooter>
          <span className="mr-auto text-micro text-ink-4">
            {t('readiness.countedTransactions', { count: rows.length })}
            {moved > 0 && ` · ${t('readiness.movedCount', { count: moved })}`}
          </span>
          <Button variant="ghost" disabled={!page} onClick={() => setPage(page - 1)}>
            {t('readiness.previous')}
          </Button>
          <Button
            variant="ghost"
            disabled={(page + 1) * PAGE >= rows.length}
            onClick={() => setPage(page + 1)}
          >
            {t('readiness.next')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ReportTransactionsDialog(
  props: Parameters<typeof ReportTransactionsDialogContent>[0]
) {
  return (
    <ReportTransactionsDialogContent
      key={JSON.stringify([
        props.target,
        props.start,
        props.end,
        props.accountIds,
        props.reportCurrency,
      ])}
      {...props}
    />
  );
}
