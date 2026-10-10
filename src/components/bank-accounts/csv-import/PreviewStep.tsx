import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CircleAlert, RefreshCw } from 'lucide-react';
import type {
  CsvCategoryOverride,
  CsvImportPreview,
  CsvPreviewRow,
  TransactionCategory,
} from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useCurrency } from '@/lib/currency';
import { translateApiError } from '@/lib/translate-api-error';
import { translateRowMessage } from '@/utils/csv-import-messages';
import { categoryDisplayName } from '@/utils/category-name';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { CategoryPick } from '@/components/common/CategoryPick';
import { cn } from '@/lib/utils';
import { formatPreviewAmount, summarizePreview } from './import-config';

interface PreviewStepProps {
  page: number;
  onPageChange: (page: number) => void;
  /** Mapping is complete, so there is something to preview. */
  ready: boolean;
  preview: CsvImportPreview | undefined;
  error: Error | null;
  /** The table shown is for an older mapping and is being replaced. */
  isUpdating: boolean;
  isLoadingFirst: boolean;
  importAnywayLines: readonly number[];
  onToggleImportAnyway: (line: number, checked: boolean) => void;
  /** Categories the user picked in this preview, by file line. */
  overrides: readonly CsvCategoryOverride[];
  onOverride: (line: number, categoryId: string | null) => void;
  categories: readonly TransactionCategory[];
  /** Currency for rows whose file has no currency column. */
  accountCurrency: string | null | undefined;
  /** Locale for amount formatting, e.g. `cs-CZ`. */
  locale: string;
}

/**
 * Step 3 (prototype csv-import.html): the summary strip, then every row with
 * its parsed date and amount, the category it would get — from the rules, a
 * learned payee, or the user's pick — and its status; duplicates can be
 * imported anyway. The first twelve rows, "Zobrazit vše" for the rest.
 */
