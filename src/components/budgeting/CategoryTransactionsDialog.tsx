import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { budgetingApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';

export interface CategoryTransactionsTarget {
  /** Category id, or `uncategorized`. */
  categoryId: string;
  name: string;
}

interface CategoryTransactionsDialogProps {
  target: CategoryTransactionsTarget | null;
  onClose: () => void;
  start: number;
  end: number;
  periodLabel: string;
}

/**
 * The transactions behind one budget row for the shown period — the drill-down
 * the old bar chart offered on click, now a modal from the row name or "···".
 */
export function CategoryTransactionsDialog({
  target,
  onClose,
  start,
  end,
  periodLabel,
}: CategoryTransactionsDialogProps) {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  const { data: transactions = [], isLoading } = useQuery({
    queryKey: ['budgeting-category-transactions', target?.categoryId, start, end],
    queryFn: () => budgetingApi.getCategoryTransactions(target!.categoryId, start, end),
    enabled: !!target,
  });

  const signed = (amount: string, txType: string) => {
    const n = Math.abs(parseFloat(amount) || 0);
    return txType.toLowerCase() === 'credit' ? n : -n;
  };

  return (
    <Dialog open={!!target} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[720px]">
        <DialogHeader>
          <DialogTitle>{target?.name}</DialogTitle>
          <DialogDescription>
            {t('transactions.description', { period: periodLabel })}
          </DialogDescription>
        </DialogHeader>
        <div className="-mx-[27px] -mb-4 max-h-[60vh] overflow-y-auto">
          {isLoading ? (
            <div className="space-y-2 px-[27px] py-4">
              <Skeleton className="h-9" />
              <Skeleton className="h-9" />
              <Skeleton className="h-9" />
            </div>
          ) : transactions.length === 0 ? (
            <p className="px-[27px] py-10 text-center text-table text-ink-3">
              {t('transactions.empty')}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[110px] pl-[27px]">{tc('labels.date')}</TableHead>
                  <TableHead>{tc('labels.description')}</TableHead>
                  <TableHead>{t('transactions.account')}</TableHead>
                  <TableHead className="pr-[27px] text-right">{tc('labels.amount')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {transactions.map((tx) => {
                  const value = signed(tx.amount, tx.txType);
                  return (
                    <TableRow key={tx.id} className="h-[46px]">
                      <TableCell className="pl-[27px] text-ink-3 num">
                        {fmt.day(tx.bookingDate, { day: 'numeric', month: 'numeric' })}
                      </TableCell>
                      <TableCell>
                        <span className="block truncate font-600 text-ink">
                          {tx.description || tx.counterpartyName || t('transactions.noDescription')}
                        </span>
                        {tx.description && tx.counterpartyName && (
                          <small className="block truncate text-micro font-500 text-ink-4">
                            {tx.counterpartyName}
                          </small>
                        )}
                      </TableCell>
                      <TableCell className="text-ink-3">{tx.bankAccountName}</TableCell>
                      <TableCell
                        className={cn(
                          'pr-[27px] text-right font-650 num',
                          value > 0 ? 'text-gain' : 'text-ink'
                        )}
                      >
                        {fmt.money(value, tx.currency || 'CZK', { signed: true })}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </div>
        <DialogFooter>
          <span className="text-micro font-500 text-ink-4">
            {t('transactions.count', { count: transactions.length })}
          </span>
          <Button variant="outline" onClick={onClose}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
