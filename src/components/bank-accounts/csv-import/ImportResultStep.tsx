import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ChevronDown } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { formatRowMessage } from '@/utils/csv-import-messages';
import type { BankAccount, CsvImportResult } from '@shared/schema';
import { cn } from '@/lib/utils';
import { formatBalance } from './import-config';
import { NextSteps } from './NextSteps';

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

interface ImportResultStepProps {
  accountId: string;
  result: CsvImportResult;
  account: BankAccount | null | undefined;
  locale: string;
  fileName: string;
  /** Rows the user categorized by hand in the preview. */
  manualCount: number;
  onShowImported?: () => void;
  onReviewUncategorized?: () => void;
  onAutoCategorize?: () => void | Promise<void>;
}

/**
 * Step 4 (prototype csv-import.html): what happened to the file on the left —
 * the three tiles and the categorization sentence — and what to do next on the
 * right.
 */
export function ImportResultStep({
  accountId,
  result,
  account,
  locale,
  fileName,
  manualCount,
  onShowImported,
  onReviewUncategorized,
  onAutoCategorize,
}: ImportResultStepProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  const skippedDuplicates = useMemo(
    () => result.skippedDuplicates.map((message) => formatRowMessage(message, t, tc)),
    [result.skippedDuplicates, t, tc]
  );
  const errors = useMemo(
    () => result.errors.map((message) => formatRowMessage(message, t, tc)),
    [result.errors, t, tc]
  );
  const manual = Math.min(manualCount, result.importedCount);
  const rules = Math.max(0, result.importedCount - result.uncategorizedCount - manual);

  return (
    <div className="grid grid-cols-2 gap-6">
      <div>
        <div className="mb-3.5 grid size-11 place-items-center rounded-full bg-gain-soft text-gain">
          <Check className="size-[22px]" aria-hidden />
        </div>
        <h3 className="m-0 text-[18px] font-650 tracking-[-0.03em] text-ink">
          {t('csvImport.done.title')}
        </h3>
        <p className="mb-0 mt-1.5 text-caption text-ink-3">
          {result.dateRange
            ? t('csvImport.done.summary', {
                count: result.importedCount,
                from: fmt.day(result.dateRange.from),
                to: fmt.day(result.dateRange.to),
              })
            : t('csvImport.done.nothing')}{' '}
          {result.lastBalance != null && result.dateRange && (
            <>
              {t('csvImport.done.balance', {
                date: fmt.day(result.dateRange.to),
                balance: '',
              }).replace(/\s*\.\s*$/, '')}{' '}
              <b className="font-650 text-ink">
                {formatBalance(result.lastBalance, account?.currency, locale)}
              </b>
              .
            </>
          )}
        </p>
        <div className="my-4 grid grid-cols-3 gap-2.5">
          <Tile value={result.importedCount} label={t('csvImport.done.imported')} />
          <Tile value={skippedDuplicates.length} label={t('csvImport.done.duplicates')} />
          <Tile value={result.errorCount} label={t('csvImport.done.errors')} tone="loss" />
        </div>
        {result.importedCount > 0 && (
          <p className="m-0 text-micro font-500 text-ink-4">
            {t('csvImport.done.categorization', {
              rules,
              manual,
              open: result.uncategorizedCount,
            })}{' '}
            {manual > 0 && t('csvImport.done.learnedNote')}
          </p>
        )}
        {(result.skippedZero > 0 || result.skippedFiltered > 0 || result.duplicates.length > 0) && (
          <ul className="m-0 mt-3 list-none p-0 text-micro font-500 text-ink-4">
            {result.duplicates.length > 0 && (
              <li>{t('csvImport.forcedDuplicatesNote', { count: result.duplicates.length })}</li>
            )}
            {result.skippedZero > 0 && (
              <li>{t('csvImport.skippedZeroNote', { count: result.skippedZero })}</li>
            )}
            {result.skippedFiltered > 0 && (
              <li>{t('csvImport.skippedFilteredNote', { count: result.skippedFiltered })}</li>
            )}
          </ul>
        )}
        {skippedDuplicates.length > 0 && (
          <MessageList
            trigger={t('csvImport.showDuplicateDetails')}
            messages={skippedDuplicates}
            tone="muted"
          />
        )}
        {errors.length > 0 && (
          <MessageList trigger={t('csvImport.showErrorDetails')} messages={errors} tone="loss" />
        )}
      </div>
      <NextSteps
        accountId={accountId}
        result={result}
        account={account}
        locale={locale}
        fileName={fileName}
        onShowImported={onShowImported}
        onReviewUncategorized={onReviewUncategorized}
        onAutoCategorize={onAutoCategorize}
      />
    </div>
  );
}