export function PreviewStep({
  page,
  onPageChange,
  ready,
  preview,
  error,
  isUpdating,
  isLoadingFirst,
  importAnywayLines,
  onToggleImportAnyway,
  overrides,
  onOverride,
  categories,
  accountCurrency,
  locale,
}: PreviewStepProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const forced = useMemo(() => new Set(importAnywayLines), [importAnywayLines]);
  const overrideByLine = useMemo(
    () => new Map(overrides.map((o) => [o.line, o.categoryId])),
    [overrides]
  );
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  if (!ready) {
    return (
      <p className="py-8 text-center text-table text-ink-3">{t('csvImport.previewIncomplete')}</p>
    );
  }
  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-r3 bg-loss-soft px-4 py-3 text-table text-loss">
        <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
        <div>
          <p className="m-0 font-650">{t('csvImport.previewFailed')}</p>
          <p className="m-0">{translateApiError(error, tc)}</p>
        </div>
      </div>
    );
  }
  if (isLoadingFirst || !preview) {
    return (
      <p className="flex items-center justify-center gap-2 py-8 text-table text-ink-3">
        <RefreshCw className="size-3.5 animate-spin" aria-hidden />
        {t('csvImport.previewLoading')}
      </p>
    );
  }

  const summary = summarizePreview(preview, importAnywayLines);
  const rows = preview.rows;
  const willWrite = (row: CsvPreviewRow) =>
    row.status === 'ok' ||
    row.status === 'promoted' ||
    (row.status === 'duplicate' && row.canImportAnyway && forced.has(row.line));

  const categoryCell = (row: CsvPreviewRow) => {
    if (!willWrite(row)) return <span className="text-ink-4">—</span>;
    const overrideId = overrideByLine.get(row.line);
    const categoryId = overrideId ?? row.categoryId;
    const category = categoryId ? categoryById.get(categoryId) : undefined;
    if (category) {
      const note =
        overrideId !== undefined
          ? t('csvImport.preview.manual')
          : row.categorySource === 'exact_match'
            ? t('csvImport.preview.learned')
            : null;
      return (
        <CategoryPick
          categories={categories}
          selectedId={category.id}
          allowClear={overrideId !== undefined}
          onPick={(id) => onOverride(row.line, id)}
        >
          <button
            type="button"
            className="inline-flex max-w-full items-center gap-1.5 rounded-r1 text-left font-600 text-ink hover:text-ink-2"
          >
            <i
              className="block size-[7px] shrink-0 rounded-full"
              style={{ background: category.color || 'var(--s2)' }}
            />
            <span className="truncate">{categoryDisplayName(category, t)}</span>
            {note && <span className="shrink-0 font-500 text-ink-4">· {note}</span>}
          </button>
        </CategoryPick>
      );
    }
    return (
      <CategoryPick
        categories={categories}
        selectedId={null}
        allowClear={false}
        onPick={(id) => onOverride(row.line, id)}
      >
        <button
          type="button"
          className="inline-flex items-center rounded-[6px] border border-dashed border-line-strong bg-paper px-[7px] py-0.5 text-micro font-600 text-ink-3 hover:border-ink-3 hover:text-ink"
        >
          {t('csvImport.preview.pickCategory')}
        </button>
      </CategoryPick>
    );
  };

  const statusCell = (row: CsvPreviewRow) => {
    const message = row.message ? translateRowMessage(row.message, t, tc) : null;
    switch (row.status) {
      case 'promoted':
        return <Badge variant="gain">{t('csvImport.status.promoted')}</Badge>;
      case 'ok':
        return <Badge variant="gain">{t('csvImport.preview.statusNew')}</Badge>;
      case 'duplicate':
        if (!row.canImportAnyway) return <Badge>{message}</Badge>;
        return (
          <label className="flex cursor-pointer items-center gap-2 text-micro font-500 text-ink-3">
            <Checkbox
              checked={forced.has(row.line)}
              onCheckedChange={(checked) => onToggleImportAnyway(row.line, checked === true)}
            />
            {t('csvImport.preview.duplicateImportAnyway')}
          </label>
        );
      case 'error':
        return (
          <span className="flex flex-col items-start gap-1">
            <Badge variant="loss">{t('csvImport.status.error')}</Badge>
            {message && <span className="text-micro text-ink-4">{message}</span>}
          </span>
        );
      default:
        return (
          <span className="flex flex-col items-start gap-1">
            <Badge variant="outline">{t('csvImport.status.skipped')}</Badge>
            {message && <span className="text-micro text-ink-4">{message}</span>}
          </span>
        );
    }
  };

  return (
    <div className={cn('transition-opacity duration-fast', isUpdating && 'opacity-60')}>
      <div className="mb-3.5 flex flex-wrap items-center gap-x-[22px] gap-y-2 rounded-r3 bg-well px-3.5 py-3 text-table text-ink-2">
        <span>
          <b className="font-650 text-ink">
            {t('csvImport.preview.willImport', { count: summary.willImport })} ·{' '}
            {t('readiness.promotions', { count: summary.willPromote })}
          </b>
        </span>
        <span>
          {t('csvImport.summary.duplicatesSkipped', { count: summary.skippedDuplicates })}
        </span>
        <span className={cn(summary.errors > 0 && 'text-loss')}>
          {t('csvImport.summary.errors', { count: summary.errors })}
        </span>
        {preview.dateRange && (
          <span className="text-ink-4">
            {fmt.day(preview.dateRange.from)} – {fmt.day(preview.dateRange.to)}
          </span>
        )}
        <span className="flex-1" />
        <span className="flex items-center gap-1">
          <Badge variant="gain">
            {t('csvImport.preview.categorized', { count: summary.categorized })}
          </Badge>
          <Badge>{t('csvImport.preview.uncategorized', { count: summary.uncategorized })}</Badge>
        </span>
      </div>

      <div className="flex items-center justify-between gap-6 border border-line mb-3 rounded-r3 px-3.5 py-2.5 text-micro font-500 text-ink-4">
        <span>
          {t('readiness.pageCount', {
            shown: rows.length,
            total: preview.totalRows,
            page: page + 1,
          })}
        </span>
        <div className="flex gap-2">
          <Button
            variant="ghost"
            size="sm"
            disabled={page === 0 || isUpdating}
            onClick={() => onPageChange(page - 1)}
          >
            {t('readiness.previous')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={(page + 1) * 100 >= preview.totalRows || isUpdating}
            onClick={() => onPageChange(page + 1)}
          >
            {t('readiness.next')}
          </Button>
        </div>
      </div>

      {preview.rows.length === 0 ? (
        <p className="py-8 text-center text-table text-ink-3">{t('csvImport.preview.noRows')}</p>
      ) : (
        <div className="max-h-[38vh] overflow-y-auto rounded-r3 border border-line">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-9">{t('csvImport.preview.colLine')}</TableHead>
                <TableHead className="w-[92px]">{t('csvImport.preview.colDate')}</TableHead>
                <TableHead className="w-[110px] text-right">
                  {t('csvImport.preview.colAmount')}
                </TableHead>
                <TableHead>{t('transaction.counterparty')}</TableHead>
                <TableHead>{t('transaction.detail')}</TableHead>
                <TableHead className="w-[20%]">{t('csvImport.preview.colCategory')}</TableHead>
                <TableHead className="w-[19%]">{t('csvImport.preview.colStatus')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => {
                const amount = Number(row.amount);
                const muted = row.status === 'duplicate' && !forced.has(row.line);
                // Same columns as the account ledger
                const text = {
                  counterpartyName: row.counterparty,
                  counterpartyIban: row.counterpartyIban,
                  counterpartyBankCode: row.counterpartyBankCode,
                  description: row.description,
                  remittanceInfo: null,
                };
                const counterparty = counterpartyLabel(text);
                const details = transactionDetails(text).join(' · ');
                return (
                  <TableRow
                    key={row.line}
                    className={cn(
                      'h-11 text-caption',
                      muted && 'text-ink-4',
                      row.status === 'error' && 'bg-loss-soft/40'
                    )}
                  >
                    <TableCell className="text-ink-4 num">{row.line}</TableCell>
                    <TableCell className="whitespace-nowrap num">
                      {row.bookingDate != null ? fmt.day(row.bookingDate) : '—'}
                    </TableCell>
                    <TableCell
                      className={cn(
                        'whitespace-nowrap text-right num',
                        !muted && amount > 0 && 'text-gain',
                        !muted && 'font-600'
                      )}
                    >
                      {formatPreviewAmount(
                        row.amount,
                        row.currency ?? accountCurrency,
                        locale,
                        currencyCode
                      )}
                    </TableCell>
                    <TableCell className="max-w-0">
                      <span className="block truncate" title={counterparty.title ?? undefined}>
                        {counterparty.title ?? <span className="text-ink-4">—</span>}
                      </span>
                      {counterparty.subtitle && (
                        <small
                          className="block truncate text-micro font-500 text-ink-4 num"
                          title={counterparty.subtitle}
                        >
                          {counterparty.subtitle}
                        </small>
                      )}
                    </TableCell>
                    <TableCell className="max-w-0">
                      <span className="block truncate" title={details || undefined}>
                        {details || '—'}
                      </span>
                    </TableCell>
                    <TableCell>{categoryCell(row)}</TableCell>
                    <TableCell>{statusCell(row)}</TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
