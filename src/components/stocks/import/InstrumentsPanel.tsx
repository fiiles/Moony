import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Check, Loader2, TriangleAlert } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { cn } from '@/lib/utils';
import { exchangeName } from '@/utils/exchange-names';
import type {
  StockImportInstrument,
  StockInstrumentOverride,
  StockInstrumentResolution,
} from '@shared/schema';
import { InstrumentEditor, type InstrumentEdit } from './InstrumentEditor';
import { instrumentCheck, needsLookup, type InstrumentCheck } from './import-config';
import type { LookupProgress } from './instrument-lookup';

interface InstrumentsPanelProps {
  instruments: readonly StockImportInstrument[];
  overrides: readonly StockInstrumentOverride[];
  resolutions: Readonly<Record<string, StockInstrumentResolution>>;
  progress: LookupProgress;
  isResolving: boolean;
  /** Stop waiting for Yahoo Finance; what is left stays unverified. */
  onSkipVerification: () => void;
  /** Look the instruments again that Yahoo Finance did not answer for. */
  onRetryVerification: () => void;
  /** Exclude (or include again) every trade of an instrument. */
  onToggleSkip: (instrument: StockImportInstrument, skip: boolean) => void;
  onEdit: (instrument: StockImportInstrument, edit: InstrumentEdit) => void;
  /** The table is for an older mapping and is being replaced. */
  isUpdating: boolean;
}

/**
 * "Cenné papíry" (spec §3, step 3): one row per security of the file with its
 * state — an existing position, a new one confirmed on Yahoo Finance, one that
 * could not be confirmed, or one that still needs a symbol. Resolution runs by
 * itself when the step opens; symbol, name and currency can be edited inline
 * and an instrument can be skipped as a whole.
 */
