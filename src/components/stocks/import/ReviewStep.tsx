import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CircleAlert, RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { useLanguage } from '@/i18n/I18nProvider';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type {
  StockImportInstrument,
  StockImportPreview,
  StockInstrumentOverride,
  StockInstrumentResolution,
  StockPreviewRow,
} from '@shared/schema';
import { translateApiError } from '@/lib/translate-api-error';
import type { InstrumentEdit } from './InstrumentEditor';
import { InstrumentsPanel } from './InstrumentsPanel';
import { PREVIEW_COLLAPSED_ROWS, formatDecimalText, summarizePreview } from './import-config';
import { rowMessageText } from './row-messages';
import type { LookupProgress } from './instrument-lookup';

interface ReviewStepProps {
  /** The mapping is complete, so there is something to review. */
  ready: boolean;
  preview: StockImportPreview | undefined;
  error: Error | null;
  /** The table shown is for an older mapping and is being replaced. */
  isUpdating: boolean;
  isLoadingFirst: boolean;
  importAnywayLines: readonly number[];
  onToggleImportAnyway: (line: number, checked: boolean) => void;
  overrides: readonly StockInstrumentOverride[];
  resolutions: Readonly<Record<string, StockInstrumentResolution>>;
  progress: LookupProgress;
  isResolving: boolean;
  onSkipVerification: () => void;
  /** The currency of the trades comes from the listing, so it can be edited. */
  currencyEditable: boolean;
  onToggleSkip: (instrument: StockImportInstrument, skip: boolean) => void;
  onEditInstrument: (instrument: StockImportInstrument, edit: InstrumentEdit) => void;
}

/** `symbol:AAPL` / `isin:US…` without the prefix, for a row whose ticker is not known. */
function keyLabel(key: string | null): string {
  return key ? key.replace(/^(symbol|isin):/, '') : '—';
}

/**
 * Step 3 (spec §3): the summary strip, the securities with their state, then
 * the rows with theirs — new, duplicate (importable anyway), skipped with the
 * reason, or an error with a translated message. The first twelve rows,
 * "Zobrazit vše" for the rest.
 */
