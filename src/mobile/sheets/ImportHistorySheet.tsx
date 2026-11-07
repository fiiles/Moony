import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Undo2 } from 'lucide-react';
import type { CsvImportBatch } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { Sheet } from '@/mobile/components/Sheet';

interface ImportHistorySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  batches: readonly CsvImportBatch[];
  /** Asks to take the batch back; the page closes this sheet and confirms. */
  onUndo: (batch: CsvImportBatch) => void;
}

/**
 * "Historie importů" from the detail's `···` (spec §8.4): the statements imported into the account,
 * newest first — the file, the period it covered (else when it was imported), the rows it added and
 * the duplicates it skipped. A tap or a long press on a row opens its menu, whose one item takes the
 * import back (the page confirms it, as the desktop does).
 */
export function ImportHistorySheet({
  open,
  onOpenChange,
  batches,
  onUndo,
}: ImportHistorySheetProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
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
            <ImportRow key={batch.id} batch={batch} onUndo={onUndo} />
          ))}
        </EntityRows>
      )}
    </Sheet>
  );
}

/** One statement: its file, period and counts, and the menu that takes it back. */
function ImportRow({
  batch,
  onUndo,
}: {
  batch: CsvImportBatch;
  onUndo: (batch: CsvImportBatch) => void;
}) {
  const { t } = useTranslation('bank_accounts');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const [menuOpen, setMenuOpen] = useState(false);

  return (
    <EntityRow
      title={batch.fileName}
      sub={
        batch.dateRange
          ? `${fmt.day(batch.dateRange.from)} – ${fmt.day(batch.dateRange.to ?? batch.dateRange.from)}`
          : fmt.dateTime(batch.importedAt)
      }
      value={fmt.number(batch.importedCount)}
      valueSub={
        batch.duplicateCount
          ? `${t('importHistory.duplicates')} ${fmt.number(batch.duplicateCount)}`
          : undefined
      }
      onClick={() => setMenuOpen(true)}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={tm('account.importHistory.menu')}
              className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
            >
              <Ellipsis aria-hidden className="size-5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[220px]">
            <RowMenuItem variant="danger" onSelect={() => onUndo(batch)}>
              <Undo2 strokeWidth={1.75} aria-hidden />
              {t(batch.linkedCount > 0 ? 'readiness.deleteImported' : 'readiness.deleteRecord')}
            </RowMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    />
  );
}
