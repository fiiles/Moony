import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { Trash } from 'lucide-react';
import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import type { OtherAsset, OtherAssetTransaction } from '@shared/schema';
import { toast } from 'sonner';
import { otherAssetsApi, portfolioApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { formatAmountWithCode } from '@/utils/format-amount';
import { useFormat } from '@/lib/use-format';

interface Props {
  asset: OtherAsset;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function OtherAssetTransactionsModal({ asset, open, onOpenChange }: Props) {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  // Transaction awaiting delete confirmation (null = dialog closed)
  const [pendingDelete, setPendingDelete] = useState<OtherAssetTransaction | null>(null);
  const { data: transactions, isLoading } = useQuery<OtherAssetTransaction[]>({
    queryKey: ['other-asset-transactions', asset.id],
    queryFn: () => otherAssetsApi.getTransactions(asset.id),
    enabled: open,
  });

  const deleteMutation = useMutation({
    mutationFn: (txId: string) => otherAssetsApi.deleteTransaction(txId),
    onSuccess: async () => {
      setPendingDelete(null);
      queryClient.invalidateQueries({ queryKey: ['other-asset-transactions', asset.id] });
      queryClient.invalidateQueries({ queryKey: ['other-assets'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // The delete already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
    },
    onError: () => toast.error(tc('status.error')),
  });

  const pendingDeleteDescription = pendingDelete
    ? t('confirmDeleteTransaction.description', {
        type: (pendingDelete.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell')).toLowerCase(),
        quantity: fmt.number(Number(pendingDelete.quantity), { maximumFractionDigits: 8 }),
        price: formatAmountWithCode(
          parseFloat(pendingDelete.pricePerUnit),
          pendingDelete.currency,
          fmt.locale
        ),
        date: fmt.day(pendingDelete.transactionDate),
      })
    : '';

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>
              {t('modal.transactions.title')} - {asset.name}
            </DialogTitle>
          </DialogHeader>

          <div className="space-y-4">
            <div className="border rounded-r1">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{tc('labels.date')}</TableHead>
                    <TableHead>{tc('labels.type')}</TableHead>
                    <TableHead>{tc('labels.quantity')}</TableHead>
                    <TableHead>{tc('labels.price')}</TableHead>
                    <TableHead>{tc('labels.total')}</TableHead>
                    <TableHead></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {isLoading ? (
                    <TableRow>
                      <TableCell colSpan={6}>{tc('status.loading')}</TableCell>
                    </TableRow>
                  ) : transactions?.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={6} className="text-center text-ink-3">
                        {t('modal.transactions.noTransactions')}
                      </TableCell>
                    </TableRow>
                  ) : (
                    transactions?.map((tx) => (
                      <TableRow key={tx.id}>
                        <TableCell>{fmt.day(tx.transactionDate)}</TableCell>
                        <TableCell
                          className={
                            tx.type === 'buy'
                              ? 'text-green-600 font-medium'
                              : 'text-red-600 font-medium'
                          }
                        >
                          {tx.type.toUpperCase()}
                        </TableCell>
                        <TableCell>
                          {fmt.number(parseFloat(tx.quantity), { maximumFractionDigits: 8 })}
                        </TableCell>
                        <TableCell>{fmt.money(parseFloat(tx.pricePerUnit), tx.currency)}</TableCell>
                        <TableCell>
                          {fmt.money(
                            parseFloat(tx.quantity) * parseFloat(tx.pricePerUnit),
                            tx.currency
                          )}
                        </TableCell>
                        <TableCell>
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={() => setPendingDelete(tx)}
                            title={tc('buttons.delete')}
                            aria-label={tc('buttons.delete')}
                          >
                            <Trash className="h-4 w-4 text-ink-3 hover:text-loss" />
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))
                  )}
                </TableBody>
              </Table>
            </div>
          </div>
        </DialogContent>
      </Dialog>

      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(isOpen) => {
          if (!isOpen) setPendingDelete(null);
        }}
        title={t('confirmDeleteTransaction.title')}
        description={pendingDeleteDescription}
        onConfirm={() => {
          if (pendingDelete) deleteMutation.mutate(pendingDelete.id);
        }}
        isPending={deleteMutation.isPending}
      />
    </>
  );
}
