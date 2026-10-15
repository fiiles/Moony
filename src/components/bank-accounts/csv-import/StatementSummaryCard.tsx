import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, FileText, Info, TriangleAlert } from 'lucide-react';
import type { StatementInspection, StatementSummary } from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { Alert, AlertActions } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { formatBalance } from './import-config';

interface StatementSummaryCardProps {
  inspection: StatementInspection;
  /** The statement being previewed (`statementIndex`). */
  summary: StatementSummary;
  onStatementChange: (index: number) => void;
  fileName: string;
  /** Currency of the account the import writes to. */
  accountCurrency: string | null | undefined;
  /** The account check stops the import until `accountConfirmed` (D9). */
  confirmRequired: boolean;
  accountConfirmed: boolean;
  onAccountConfirmedChange: (confirmed: boolean) => void;
  /** Make the account the statement belongs to the import's target. */
  onSwitchAccount: (accountId: string) => void;
  /** The file is being read again (after a switch) or imported; the controls wait for it. */
  busy: boolean;
  /** Locale for amount formatting, e.g. `cs-CZ`. */
  locale: string;
}

/** A note under the summary: an icon and one sentence, in the tone of what it says. */
function Note({ tone, children }: { tone: 'ok' | 'muted'; children: ReactNode }) {
  const Icon = tone === 'ok' ? Check : Info;
  return (
    <p
      className={
        tone === 'ok'
          ? 'm-0 flex items-start gap-2 text-caption font-600 text-gain'
          : 'm-0 flex items-start gap-2 text-caption text-ink-3'
      }
    >
      <Icon className="mt-px size-3.5 shrink-0" aria-hidden />
      <span className="min-w-0">{children}</span>
    </p>
  );
}

/** A warning (design system `.alert`), with what resolves it when there is something to do. */
function Warning({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <Alert>
      <TriangleAlert aria-hidden />
      <p className="m-0">{children}</p>
      {actions && <AlertActions className="flex-wrap items-center gap-x-4">{actions}</AlertActions>}
    </Alert>
  );
}

/**
 * What the statement file says before its payments are previewed (spec §7, §8): the statement
 * picker when the file holds several, the format, account, period, balances and counts, whether
 * the balances add up (D11), whose account it is (D9) and whether its currency differs (D10).
 */
