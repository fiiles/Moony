import { useTranslation } from 'react-i18next';
import { Undo2 } from 'lucide-react';
import type { StockImportBatch } from '@shared/schema';
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
import { useFormat } from '@/lib/use-format';
import { useStockImportFormats } from './import/use-stock-import-data';
import { useSourceLabel } from './import/use-source-label';

interface StockImportHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  batches: readonly StockImportBatch[];
  /** Opens the page's delete confirmation for one batch. */
  onDelete: (batch: StockImportBatch) => void;
}

/**
 * Every recorded stock import, CSV and MCP alike (date, file, source, trades
 * still there), with the delete action. Mirrors the bank account's
 * `ImportHistoryDialog`; an import whose trades were all deleted since keeps
 * only its record to delete.
 */
export function StockImportHistoryDialog({
  open,
  onOpenChange,
  batches,
  onDelete,
}: StockImportHistoryDialogProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { data: formats = [] } = useStockImportFormats(open);
  const sourceLabel = useSourceLabel(formats);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[calc(100vw-48px)] max-w-[960px]">
        <DialogHeader>
          <DialogTitle>{t('importHistory.title')}</DialogTitle>
          <DialogDescription>
            {batches.length > 0 ? t('importHistory.description') : t('importHistory.empty')}
          </DialogDescription>
        </DialogHeader>
        {batches.length > 0 && (
          // min-w-0: in a narrow window (the phone) the table scrolls sideways instead of widening the dialog past its edge
          <div className="-mx-[27px] min-w-0">
            <Table containerClassName="max-h-[420px] overflow-y-auto">
              {/* The head's hairline is an inset shadow on the cells: a collapsed border would scroll away with the rows. */}
              <TableHeader className="sticky top-0 z-[1] bg-well [&_th]:shadow-[inset_0_-1px_0_var(--line)] [&_tr]:border-b-0">
                <TableRow>
                  <TableHead>{tc('labels.date')}</TableHead>
                  <TableHead>{t('importHistory.file')}</TableHead>
                  <TableHead>{t('importHistory.source')}</TableHead>
                  <TableHead className="text-right">{t('importHistory.trades')}</TableHead>
                  <TableHead>
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {batches.map((batch) => (
                  <TableRow key={batch.id} className="h-[50px]">
                    <TableCell className="whitespace-nowrap">
                      {fmt.dateTime(batch.createdAt)}
                    </TableCell>
                    <TableCell
                      className="max-w-[240px] truncate font-650 text-ink"
                      title={batch.fileName}
                    >
                      {batch.fileName}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">{sourceLabel(batch.source)}</TableCell>
                    <TableCell className="whitespace-nowrap text-right num">
                      {batch.remainingCount < batch.tradeCount
                        ? t('importHistory.remainingOf', {
                            remaining: batch.remainingCount,
                            total: batch.tradeCount,
                          })
                        : batch.tradeCount}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      <div data-row-actions>
                        <Button variant="danger" size="sm" onClick={() => onDelete(batch)}>
                          <Undo2 />
                          {t(
                            batch.remainingCount > 0
                              ? 'importHistory.deleteImported'
                              : 'importHistory.deleteRecord'
                          )}
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
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
  );
}
