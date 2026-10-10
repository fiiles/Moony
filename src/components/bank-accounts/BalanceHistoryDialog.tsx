import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Trash2 } from 'lucide-react';
import type { BankBalanceRecord } from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { useBalanceRecordMutations } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

interface BalanceHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  currency: string;
  /** Newest first (as the API returns them). */
  records: readonly BankBalanceRecord[];
}

/** Every dated balance of the account with its change against the previous one. */
export function BalanceHistoryDialog({
  open,
  onOpenChange,
  accountId,
  currency,
  records,
}: BalanceHistoryDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { deleteRecord } = useBalanceRecordMutations(accountId);
  const [pending, setPending] = useState<BankBalanceRecord | null>(null);
  const money = (v: number, signed = false) => fmt.money(v, currency, { signed, decimals: 0 });

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('balanceHistory.title')}</DialogTitle>
            <DialogDescription>
              {records.length
                ? t('balanceHistory.sub', { count: records.length, date: fmt.day(records[0].asOf) })
                : t('balanceHistory.empty')}
            </DialogDescription>
          </DialogHeader>
          {records.length > 0 && (
            <div className="-mx-[27px] max-h-[420px] overflow-y-auto border-t border-line">
              <Table>
                <TableHeader className="sticky top-0 z-[1] bg-well">
                  <TableRow>
                    <TableHead>{tc('labels.date')}</TableHead>
                    <TableHead className="text-right">{t('balanceHistory.amount')}</TableHead>
                    <TableHead className="text-right">{t('balanceHistory.change')}</TableHead>
                    <TableHead>{t('balanceHistory.source')}</TableHead>
                    <TableHead className="w-10">
                      <span className="sr-only">{tc('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {records.map((r, i) => {
                    const prev = records[i + 1];
                    const delta = prev ? Number(r.amount) - Number(prev.amount) : null;
                    return (
                      <TableRow key={r.id} className="h-[46px]">
                        <TableCell>{fmt.day(r.asOf)}</TableCell>
                        <TableCell className="text-right font-650 text-ink num">
                          {money(Number(r.amount))}
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right num',
                            delta != null && delta > 0 && 'text-gain',
                            delta != null && delta < 0 && 'text-loss'
                          )}
                        >
                          {delta == null ? '—' : money(delta, true)}
                        </TableCell>
                        <TableCell>
                          <Badge variant="outline">
                            {t(
                              r.source === 'statement'
                                ? 'balanceHistory.sourceStatement'
                                : 'balanceHistory.sourceManual'
                            )}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-right">
                          <div data-row-actions>
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={tc('labels.moreActions')}
                                >
                                  <Ellipsis />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem variant="danger" onSelect={() => setPending(r)}>
                                  <Trash2 />
                                  {t('balanceHistory.delete')}
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDeleteDialog
        open={pending !== null}
        onOpenChange={(o) => {
          if (!o) setPending(null);
        }}
        title={t('balanceHistory.deleteTitle')}
        description={
          pending
            ? t('balanceHistory.deleteDescription', {
                amount: money(Number(pending.amount)),
                date: fmt.day(pending.asOf),
              })
            : ''
        }
        onConfirm={() =>
          pending && deleteRecord.mutate(pending.id, { onSuccess: () => setPending(null) })
        }
        isPending={deleteRecord.isPending}
        confirmLabel={t('balanceHistory.delete')}
      />
    </>
  );
}
