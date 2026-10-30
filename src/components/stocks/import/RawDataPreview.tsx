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
import { cn } from '@/lib/utils';
import type { StockCsvInspection } from '@shared/schema';
import type { ColumnOption, MappingRole } from './import-config';

interface RawDataPreviewProps {
  inspection: Pick<StockCsvInspection, 'headers' | 'sampleRows' | 'rowCount'>;
  /** Names of the columns (blank headers already labelled). */
  options: readonly ColumnOption[];
  /** The columns the mapping reads, with what each is read as. */
  mapped: ReadonlyMap<number, readonly MappingRole[]>;
}

/**
 * The first rows of the file exactly as they are written, behind a toggle. The
 * columns the mapping reads are shaded and named, so a wrong pick shows at a glance.
 */
export function RawDataPreview({ inspection, options, mapped }: RawDataPreviewProps) {
  const { t } = useTranslation('stocks');
  const [open, setOpen] = useState(false);

  return (
    <div className="rounded-r2 border border-line">
      <button
        type="button"
        className="flex w-full items-center justify-between px-3 py-2.5 text-left text-table font-600 text-ink-2 hover:bg-well"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <span>{t('importWizard.mapping.rawData')}</span>
        <span className="flex items-center gap-2 text-micro font-500 text-ink-4">
          {t('importWizard.mapping.rawCounts', {
            columns: inspection.headers.length,
            rows: inspection.rowCount,
          })}
          <ChevronDown
            className={cn('size-3.5 transition-transform duration-fast', open && 'rotate-180')}
            aria-hidden
          />
        </span>
      </button>
      {open && (
        <div className="max-h-56 overflow-x-auto border-t border-line">
          <Table>
            <TableHeader>
              <TableRow>
                {options.map((option) => {
                  const roles = mapped.get(option.index);
                  return (
                    <TableHead
                      key={option.index}
                      className={cn('whitespace-nowrap align-top', roles && 'bg-well-2')}
                    >
                      <span className="block">{option.label}</span>
                      {roles && (
                        <span className="mt-0.5 block text-micro font-600 normal-case tracking-normal text-ink-4">
                          {roles.map((role) => t(`importWizard.roles.${role}`)).join(' · ')}
                        </span>
                      )}
                    </TableHead>
                  );
                })}
              </TableRow>
            </TableHeader>
            <TableBody>
              {inspection.sampleRows.slice(0, 5).map((row, i) => (
                <TableRow key={i} className="h-9">
                  {options.map((option) => (
                    <TableCell
                      key={option.index}
                      className={cn(
                        'max-w-[200px] truncate whitespace-nowrap',
                        mapped.has(option.index) && 'bg-well/60 font-500 text-ink'
                      )}
                    >
                      {row[option.index] ?? ''}
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
