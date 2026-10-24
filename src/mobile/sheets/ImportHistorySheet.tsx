import { useTranslation } from 'react-i18next';
import type { CsvImportBatch } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { Sheet } from '@/mobile/components/Sheet';

interface ImportHistorySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  batches: readonly CsvImportBatch[];
}

/**
 * "Historie importů" from the detail's `···` (spec §8.4): the statements imported into the account,
 * newest first — the file, the period it covered (else when it was imported), the rows it added and
 * the duplicates it skipped. Read-only on the phone: undoing an import stays on the desktop.
 */
export function ImportHistorySheet({ open, onOpenChange, batches }: ImportHistorySheetProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const newestFirst = [...batches].sort((a, b) => b.importedAt - a.importedAt);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('importHistory.title')}
      description={newestFirst.length === 0 ? t('importHistory.empty') : undefined}
      footer={
        <Button
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      {newestFirst.length > 0 && (
        <EntityRows className="mb-0">
          {newestFirst.map((batch) => (
            <EntityRow
              key={batch.id}
              title={batch.fileName}
              sub={
                batch.dateRange
                  ? `${fmt.day(batch.dateRange.from)} – ${fmt.day(batch.dateRange.to)}`
                  : fmt.dateTime(batch.importedAt)
              }
              value={fmt.number(batch.importedCount)}
              valueSub={
                batch.duplicateCount
                  ? `${t('importHistory.duplicates')} ${fmt.number(batch.duplicateCount)}`
                  : undefined
              }
            />
          ))}
        </EntityRows>
      )}
    </Sheet>
  );
}