export function ReviewStep({
  ready,
  preview,
  error,
  isUpdating,
  isLoadingFirst,
  importAnywayLines,
  onToggleImportAnyway,
  overrides,
  resolutions,
  progress,
  isResolving,
  onSkipVerification,
  currencyEditable,
  onToggleSkip,
  onEditInstrument,
}: ReviewStepProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { getLocale } = useLanguage();
  const locale = getLocale();
  const fmt = useFormat();
  const [showAll, setShowAll] = useState(false);
  const forced = useMemo(() => new Set(importAnywayLines), [importAnywayLines]);

  if (!ready) {
    return (
      <p className="py-8 text-center text-table text-ink-3">
        {t('importWizard.review.incomplete')}
      </p>
    );
  }
  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-r3 bg-loss-soft px-4 py-3 text-table text-loss">
        <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
        <div>
          <p className="m-0 font-650">{t('importWizard.review.failed')}</p>
          <p className="m-0">{translateApiError(error, tc)}</p>
        </div>
      </div>
    );
  }
  if (isLoadingFirst || !preview) {
    return (
      <p className="flex items-center justify-center gap-2 py-8 text-table text-ink-3">
        <RefreshCw className="size-3.5 animate-spin" aria-hidden />
        {t('importWizard.review.loading')}
      </p>
    );
  }

  const summary = summarizePreview(preview, importAnywayLines);
  const rows = showAll ? preview.rows : preview.rows.slice(0, PREVIEW_COLLAPSED_ROWS);
  const context = { t, tc, locale };

  const statusCell = (row: StockPreviewRow) => {
    // A duplicate the user ticked stays tickable: the backend may already call it new.
    if (row.status === 'duplicate' || (row.status === 'new' && forced.has(row.line))) {
      return (
        <label className="flex cursor-pointer items-center gap-2 text-micro font-500 text-ink-3">
          <Checkbox
            checked={forced.has(row.line)}
            onCheckedChange={(checked) => onToggleImportAnyway(row.line, checked === true)}
          />
          {t('importWizard.review.duplicateImportAnyway')}
        </label>
      );
    }
    const message = row.message ? rowMessageText(row.message, context) : null;
    switch (row.status) {
      case 'new':
        return <Badge variant="gain">{t('importWizard.review.statusNew')}</Badge>;
      case 'error':
        return (
          <span className="flex flex-col items-start gap-1">
            <Badge variant="loss">{t('importWizard.review.statusError')}</Badge>
            {message && <span className="text-micro text-ink-4">{message}</span>}
          </span>
        );
      default:
        return (
          <span className="flex flex-col items-start gap-1">
            <Badge variant="outline">{t('importWizard.review.statusSkipped')}</Badge>
            {message && <span className="text-micro text-ink-4">{message}</span>}
          </span>
        );
    }
  };

  return (
    <div>
      <div
        className={cn(
          'mb-4 flex flex-wrap items-center gap-x-[22px] gap-y-2 rounded-r3 bg-well px-3.5 py-3 text-table text-ink-2 transition-opacity duration-fast',
          isUpdating && 'opacity-60'
        )}
      >
        <span>
          <b className="font-650 text-ink">
            {t('importWizard.review.summary.willImport', { count: summary.willImport })}
          </b>
        </span>
        <span>{t('importWizard.review.summary.duplicates', { count: summary.duplicates })}</span>
        <span>{t('importWizard.review.summary.skipped', { count: summary.skipped })}</span>
        <span className={cn(summary.errors > 0 && 'font-600 text-loss')}>
          {t('importWizard.review.summary.errors', { count: summary.errors })}
        </span>
        {preview.dateRange && (
          <span className="text-ink-4">
            {fmt.day(preview.dateRange.from)} – {fmt.day(preview.dateRange.to)}
          </span>
        )}
        {preview.hasFeeColumn && (
          <span className="ml-auto text-micro font-600 text-ink-3">
            {t('importWizard.review.feeNote')}
          </span>
        )}
      </div>

      {summary.willImport === 0 && (
        <p className="mb-4 mt-0 text-caption font-600 text-ink-3">
          {t('importWizard.review.nothingToImport')}
        </p>
      )}

      <InstrumentsPanel
        instruments={preview.instruments}
        overrides={overrides}
        resolutions={resolutions}
        progress={progress}
        isResolving={isResolving}
        onSkipVerification={onSkipVerification}
        currencyEditable={currencyEditable}
        onToggleSkip={onToggleSkip}
        onEdit={onEditInstrument}
        isUpdating={isUpdating}
      />

      <section className={cn('mt-5 transition-opacity duration-fast', isUpdating && 'opacity-60')}>
        <h4 className="mb-2 mt-0 text-caption font-700 text-ink-2">
          {t('importWizard.review.rows.title')}
        </h4>
        {preview.rows.length === 0 ? (
          <p className="py-8 text-center text-table text-ink-3">
            {t('importWizard.review.rows.none')}
          </p>
        ) : (
          <div className="overflow-hidden rounded-r3 border border-line">
            <Table className="border-t-0">
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">{t('importWizard.review.rows.line')}</TableHead>
                  <TableHead className="w-[92px]">{t('importWizard.review.rows.date')}</TableHead>
                  <TableHead className="w-[84px]">
                    {t('importWizard.review.rows.direction')}
                  </TableHead>
                  <TableHead>{t('importWizard.review.rows.symbol')}</TableHead>
                  <TableHead className="text-right">
                    {t('importWizard.review.rows.quantity')}
                  </TableHead>
                  <TableHead className="text-right">
                    {t('importWizard.review.rows.price')}
                  </TableHead>
                  <TableHead className="w-[64px]">
                    {t('importWizard.review.rows.currency')}
                  </TableHead>
                  <TableHead className="w-[26%]">{t('importWizard.review.rows.status')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((row) => {
                  const muted = row.status === 'duplicate' && !forced.has(row.line);
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
                        {row.day != null ? fmt.day(row.day) : '—'}
                      </TableCell>
                      <TableCell>
                        {row.direction ? (
                          <Badge variant={row.direction === 'sell' ? 'outline' : 'dark'}>
                            {t(`importWizard.direction.${row.direction}`)}
                          </Badge>
                        ) : (
                          '—'
                        )}
                      </TableCell>
                      <TableCell className="max-w-0">
                        <span
                          className={cn('block truncate font-650 num', !muted && 'text-ink')}
                          title={row.ticker ?? keyLabel(row.instrumentKey)}
                        >
                          {row.ticker ?? keyLabel(row.instrumentKey)}
                        </span>
                      </TableCell>
                      <TableCell className="whitespace-nowrap text-right num">
                        {formatDecimalText(row.quantity, locale)}
                      </TableCell>
                      <TableCell className="whitespace-nowrap text-right num">
                        {formatDecimalText(row.price, locale, 2)}
                      </TableCell>
                      <TableCell className="num">{row.currency ?? '—'}</TableCell>
                      <TableCell>{statusCell(row)}</TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
            <div className="flex items-center justify-between gap-6 border-t border-line px-3.5 py-2.5 text-micro font-500 text-ink-4">
              <span>
                {rows.length >= summary.total
                  ? t('importWizard.review.rows.footerAll', { count: summary.total })
                  : t('importWizard.review.rows.footer', {
                      shown: rows.length,
                      total: summary.total,
                    })}
              </span>
              {preview.rows.length > PREVIEW_COLLAPSED_ROWS && (
                <button
                  type="button"
                  className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline"
                  onClick={() => setShowAll((value) => !value)}
                >
                  {showAll
                    ? t('importWizard.review.rows.showLess')
                    : t('importWizard.review.rows.showAll')}
                </button>
              )}
            </div>
          </div>
        )}
        {summary.total > preview.rows.length && (
          <p className="mb-0 mt-2 text-micro font-500 text-ink-4">
            {t('importWizard.review.rows.truncated', { shown: preview.rows.length })}
          </p>
        )}
      </section>
    </div>
  );
}