export function StatementSummaryCard({
  inspection,
  summary,
  onStatementChange,
  fileName,
  accountCurrency,
  confirmRequired,
  accountConfirmed,
  onAccountConfirmedChange,
  onSwitchAccount,
  busy,
  locale,
}: StatementSummaryCardProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const { accounts } = useBankAccounts();

  const label = (statement: StatementSummary) =>
    statement.accountIdentifier ?? statement.accountName ?? '—';
  const identifier = summary.accountIdentifier ?? summary.accountName;
  const matchedAccountId = summary.matchedAccountId;
  const matched =
    matchedAccountId != null
      ? accounts.find((account) => account.id === matchedAccountId)
      : undefined;
  const matchedName = matched?.name ?? identifier ?? '';
  const balance = (amount: string) => formatBalance(amount, summary.currency, locale, currencyCode);

  const confirmation = confirmRequired && (
    <label className="flex cursor-pointer items-center gap-2 text-caption font-600 text-ink-2">
      <Checkbox
        checked={accountConfirmed}
        disabled={busy}
        onCheckedChange={(checked) => onAccountConfirmedChange(checked === true)}
      />
      {t('statementImport.match.confirm')}
    </label>
  );

  const matchBlock = () => {
    switch (summary.accountMatch) {
      case 'selected':
        return <Note tone="muted">{t('statementImport.match.selected')}</Note>;
      case 'other':
        return (
          <Warning
            actions={
              <>
                {matchedAccountId != null && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => onSwitchAccount(matchedAccountId)}
                  >
                    {t('statementImport.match.switch', { account: matchedName })}
                  </Button>
                )}
                {confirmation}
              </>
            }
          >
            {t('statementImport.match.other', { account: matchedName })}
          </Warning>
        );
      case 'none':
        return (
          <Warning actions={confirmation || undefined}>
            {t('statementImport.match.none', { identifier: identifier ?? '' })}
          </Warning>
        );
      case 'unknown':
        return identifier ? (
          <Note tone="muted">{t('statementImport.match.unknown', { identifier })}</Note>
        ) : null;
    }
  };

  const currencyDiffers =
    summary.currency != null && accountCurrency != null && summary.currency !== accountCurrency;

  return (
    <div className="mb-3.5 grid gap-2.5">
      {inspection.statements.length > 1 && (
        <div className="rounded-r3 border border-line px-3.5 py-3">
          <p className="mb-2 mt-0 text-caption font-600 text-ink-2">
            {t('statementImport.pickStatement')}
          </p>
          <RadioGroup
            value={String(summary.index)}
            onValueChange={(value) => onStatementChange(Number(value))}
            disabled={busy}
            className="gap-1.5"
          >
            {inspection.statements.map((statement) => (
              <label
                key={statement.index}
                className="flex cursor-pointer items-center gap-2.5 text-table text-ink-2"
              >
                <RadioGroupItem value={String(statement.index)} />
                <span className="min-w-0 truncate font-600 text-ink num">{label(statement)}</span>
                {statement.currency && <span className="text-ink-3">{statement.currency}</span>}
                <span className="text-micro font-500 text-ink-4">
                  {t('statementImport.entries', { count: statement.entryCount })}
                </span>
              </label>
            ))}
          </RadioGroup>
        </div>
      )}

      <div className="rounded-r3 border border-line bg-well px-3.5 py-3 text-table text-ink-2">
        <div className="flex items-center gap-3">
          <FileText className="size-4 shrink-0 text-ink-3" aria-hidden />
          <span className="min-w-0 truncate">
            <b className="font-650 text-ink">{fileName}</b>
            {identifier && <span className="num"> · {identifier}</span>}
            {summary.accountIdentifier && summary.accountName && (
              <span className="text-ink-3"> · {summary.accountName}</span>
            )}
          </span>
          <Badge className="shrink-0">{t(`statementImport.format.${inspection.format}`)}</Badge>
          {summary.period && (
            <span className="ml-auto shrink-0 text-micro font-500 text-ink-4 num">
              {t('statementImport.period', {
                from: fmt.day(summary.period.from),
                to: fmt.day(summary.period.to),
              })}
            </span>
          )}
        </div>
        <div className="mt-2 flex flex-wrap gap-x-[22px] gap-y-1 pl-7 text-caption text-ink-3">
          {summary.openingBalance && (
            <span>
              {t('statementImport.opening')}{' '}
              <b className="font-650 text-ink num">{balance(summary.openingBalance.amount)}</b>
            </span>
          )}
          {summary.closingBalance && (
            <span>
              {t('statementImport.closing')}{' '}
              <b className="font-650 text-ink num">{balance(summary.closingBalance.amount)}</b>
            </span>
          )}
          <span>{t('statementImport.entries', { count: summary.entryCount })}</span>
          {summary.skippedCount > 0 && (
            <span>{t('statementImport.skipped', { count: summary.skippedCount })}</span>
          )}
        </div>
      </div>

      {summary.balanceCheck === 'ok' && <Note tone="ok">{t('statementImport.check.ok')}</Note>}
      {summary.balanceCheck === 'mismatch' && (
        <Warning>
          {t('statementImport.check.mismatch', {
            difference: balance(summary.balanceDifference ?? '0'),
          })}
        </Warning>
      )}
      {summary.balanceCheck === 'unavailable' && (
        <Note tone="muted">{t('statementImport.check.unavailable')}</Note>
      )}

      {matchBlock()}

      {currencyDiffers && (
        <Warning>
          {t('statementImport.currencyDiffers', {
            statement: summary.currency,
            account: accountCurrency,
          })}
        </Warning>
      )}

      {inspection.warnings.map((warning, i) => (
        <Warning key={i}>
          {t(warning.key, { detail: warning.detail ?? '', defaultValue: warning.key })}
        </Warning>
      ))}
    </div>
  );
}
