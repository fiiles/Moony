import { useMemo, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Check, ChevronDown } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { useCurrency } from '@/lib/currency';
import { formatRowMessage } from '@/utils/csv-import-messages';
import type { BankAccount, CsvImportResult } from '@shared/schema';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { formatBalance, type ImportKind } from './import-config';
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
  kind: ImportKind;
  /** The account the dialog was opened for (the page behind it). */
  accountId: string;
  /**
   * The account the import wrote to; `NextSteps` acts on it. Another account than `accountId`
   * when the user switched the target to the statement's own account (D9).
   */
  importedAccountId: string;
  result: CsvImportResult;
  /** The account the import wrote to. */
  account: BankAccount | null | undefined;
  locale: string;
  fileName: string;
  /** Rows the user categorized by hand in the preview. */
  manualCount: number;
  onShowImported?: () => void;
  onReviewUncategorized?: () => void;
  onAutoCategorize?: () => void | Promise<void>;
  /** Closes the dialog (after "Open {account}" navigated away). */
  onClose: () => void;
}

/**
 * Step 4 (prototype csv-import.html): what happened to the file on the left —
 * the three tiles and the categorization sentence — and what to do next on the
 * right.
 */
export function ImportResultStep({
  kind,
  accountId,
  importedAccountId,
  result,
  account,
  locale,
  fileName,
  manualCount,
  onShowImported,
  onReviewUncategorized,
  onAutoCategorize,
  onClose,
}: ImportResultStepProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  // The page behind the dialog shows another account than the one imported into.
  const otherAccount = importedAccountId !== accountId;

  const skippedDuplicates = useMemo(
    () => result.skippedDuplicates.map((message) => formatRowMessage(message, t, tc, kind)),
    [result.skippedDuplicates, t, tc, kind]
  );
  const errors = useMemo(
    () => result.errors.map((message) => formatRowMessage(message, t, tc, kind)),
    [result.errors, t, tc, kind]
  );
  const categorized = result.importedCount - result.uncategorizedCount;

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
          {result.statementBalance != null && (
            <Trans
              t={t}
              i18nKey="csvImport.done.statementBalance"
              values={{
                date: fmt.day(result.statementBalance.asOf),
                balance: formatBalance(
                  result.statementBalance.amount,
                  result.statementBalance.currency,
                  locale,
                  currencyCode
                ),
              }}
              components={{ b: <b className="font-650 text-ink" /> }}
            />
          )}
        </p>
        {otherAccount && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="mt-3"
            onClick={() => {
              setLocation(`/bank-accounts/${importedAccountId}`);
              onClose();
            }}
          >
            {t('statementImport.openAccount', { account: account?.name ?? '' })}
          </Button>
        )}
        <div className="my-4 grid grid-cols-4 gap-2.5">
          <Tile value={result.promotedCount} label={t('readiness.promoted')} />
          <Tile value={result.importedCount} label={t('csvImport.done.imported')} />
          <Tile value={skippedDuplicates.length} label={t('csvImport.done.duplicates')} />
          <Tile value={result.errorCount} label={t('csvImport.done.errors')} tone="loss" />
        </div>
        {result.importedCount > 0 && (
          <p className="m-0 text-micro font-500 text-ink-4">
            {t('readiness.resultCategorized', {
              count: categorized,
              open: result.uncategorizedCount,
            })}{' '}
            {manualCount > 0 && t('readiness.oneTimeChoices')}
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
        accountId={importedAccountId}
        result={result}
        account={account}
        locale={locale}
        fileName={fileName}
        // The page's callbacks act on the page's account: "Open {account}" replaces them.
        onShowImported={otherAccount ? undefined : onShowImported}
        onReviewUncategorized={otherAccount ? undefined : onReviewUncategorized}
        onAutoCategorize={otherAccount ? undefined : onAutoCategorize}
      />
    </div>
  );
}
