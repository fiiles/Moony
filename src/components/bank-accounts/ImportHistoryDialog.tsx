import { useTranslation } from 'react-i18next';
import { Undo2 } from 'lucide-react';
import type { CsvImportBatch } from '@shared/schema';
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

interface ImportHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  batches: readonly CsvImportBatch[];
  /** Opens the page's undo confirmation for one batch. */
  onUndo: (batch: CsvImportBatch) => void;
}

/** Every CSV import of the account (date, file, counts) with the undo action; wide enough for the longest undo label. */
export function ImportHistoryDialog({
  open,
  onOpenChange,
  batches,
  onUndo,
}: ImportHistoryDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const newest = batches.reduce<number | null>(
    (max, b) => (max === null || b.importedAt > max ? b.importedAt : max),
    null
  );
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[calc(100vw-48px)] max-w-[1120px]">
        <DialogHeader>
          <DialogTitle>{t('importHistory.title')}</DialogTitle>
          <DialogDescription>
            {newest !== null
              ? t('status.lastImport', { date: fmt.day(newest) })
              : t('importHistory.empty')}
          </DialogDescription>
        </DialogHeader>
        {batches.length > 0 && (
          <div className="-mx-[27px]">
            <Table containerClassName="max-h-[420px] overflow-y-auto">
              {/* The head's hairline is an inset shadow on the cells: a collapsed border would scroll away with the rows. */}
              <TableHeader className="sticky top-0 z-[1] bg-well [&_th]:shadow-[inset_0_-1px_0_var(--line)] [&_tr]:border-b-0">
                <TableRow>
                  <TableHead>{tc('labels.date')}</TableHead>
                  <TableHead>{t('importHistory.fileName')}</TableHead>
                  <TableHead>{t('importHistory.period')}</TableHead>
                  <TableHead className="text-right">{t('importHistory.imported')}</TableHead>
                  <TableHead className="text-right" title={t('importHistory.duplicatesHelp')}>
                    {t('importHistory.duplicates')}
                  </TableHead>
                  <TableHead className="text-right">{t('importHistory.errors')}</TableHead>
                  <TableHead>
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {batches.map((batch) => (
                  <TableRow key={batch.id} className="h-[50px]">
                    <TableCell className="whitespace-nowrap">
                      {fmt.dateTime(batch.importedAt)}
                    </TableCell>
                    <TableCell
                      className="max-w-[220px] truncate font-650 text-ink"
                      title={batch.fileName}
                    >
                      {batch.fileName}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {batch.dateRange
                        ? `${fmt.day(batch.dateRange.from)} – ${fmt.day(batch.dateRange.to ?? batch.dateRange.from)}`
                        : t('readiness.unknownDate')}
                    </TableCell>
                    <TableCell className="text-right num">{batch.importedCount}</TableCell>
                    <TableCell className="text-right num">{batch.duplicateCount}</TableCell>
                    <TableCell className="text-right num">{batch.errorCount}</TableCell>
                    <TableCell className="whitespace-nowrap text-right">
                      <div data-row-actions>
                        <Button variant="danger" size="sm" onClick={() => onUndo(batch)}>
                          <Undo2 />
                          {t(
                            batch.linkedCount
                              ? 'readiness.deleteImported'
                              : 'readiness.deleteRecord'
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
