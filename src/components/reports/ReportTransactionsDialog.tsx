import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import type { FlowKind } from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
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
  categoryId?: string;
  sourceKey?: string;
  expectedCount?: number;
}
function ReportTransactionsDialogContent({
  target,
  start,
  end,
  accountIds,
  onClose,
}: {
  target: ReportTarget | null;
  start: number;
  end: number;
  accountIds?: string[];
  onClose: () => void;
}) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const [page, setPage] = useState(0);
  const {
    data = [],
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ['report-transactions', target, start, end, accountIds],
    queryFn: () =>
      cashflowApi.getTransactions(start, end, {
        accountIds,
        flowKind: target?.flowKind,
        categoryId: target?.categoryId,
        sourceKey: target?.sourceKey,
      }),
    enabled: target != null,
  });
  return (
    <Dialog open={!!target} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[800px]">
        <DialogHeader>
          <DialogTitle>{target?.name}</DialogTitle>
          <DialogDescription>
            {fmt.day(start)} – {fmt.day(end)} · {t('readiness.drilldownUnits')}
          </DialogDescription>
        </DialogHeader>
        {isError ? (
          <ReportError onRetry={() => void refetch()} />
        ) : isLoading ? (
          <p>{t('readiness.loading')}</p>
        ) : (
          <div className="max-h-[60vh] overflow-y-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('readiness.date')}</TableHead>
                  <TableHead>{t('readiness.description')}</TableHead>
                  <TableHead>{t('readiness.account')}</TableHead>
                  <TableHead className="text-right">{t('readiness.native')}</TableHead>
                  <TableHead className="text-right">{t('readiness.contribution')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {data.slice(page * 100, (page + 1) * 100).map((tx) => (
                  <TableRow key={tx.id}>
                    <TableCell>{fmt.day(tx.bookingDate)}</TableCell>
                    <TableCell>
                      {tx.description || tx.counterpartyName || '—'}
                      <small className="block text-micro text-ink-4">
                        {t(`readiness.flow.${tx.effectiveFlowKind}`)} ·{' '}
                        {t(`readiness.source.${tx.flowSource}`)}
                      </small>
                    </TableCell>
                    <TableCell>{tx.bankAccountName}</TableCell>
                    <TableCell className="text-right num">
                      {fmt.money(
                        Number(tx.amount) * (tx.txType === 'credit' ? 1 : -1),
                        tx.currency,
                        { signed: true }
                      )}
                    </TableCell>
                    <TableCell className="text-right num">
                      {tx.signedAmountCzk === null
                        ? t('readiness.fxUnavailable')
                        : fmt.money(Number(tx.signedAmountCzk), 'CZK', { signed: true })}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
        <DialogFooter>
          <span className="mr-auto text-micro text-ink-4">
            {t('readiness.countedTransactions', { count: data.length })}
          </span>
          <Button variant="ghost" disabled={!page} onClick={() => setPage(page - 1)}>
            {t('readiness.previous')}
          </Button>
          <Button
            variant="ghost"
            disabled={(page + 1) * 100 >= data.length}
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
      key={JSON.stringify([props.target, props.start, props.end, props.accountIds])}
      {...props}
    />
  );
}
