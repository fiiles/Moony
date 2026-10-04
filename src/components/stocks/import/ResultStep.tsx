import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ChevronDown, ChevronRight, Loader2, Undo2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLanguage } from '@/i18n/I18nProvider';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { CsvDateRange, StockImportResult, StockImportUndoResult } from '@shared/schema';
import { groupResultMessages, type RowStatusByLine } from './import-config';
import { rowMessageLine } from './row-messages';

/** Tickers named in a sentence before "+N". */
const TICKERS_SHOWN = 8;

function Tile({ value, label, tone }: { value: number; label: string; tone?: 'loss' }) {
  return (
    <div className="rounded-r3 border border-line bg-paper px-[13px] py-3">
      <small className="block text-micro font-600 text-ink-3">{label}</small>
      <b
        className={cn(
          'mt-[5px] block text-[20px] font-700 tracking-[-0.04em] text-ink num',
          tone === 'loss' && value > 0 && 'text-loss'
        )}
      >
        {value}
      </b>
    </div>
  );
}

function MessageList({
  trigger,
  messages,
  tone,
}: {
  trigger: string;
  messages: string[];
  tone: 'muted' | 'loss';
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mt-3">
      <button
        type="button"
        className={cn(
          'flex items-center gap-1.5 text-caption font-600 underline-offset-[3px] hover:underline',
          tone === 'loss' ? 'text-loss' : 'text-ink-3'
        )}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        {trigger}
        <ChevronDown
          className={cn('size-3.5 transition-transform duration-fast', open && 'rotate-180')}
          aria-hidden
        />
      </button>
      {open && (
        <ul className="mt-2 max-h-40 list-none overflow-y-auto rounded-r2 bg-well px-3 py-2 text-micro text-ink-3 num">
          {messages.map((message, i) => (
            <li key={i} className="py-0.5">
              {message}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

interface NextStepRow {
  id: string;
  label: string;
  hint: string;
  action: string;
  danger?: boolean;
  busy?: boolean;
  onClick: () => void;
}

interface ResultStepProps {
  result: StockImportResult;
  fileName: string;
  /** Days of the trades that were imported (from the review). */
  dateRange: CsvDateRange | null;
  /** What the review said about each row it listed, to sort the messages by. */
  reviewed: RowStatusByLine;
  /** The history of portfolio value is being rebuilt in the background. */
  recalculating: boolean;
  /** Set once the import was undone from here. */
  undone: StockImportUndoResult | null;
  isUndoing: boolean;
  onShowStocks: () => void;
  onOpenAnalysis: () => void;
  onImportAnother: () => void;
  /** Ask whether to undo (the dialog confirms and does it). */
  onUndo: () => void;
}

/**
 * Step 4 (spec §3): what happened to the file on the left — the tiles, the new
 * and the extended positions, the lists of duplicates, skipped rows and errors
 * with their file lines — and what to do next on the right, undo included.
 */
export function ResultStep({
  result,
  fileName,
  dateRange,
  reviewed,
  recalculating,
  undone,
  isUndoing,
  onShowStocks,
  onOpenAnalysis,
  onImportAnother,
  onUndo,
}: ResultStepProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { getLocale } = useLanguage();
  const fmt = useFormat();
  const locale = getLocale();

  const groups = useMemo(
    () => groupResultMessages(result.messages, reviewed),
    [result.messages, reviewed]
  );
  const lines = useMemo(() => {
    const context = { t, tc, locale };
    return {
      duplicates: groups.duplicates.map((m) => rowMessageLine(m, context)),
      skipped: groups.skipped.map((m) => rowMessageLine(m, context)),
      errors: groups.errors.map((m) => rowMessageLine(m, context)),
    };
  }, [groups, t, tc, locale]);

  const tickers = (list: readonly string[]) => {
    const shown = list.slice(0, TICKERS_SHOWN).join(', ');
    const more = list.length - TICKERS_SHOWN;
    return more > 0 ? `${shown} ${t('importWizard.result.moreTickers', { count: more })}` : shown;
  };

  if (undone) {
    const rows: NextStepRow[] = [
      {
        id: 'another',
        label: t('importWizard.result.next.another'),
        hint: t('importWizard.result.next.anotherHint'),
        action: t('importWizard.result.next.anotherAction'),
        onClick: onImportAnother,
      },
      {
        id: 'show',
        label: t('importWizard.result.next.show'),
        hint: t('importWizard.result.next.showHint'),
        action: t('importWizard.result.next.showAction'),
        onClick: onShowStocks,
      },
    ];
    return (
      <div className="grid grid-cols-2 gap-6">
        <div>
          <div className="mb-3.5 grid size-11 place-items-center rounded-full bg-well-2 text-ink-2">
            <Undo2 className="size-[22px]" aria-hidden />
          </div>
          <h3 className="m-0 text-[18px] font-650 tracking-[-0.03em] text-ink">
            {t('importWizard.undone.title')}
          </h3>
          <p className="mb-0 mt-1.5 text-caption text-ink-3">
            {t('importWizard.undone.summary', { count: undone.removed, file: fileName })}
          </p>
          {undone.removedPositions.length > 0 && (
            <p className="mb-0 mt-2.5 text-caption text-ink-3">
              <b className="font-650 text-ink">{t('importWizard.undone.removedPositions')}</b>{' '}
              {tickers(undone.removedPositions)}
            </p>
          )}
        </div>
        <NextSteps rows={rows} />
      </div>
    );
  }

  const imported = result.imported > 0;
  const rows: NextStepRow[] = [
    {
      id: 'show',
      label: t('importWizard.result.next.show'),
      hint: t('importWizard.result.next.showHint'),
      action: t('importWizard.result.next.showAction'),
      onClick: onShowStocks,
    },
    ...(result.newPositions.length > 0
      ? [
          {
            id: 'tags',
            label: t('importWizard.result.next.tags'),
            hint: t('importWizard.result.next.tagsHint'),
            action: t('importWizard.result.next.tagsAction'),
            onClick: onOpenAnalysis,
          },
        ]
      : []),
    {
      id: 'another',
      label: t('importWizard.result.next.another'),
      hint: t('importWizard.result.next.anotherHint'),
      action: t('importWizard.result.next.anotherAction'),
      onClick: onImportAnother,
    },
    ...(result.batchId
      ? [
          {
            id: 'undo',
            label: t('importWizard.result.next.undo'),
            hint: t('importWizard.result.next.undoHint', { count: result.imported }),
            action: t('importWizard.result.next.undoAction'),
            danger: true,
            busy: isUndoing,
            onClick: onUndo,
          },
        ]
      : []),
  ];

  return (
    <div className="grid grid-cols-2 gap-6">
      <div>
        <div
          className={cn(
            'mb-3.5 grid size-11 place-items-center rounded-full',
            imported ? 'bg-gain-soft text-gain' : 'bg-well-2 text-ink-3'
          )}
        >
          <Check className="size-[22px]" aria-hidden />
        </div>
        <h3 className="m-0 text-[18px] font-650 tracking-[-0.03em] text-ink">
          {imported ? t('importWizard.result.title') : t('importWizard.result.titleNothing')}
        </h3>
        <p className="mb-0 mt-1.5 text-caption text-ink-3">
          {!imported
            ? t('importWizard.result.nothing')
            : dateRange
              ? t('importWizard.result.summary', {
                  count: result.imported,
                  from: fmt.day(dateRange.from),
                  to: fmt.day(dateRange.to),
                })
              : t('importWizard.result.summaryCount', { count: result.imported })}
        </p>

        <div className="my-4 grid grid-cols-4 gap-2.5">
          <Tile value={result.imported} label={t('importWizard.result.imported')} />
          <Tile value={result.duplicates} label={t('importWizard.result.duplicates')} />
          <Tile value={result.skipped} label={t('importWizard.result.skipped')} />
          <Tile value={result.errors} label={t('importWizard.result.errors')} tone="loss" />
        </div>

        {(result.newPositions.length > 0 || result.updatedPositions.length > 0) && (
          <ul className="m-0 mb-1 grid list-none gap-1 p-0 text-caption text-ink-3">
            {result.newPositions.length > 0 && (
              <li>
                <b className="font-650 text-ink">{t('importWizard.result.newPositions')}</b>{' '}
                {tickers(result.newPositions)}
              </li>
            )}
            {result.updatedPositions.length > 0 && (
              <li>
                <b className="font-650 text-ink">{t('importWizard.result.updatedPositions')}</b>{' '}
                {tickers(result.updatedPositions)}
              </li>
            )}
          </ul>
        )}

        {imported && recalculating && (
          <p
            className="mb-0 mt-3 flex items-center gap-2 text-micro font-500 text-ink-4"
            role="status"
          >
            <Loader2 className="size-3 animate-spin" aria-hidden />
            {t('importWizard.result.recalculating')}
          </p>
        )}

        {lines.duplicates.length > 0 && (
          <MessageList
            trigger={t('importWizard.result.showDuplicates')}
            messages={lines.duplicates}
            tone="muted"
          />
        )}
        {lines.skipped.length > 0 && (
          <MessageList
            trigger={t('importWizard.result.showSkipped')}
            messages={lines.skipped}
            tone="muted"
          />
        )}
        {lines.errors.length > 0 && (
          <MessageList
            trigger={t('importWizard.result.showErrors')}
            messages={lines.errors}
            tone="loss"
          />
        )}
      </div>
      <NextSteps rows={rows} />
    </div>
  );
}

/** What to do after an import, as the prototype's "Co dál" list. */
function NextSteps({ rows }: { rows: readonly NextStepRow[] }) {
  const { t } = useTranslation('stocks');
  return (
    <div>
      <h4 className="mb-1.5 mt-0 text-caption font-700 text-ink-2">
        {t('importWizard.result.whatNext')}
      </h4>
      <ul className="m-0 list-none p-0">
        {rows.map((row) => (
          <li
            key={row.id}
            className="flex items-center gap-3 border-b border-line-soft py-[11px] text-table last:border-0"
          >
            <ChevronRight className="size-[15px] shrink-0 text-ink-4" aria-hidden />
            <span className="min-w-0 flex-1">
              <b className="block font-600 text-ink">{row.label}</b>
              <small className="mt-px block text-micro text-ink-4">{row.hint}</small>
            </span>
            <Button
              type="button"
              size="sm"
              variant={row.danger ? 'danger' : 'outline'}
              loading={row.busy}
              disabled={row.busy}
              onClick={row.onClick}
            >
              {row.action}
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
