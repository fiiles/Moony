import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { CsvPreviewRow } from '@shared/schema';
import { useCategories } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { Button } from '@/components/ui/button';
import { FileBar } from '@/mobile/components/import/FileBar';
import { ImportNote } from '@/mobile/components/import/ImportNote';
import { PreviewRows } from '@/mobile/components/import/PreviewRows';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import type { ImportFlow } from '@/mobile/hooks/use-import-flow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { exactMoneyText } from '@/mobile/lib/import-flow';

interface PreviewBodyProps {
  flow: ImportFlow;
  /** A row that will be written was tapped: the category choice opens for it. */
  onPick: (row: CsvPreviewRow) => void;
}

/**
 * The preview (spec §8.6) keeps the desktop's guarantees on one narrow column: the file bar, what
 * the statement says about itself (whose account it is, whether its balances add up), then the
 * counts, the period and the rows with the category each would get. A statement of another account
 * is imported only after the person switches to that account or says so (D9). A CSV that a format
 * the person confirmed before reads says so first, with "Změnit" to correct it. Amounts are masked
 * while hidden; counts and dates are not.
 */
export function PreviewBody({ flow, onPick }: PreviewBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { categories } = useCategories();
  const { summary, statement, preview, counts, target } = flow;

  const currency = target?.currency ?? '';
  const meta =
    flow.kind === 'statement' && statement
      ? [
          tb(`statementImport.format.${statement.format}`),
          summary?.accountIdentifier ?? summary?.accountName,
        ]
      : [flow.preset?.bankName, 'CSV'];
  const overrides = new Map(flow.overrides.map((o) => [o.line, o.categoryId]));

  return (
    <>
      <FileBar name={flow.fileName} meta={meta.filter(Boolean).join(' · ')} />
      {flow.source === 'learned' && flow.learned && (
        <ImportNote
          actions={
            <Button
              type="button"
              variant="ghost"
              className="h-auto min-h-11 min-w-11 flex-none whitespace-normal rounded-[10px] px-2.5 py-2 text-m-sub font-semibold text-ink"
              disabled={flow.stage === 'importing'}
              onClick={() => void flow.changeLearned()}
            >
              {t('import.learned.change')}
            </Button>
          }
        >
          {t('import.learned.note', { name: flow.learned.name })}
        </ImportNote>
      )}
      {flow.kind === 'statement' && <StatementNotes flow={flow} />}

      {flow.previewError ? (
        <ImportNote tone="warn">
          {tb('csvImport.previewFailed')}. {translateApiError(flow.previewError, tc)}
        </ImportNote>
      ) : !preview || !counts ? (
        <div aria-busy>
          <p className="mx-1 mb-3 mt-0 text-m-sub text-ink-3">{tb('csvImport.previewLoading')}</p>
          <div
            aria-hidden
            className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper"
          >
            {[0, 1, 2].map((i) => (
              <div key={i} className="flex min-h-[60px] items-center gap-3 px-3.5 py-2.5">
                <div className="skeleton size-9 flex-none rounded-full" />
                <div className="min-w-0 flex-1">
                  <div className="skeleton h-3.5 w-2/5" />
                  <div className="skeleton mt-2 h-3 w-3/5" />
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <>
          <p className="mx-1 mb-0.5 mt-0 text-m-row text-ink">
            {tb('csvImport.preview.willImport', { count: counts.willImport })}
            {counts.skippedDuplicates > 0 &&
              ` · ${tb('csvImport.summary.duplicatesSkipped', { count: counts.skippedDuplicates })}`}
          </p>
          <p className="mx-1 mb-0.5 mt-0 text-m-sub text-ink-3 num">
            {[
              preview.dateRange &&
                tb('statementImport.period', {
                  from: fmt.day(preview.dateRange.from),
                  to: fmt.day(preview.dateRange.to),
                }),
              tb('csvImport.preview.categorized', { count: counts.categorized }),
              tb('csvImport.preview.uncategorized', { count: counts.uncategorized }),
            ]
              .filter(Boolean)
              .join(' · ')}
          </p>
          {(counts.errors > 0 || counts.willPromote > 0) && (
            <p className="mx-1 mb-0.5 mt-0 text-m-sub text-ink-3">
              {counts.errors > 0 && (
                <span className="text-loss">
                  {tb('csvImport.summary.errors', { count: counts.errors })}
                </span>
              )}
              {counts.errors > 0 && counts.willPromote > 0 && ' · '}
              {counts.willPromote > 0 && tb('readiness.promotions', { count: counts.willPromote })}
            </p>
          )}
          {preview.overlap && (
            <div className="mt-2">
              <ImportNote>
                {tb('csvImport.overlapHint', {
                  count: preview.overlap.newRowsInCoveredRange,
                  file: preview.overlap.batches[0]?.fileName ?? '',
                })}
              </ImportNote>
            </div>
          )}
          {preview.rows.length === 0 ? (
            <p className="mx-1 my-6 text-center text-m-body text-ink-3">
              {tb('csvImport.preview.noRows')}
            </p>
          ) : (
            <>
              <p className="mx-1 mb-2 mt-2 text-m-caption text-ink-4">{t('import.preview.hint')}</p>
              <PreviewRows
                rows={preview.rows}
                total={preview.totalRows}
                accountCurrency={currency}
                categories={categories}
                overrides={overrides}
                onPick={onPick}
                updating={flow.previewUpdating}
              />
            </>
          )}
        </>
      )}
    </>
  );
}

/**
 * What the statement says about itself (D9, D10, D11): whose account it is, with the way to the
 * right one, whether its balances add up, a currency that differs, the warnings of the reader.
 */
function StatementNotes({ flow }: { flow: ImportFlow }) {
  const { t: tb } = useTranslation('bank_accounts');
  const moneyText = useMoneyText();
  const confirmId = useId();
  const { summary, statement, target, accounts } = flow;
  if (!summary || !statement) return null;

  const identifier = summary.accountIdentifier ?? summary.accountName;
  const matchedId = summary.matchedAccountId;
  const matchedName =
    (matchedId !== null ? accounts.find((account) => account.id === matchedId)?.name : undefined) ??
    identifier ??
    '';
  const currencyDiffers =
    summary.currency !== null && target !== undefined && summary.currency !== target.currency;

  const switchTo = matchedId !== null && summary.accountMatch === 'other' && (
    <Button
      type="button"
      variant="ghost"
      className="mt-1.5 h-auto min-h-11 whitespace-normal py-2 text-left w-full justify-start rounded-[10px] px-2 text-m-sub text-ink"
      disabled={flow.switching}
      onClick={() => void flow.selectAccount(matchedId)}
    >
      {tb('statementImport.match.switch', { account: matchedName })}
    </Button>
  );

  return (
    <>
      {summary.accountMatch === 'selected' && (
        <ImportNote>{tb('statementImport.match.selected')}</ImportNote>
      )}
      {summary.accountMatch === 'other' && (
        <ImportNote tone="warn" actions={switchTo || undefined}>
          {tb('statementImport.match.other', { account: matchedName })}
        </ImportNote>
      )}
      {summary.accountMatch === 'none' && (
        <ImportNote tone="warn">
          {tb('statementImport.match.none', { identifier: identifier ?? '' })}
        </ImportNote>
      )}
      {summary.accountMatch === 'unknown' && identifier && (
        <ImportNote>{tb('statementImport.match.unknown', { identifier })}</ImportNote>
      )}
      {flow.confirmRequired && (
        <div className="-mt-1 mb-3">
          <SwitchRow
            id={confirmId}
            label={tb('statementImport.match.confirm')}
            checked={flow.accountConfirmed}
            onCheckedChange={flow.setAccountConfirmed}
            disabled={flow.switching}
          />
        </div>
      )}

      {summary.balanceCheck === 'ok' && (
        <ImportNote tone="ok">{tb('statementImport.check.ok')}</ImportNote>
      )}
      {summary.balanceCheck === 'mismatch' && (
        <ImportNote tone="warn">
          {tb('statementImport.check.mismatch', {
            difference: exactMoneyText(
              moneyText,
              summary.balanceDifference ?? '0',
              summary.currency ?? ''
            ),
          })}
        </ImportNote>
      )}
      {summary.balanceCheck === 'unavailable' && (
        <ImportNote>{tb('statementImport.check.unavailable')}</ImportNote>
      )}

      {currencyDiffers && (
        <ImportNote tone="warn">
          {tb('statementImport.currencyDiffers', {
            statement: summary.currency,
            account: target?.currency,
          })}
        </ImportNote>
      )}
      {statement.warnings.map((warning, i) => (
        <ImportNote key={i} tone="warn">
          {tb(warning.key, { detail: warning.detail ?? '', defaultValue: warning.key })}
        </ImportNote>
      ))}
    </>
  );
}
