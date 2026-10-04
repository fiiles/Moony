import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import type { CsvPreviewResult } from '@shared/schema';
import { cn } from '@/lib/utils';

/** The first rows of the file exactly as they are written, behind a toggle. */
export function RawDataPreview({ inspection }: { inspection: CsvPreviewResult }) {
  const { t } = useTranslation('bank_accounts');
  const [open, setOpen] = useState(false);

  return (
    <div className="rounded-r2 border border-line">
      <button
        type="button"
        className="flex w-full items-center justify-between px-3 py-2.5 text-left text-table font-600 text-ink-2 hover:bg-well"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <span>{t('csvImport.previewData')}</span>
        <span className="flex items-center gap-2 text-micro font-500 text-ink-4">
          {t('csvImport.columns')}: {inspection.headers.length} · {t('csvImport.rows')}:{' '}
          {inspection.totalRows}
          <ChevronDown
            className={cn('size-3.5 transition-transform duration-fast', open && 'rotate-180')}
            aria-hidden
          />
        </span>
      </button>
      {open && (
        <div className="max-h-48 overflow-x-auto border-t border-line">
          <Table>
            <TableHeader>
              <TableRow>
                {inspection.headers.map((header, i) => (
                  <TableHead key={i} className="whitespace-nowrap">
                    {header}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {inspection.sampleRows.slice(0, 5).map((row, i) => (
                <TableRow key={i} className="h-9">
                  {row.map((cell, j) => (
                    <TableCell key={j} className="max-w-[200px] truncate whitespace-nowrap">
                      {cell}
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
