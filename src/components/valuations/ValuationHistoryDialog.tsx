import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Trash2 } from 'lucide-react';
import type { AssetValuation } from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
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
import { useValuationMutations, type ValuationKind } from '@/hooks/use-valuation-mutations';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

interface ValuationHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: ValuationKind;
  assetId: string;
  /** Oldest first (as the API returns them). */
  valuations: readonly AssetValuation[];
}

/**
 * Every dated estimate of a manually priced asset, newest first, with its change against the
 * previous one (the counterpart of the bank account's balance history). The change is left out
 * where the currency changed between two estimates: native values are never compared across
 * currencies.
 */
export function ValuationHistoryDialog({
  open,
  onOpenChange,
  kind,
  assetId,
  valuations,
}: ValuationHistoryDialogProps) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const { remove } = useValuationMutations(kind, assetId);
  const [pending, setPending] = useState<AssetValuation | null>(null);
  const rows = useMemo(() => [...valuations].reverse(), [valuations]);
  const money = (v: number, currency: string, signed = false) =>
    fmt.money(v, currency, { signed, decimals: 0 });

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('valuationHistory.title')}</DialogTitle>
            <DialogDescription>
              {rows.length
                ? t('valuationHistory.sub', { count: rows.length, date: fmt.day(rows[0].valuedAt) })
                : t('valuationHistory.empty')}
            </DialogDescription>
          </DialogHeader>
          {rows.length > 0 && (
            <div className="-mx-[27px]">
              <Table containerClassName="max-h-[420px] overflow-y-auto">
                {/* The head's hairline is an inset shadow on the cells: a collapsed border would scroll away with the rows. */}
                <TableHeader className="sticky top-0 z-[1] bg-well [&_th]:shadow-[inset_0_-1px_0_var(--line)] [&_tr]:border-b-0">
                  <TableRow>
                    <TableHead>{t('labels.date')}</TableHead>
                    <TableHead className="text-right">{t('valuationHistory.value')}</TableHead>
                    <TableHead className="text-right">{t('valuationHistory.change')}</TableHead>
                    <TableHead>{t('revalue.note')}</TableHead>
                    <TableHead className="w-10">
                      <span className="sr-only">{t('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((v, i) => {
                    const prev = rows[i + 1];
                    const delta =
                      prev && prev.currency === v.currency
                        ? Number(v.value) - Number(prev.value)
                        : null;
                    return (
                      <TableRow key={v.id} className="h-[46px]">
                        <TableCell>{fmt.day(v.valuedAt)}</TableCell>
                        <TableCell className="text-right font-650 text-ink num">
                          {money(Number(v.value), v.currency)}
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right num',
                            delta != null && delta > 0 && 'text-gain',
                            delta != null && delta < 0 && 'text-loss'
                          )}
                        >
                          {delta == null ? '—' : money(delta, v.currency, true)}
                        </TableCell>
                        <TableCell
                          className="max-w-[220px] truncate text-ink-3"
                          title={v.note ?? ''}
                        >
                          {v.note || '—'}
                        </TableCell>
                        <TableCell className="text-right">
                          <div data-row-actions>
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={t('labels.moreActions')}
                                >
                                  <Ellipsis />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem variant="danger" onSelect={() => setPending(v)}>
                                  <Trash2 />
                                  {t('valuationHistory.delete')}
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
              {t('buttons.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDeleteDialog
        open={pending !== null}
        onOpenChange={(o) => {
          if (!o) setPending(null);
        }}
        title={t('valuationHistory.deleteTitle')}
        description={
          pending
            ? t('valuationHistory.deleteDescription', {
                amount: money(Number(pending.value), pending.currency),
                date: fmt.day(pending.valuedAt),
              })
            : ''
        }
        onConfirm={() =>
          pending && remove.mutate(pending.id, { onSuccess: () => setPending(null) })
        }
        isPending={remove.isPending}
        confirmLabel={t('valuationHistory.delete')}
      />
    </>
  );
}