export function InstrumentsPanel({
  instruments,
  overrides,
  resolutions,
  progress,
  isResolving,
  onSkipVerification,
  onRetryVerification,
  onToggleSkip,
  onEdit,
  isUpdating,
}: InstrumentsPanelProps) {
  const { t } = useTranslation('stocks');
  const [editingKey, setEditingKey] = useState<string | null>(null);

  if (instruments.length === 0) return null;

  const checks = new Map(
    instruments.map((instrument) => [
      instrument.key,
      instrumentCheck(instrument, resolutions[instrument.key]),
    ])
  );
  const missing = instruments.filter((i) => i.status === 'missingSymbol').length;
  // Instruments Yahoo Finance did not answer for (offline, rate limit): worth asking again.
  const answerless = instruments.filter(
    (i) => needsLookup(i) && checks.get(i.key)?.state === 'failed'
  ).length;
  const unverified = instruments.filter((i) => {
    const state = checks.get(i.key)?.state;
    return (
      i.status === 'new' && (state === 'failed' || state === 'notFound' || state === 'unmatched')
    );
  }).length;

  return (
    <section className={cn('transition-opacity duration-fast', isUpdating && 'opacity-60')}>
      <div className="mb-2 flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
        <h4 className="m-0 text-caption font-700 text-ink-2">
          {t('importWizard.review.instruments.title')}
          <span className="ml-1.5 font-500 text-ink-4">{instruments.length}</span>
        </h4>
        {isResolving && (
          <p className="m-0 flex items-center gap-2 text-micro font-500 text-ink-3" role="status">
            <Loader2 className="size-3 animate-spin" aria-hidden />
            {t('importWizard.review.instruments.progress', {
              done: progress.done,
              total: progress.total,
            })}
            <button
              type="button"
              className="font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink"
              onClick={onSkipVerification}
            >
              {t('importWizard.review.instruments.skipVerification')}
            </button>
          </p>
        )}
      </div>

      <div className="overflow-hidden rounded-r3 border border-line">
        <div className="max-h-[340px] overflow-y-auto">
          <Table className="border-t-0">
            <TableHeader>
              <TableRow>
                <TableHead>{t('importWizard.review.instruments.security')}</TableHead>
                <TableHead className="w-[84px]">
                  {t('importWizard.review.instruments.currency')}
                </TableHead>
                <TableHead className="w-[84px] text-right">
                  {t('importWizard.review.instruments.trades')}
                </TableHead>
                <TableHead className="w-[34%]">
                  {t('importWizard.review.instruments.status')}
                </TableHead>
                <TableHead className="w-[170px]">
                  <span className="sr-only">{t('importWizard.review.instruments.actions')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {instruments.map((instrument) => {
                const resolution = resolutions[instrument.key];
                const override = overrides.find((o) => o.key === instrument.key);
                const skipped = instrument.status === 'skipped' || override?.skip === true;
                const name = override?.name?.trim() || instrument.name?.trim() || '';
                const editing = editingKey === instrument.key;
                return (
                  <InstrumentRows
                    key={instrument.key}
                    instrument={instrument}
                    name={name}
                    check={checks.get(instrument.key) ?? { state: 'pending' }}
                    isResolving={isResolving}
                    skipped={skipped}
                    editing={editing}
                    candidates={resolution?.candidates ?? []}
                    onEdit={() => setEditingKey(instrument.key)}
                    onCancelEdit={() => setEditingKey(null)}
                    onApply={(edit) => {
                      onEdit(instrument, edit);
                      setEditingKey(null);
                    }}
                    onToggleSkip={() => onToggleSkip(instrument, !skipped)}
                  />
                );
              })}
            </TableBody>
          </Table>
        </div>
      </div>

      {missing > 0 && (
        <p className="mb-0 mt-2 flex items-start gap-2 text-caption font-600 text-loss">
          <TriangleAlert className="mt-px size-3.5 shrink-0" aria-hidden />
          {t('importWizard.review.instruments.missingHint', { count: missing })}
        </p>
      )}
      {(unverified > 0 || answerless > 0) && !isResolving && (
        <p className="mb-0 mt-2 flex flex-wrap items-baseline gap-x-3 text-micro font-500 text-ink-4">
          {unverified > 0 && (
            <span>
              {t('importWizard.review.instruments.unverifiedHint', { count: unverified })}
            </span>
          )}
          {answerless > 0 && (
            <button
              type="button"
              className="font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink"
              onClick={onRetryVerification}
            >
              {t('importWizard.review.instruments.retry')}
            </button>
          )}
        </p>
      )}
    </section>
  );
}

interface InstrumentRowsProps {
  instrument: StockImportInstrument;
  name: string;
  check: InstrumentCheck;
  isResolving: boolean;
  skipped: boolean;
  editing: boolean;
  candidates: StockInstrumentResolution['candidates'];
  onEdit: () => void;
  onCancelEdit: () => void;
  onApply: (edit: InstrumentEdit) => void;
  onToggleSkip: () => void;
}

/** The row of one instrument and, while it is edited, the editor under it. */
function InstrumentRows({
  instrument,
  name,
  check,
  isResolving,
  skipped,
  editing,
  candidates,
  onEdit,
  onCancelEdit,
  onApply,
  onToggleSkip,
}: InstrumentRowsProps) {
  const { t } = useTranslation('stocks');

  const primary = instrument.ticker ?? instrument.symbol ?? instrument.isin ?? '—';
  const fileSymbol =
    instrument.symbol && instrument.symbol.toUpperCase() !== primary.toUpperCase()
      ? t('importWizard.review.instruments.inFile', { symbol: instrument.symbol })
      : '';
  const isin = instrument.isin && instrument.isin !== primary ? instrument.isin : '';
  const secondary = [name, fileSymbol, isin].filter(Boolean).join(' · ');

  const waiting = isResolving && check.state === 'pending';
  const { badge, detail } = instrumentState(instrument, check, skipped, waiting, t);

  return (
    <>
      <TableRow className={cn(skipped && 'text-ink-4')}>
        <TableCell className="max-w-0">
          <b className={cn('block truncate font-650 num', skipped ? 'text-ink-4' : 'text-ink')}>
            {primary}
          </b>
          {secondary && (
            <span className="block truncate text-micro font-500 text-ink-4" title={secondary}>
              {secondary}
            </span>
          )}
        </TableCell>
        <TableCell className="num">{instrument.currency ?? '—'}</TableCell>
        <TableCell className="text-right num">{instrument.tradeCount}</TableCell>
        <TableCell>
          <div className="flex flex-col items-start gap-1">
            {badge}
            {detail && <span className="text-micro font-500 text-ink-4">{detail}</span>}
            {instrument.positionCurrency && !skipped && (
              <span className="text-micro font-600 text-loss">
                {t('importWizard.review.instruments.positionCurrency', {
                  currency: instrument.positionCurrency,
                })}
              </span>
            )}
          </div>
        </TableCell>
        <TableCell>
          <div className="flex justify-end gap-1">
            {!skipped && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onEdit}
                disabled={editing}
                aria-label={t('importWizard.review.instruments.editFor', { name: primary })}
              >
                {t('importWizard.review.instruments.edit')}
              </Button>
            )}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={onToggleSkip}
              aria-label={t(
                skipped
                  ? 'importWizard.review.instruments.includeFor'
                  : 'importWizard.review.instruments.skipFor',
                { name: primary }
              )}
            >
              {skipped
                ? t('importWizard.review.instruments.include')
                : t('importWizard.review.instruments.skip')}
            </Button>
          </div>
        </TableCell>
      </TableRow>
      {editing && (
        <TableRow className="h-auto hover:bg-transparent">
          <TableCell colSpan={5} className="bg-well/50 py-3">
            <InstrumentEditor
              instrument={instrument}
              name={name}
              candidates={candidates}
              onApply={onApply}
              onCancel={onCancelEdit}
            />
          </TableCell>
        </TableRow>
      )}
    </>
  );
}

/** The badge and the line under it for the state of an instrument. */
function instrumentState(
  instrument: StockImportInstrument,
  check: InstrumentCheck,
  skipped: boolean,
  waiting: boolean,
  t: TFunction
): { badge: ReactNode; detail: ReactNode } {
  if (skipped) {
    return {
      badge: <Badge variant="outline">{t('importWizard.review.instruments.state.skipped')}</Badge>,
      detail: null,
    };
  }

  const checking = (
    <span className="flex items-center gap-1.5 text-micro font-500 text-ink-4">
      <Loader2 className="size-3 animate-spin" aria-hidden />
      {t('importWizard.review.instruments.checking')}
    </span>
  );

  if (instrument.status === 'existing') {
    return {
      badge: <Badge>{t('importWizard.review.instruments.state.existing')}</Badge>,
      detail: null,
    };
  }

  if (instrument.status === 'missingSymbol') {
    const why =
      check.state === 'failed' || check.state === 'notFound' ? check.state : ('enter' as const);
    return {
      badge: (
        <Badge variant="loss">{t('importWizard.review.instruments.state.missingSymbol')}</Badge>
      ),
      detail: waiting ? checking : t(`importWizard.review.instruments.missingReason.${why}`),
    };
  }

  // A new position.
  if (check.state === 'verified') {
    const { exchange, currency } = check.candidate;
    return {
      badge: (
        <Badge>
          <Check aria-hidden />
          {t('importWizard.review.instruments.state.new')}
        </Badge>
      ),
      detail: [exchangeName(exchange), currency].filter(Boolean).join(' · '),
    };
  }
  if (waiting || check.state === 'pending') {
    return {
      badge: <Badge>{t('importWizard.review.instruments.state.new')}</Badge>,
      detail: checking,
    };
  }
  return {
    badge: <Badge variant="outline">{t('importWizard.review.instruments.state.unverified')}</Badge>,
    detail: t(`importWizard.review.instruments.reason.${check.state}`),
  };
}
