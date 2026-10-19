import { useState, type ReactNode } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Check, List, Scale, Sparkles } from 'lucide-react';
import { toast } from 'sonner';
import type { CsvImportResult } from '@shared/schema';
import {
  nextStepsFor,
  regionName,
  type NextStepId,
} from '@/components/bank-accounts/csv-import/import-config';
import {
  useEnableRulePack,
  useRecordBalance,
} from '@/components/bank-accounts/csv-import/use-csv-import-mutations';
import { Button } from '@/components/ui/button';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { Sheet } from '@/mobile/components/Sheet';
import { StatRow } from '@/mobile/components/StatRow';
import { useAutoCategorize } from '@/mobile/hooks/use-auto-categorize';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { exactMoneyText, importFacts } from '@/mobile/lib/import-flow';

interface DoneStepProps {
  result: CsvImportResult;
  /** The account the import wrote to; the rows below act on it. */
  accountId: string;
  fileName: string;
  /** "Zařadit M plateb": the sheet closes and the categorize queue of the account opens. */
  onQueue: () => void;
  /** "Zobrazit importované": the sheet closes and the account opens. */
  onShow: () => void;
}

/** The rows under the primary: the desktop's next steps, without "review" (the primary is its way). */
type ShownStep = Exclude<NextStepId, 'review'>;
const STEP_ORDER: readonly ShownStep[] = ['show', 'rulePack', 'balance'];

/**
 * The done step (spec §8.6, README §13 "Import done"): the three stats, the statement's balance
 * and what the rules did with the payments, the one primary — "Zařadit M plateb", the categorize
 * queue — while any are left, and the desktop's next steps as rows: show what was imported, turn
 * the suggested rule pack on (then the rules run over the account's open payments), set the
 * account's balance to the statement's. A step that was carried out stays, ticked. Every amount
 * is masked while amounts are hidden.
 */
export function DoneStep({ result, accountId, fileName, onQueue, onShow }: DoneStepProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { account } = useBankAccount(accountId);
  const recordBalance = useRecordBalance(accountId);
  const enableRulePack = useEnableRulePack();
  const autoCategorize = useAutoCategorize();
  const [done, setDone] = useState<ReadonlySet<ShownStep>>(new Set());
  const [confirmBalance, setConfirmBalance] = useState(false);

  const facts = importFacts(result);
  const statement = result.statementBalance;
  const suggested = nextStepsFor(result, {
    canShowImported: true,
    // The primary above is the way to the queue
    canReviewUncategorized: false,
    accountBalance: account?.balance ?? null,
    accountBalanceAsOf: account?.balanceAsOf ?? null,
  });
  // A step that was just carried out stays on screen, ticked, though it no longer applies
  const visible = STEP_ORDER.filter((id) => suggested.includes(id) || done.has(id));
  const markDone = (id: ShownStep) => setDone((current) => new Set(current).add(id));

  const balanceBlocked = !statement || !account || statement.currency !== account.currency;
  // Older than the newest record: it goes into the history and the current balance stays
  const olderThanCurrent =
    statement !== null && account?.balanceAsOf != null && statement.asOf < account.balanceAsOf;
  const packId = result.suggestedRulePack;
  const country = packId ? regionName(packId, fmt.locale) : '';
  const balanceText = (amount: string, currency: string) =>
    exactMoneyText(moneyText, amount, currency);

  const handleRulePack = async () => {
    if (!packId || enableRulePack.isPending) return;
    try {
      await enableRulePack.mutateAsync(packId);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
      return;
    }
    markDone('rulePack');
    toast(tb('csvImport.next.rulePackEnabled', { country }));
    // Reports itself in toasts and never throws
    await autoCategorize(accountId);
  };

  const handleBalance = () => {
    if (!account || !statement) return;
    recordBalance.mutate(
      {
        amount: statement.amount,
        currency: statement.currency,
        asOf: statement.asOf,
        sourceBatchId: statement.batchId,
        confirmUnknownAsOf: account.balanceAsOf == null,
      },
      {
        onSuccess: () => {
          markDone('balance');
          setConfirmBalance(false);
        },
        onError: (error) =>
          toast.error(tc('status.error'), { description: translateApiError(error, tc) }),
      }
    );
  };

  const rows: Record<
    ShownStep,
    { label: string; hint?: string; icon: ReactNode; onClick: () => void; disabled?: boolean }
  > = {
    show: {
      label: tb('csvImport.next.show'),
      hint: tb('csvImport.nextRows.showHint', { count: result.importedCount, file: fileName }),
      icon: <List strokeWidth={1.75} />,
      onClick: onShow,
    },
    rulePack: {
      label: tb('csvImport.next.rulePack', { country }),
      hint: tb('csvImport.next.rulePackHint'),
      icon: <Sparkles strokeWidth={1.75} />,
      onClick: () => void handleRulePack(),
    },
    balance: {
      label: tb('csvImport.next.setBalance', {
        balance: balanceText(statement?.amount ?? '0', statement?.currency ?? ''),
      }),
      hint: balanceBlocked
        ? tb('readiness.balanceBlocked')
        : tb('readiness.statementAsOf', {
            date: statement ? fmt.day(statement.asOf) : tb('readiness.unknownDate'),
          }),
      icon: <Scale strokeWidth={1.75} />,
      onClick: () => setConfirmBalance(true),
      disabled: balanceBlocked,
    },
  };

  return (
    <>
      <StatRow
        stats={[
          { label: tb('csvImport.done.imported'), value: String(facts.imported) },
          { label: tb('csvImport.done.duplicates'), value: String(facts.duplicates) },
          {
            label: tb('csvImport.done.errors'),
            value: String(facts.errors),
            tone: facts.errors > 0 ? 'loss' : undefined,
          },
        ]}
      />

      <div className="mx-1 mb-3.5 grid gap-1.5 text-m-sub text-ink-3">
        {statement ? (
          <p className="m-0">
            <Trans
              t={tb}
              i18nKey="csvImport.done.statementBalance"
              values={{
                date: fmt.day(statement.asOf),
                balance: balanceText(statement.amount, statement.currency),
              }}
              components={{ b: <b className="font-semibold text-ink num" /> }}
            />
          </p>
        ) : (
          result.lastBalance !== null &&
          account && (
            <p className="m-0">
              <Trans
                t={t}
                i18nKey="import.done.lastBalance"
                values={{ balance: balanceText(result.lastBalance, account.currency) }}
                components={{ b: <b className="font-semibold text-ink num" /> }}
              />
            </p>
          )
        )}
        {facts.imported > 0 && (
          <p className="m-0">
            {[
              facts.categorized > 0 && t('import.done.categorized', { count: facts.categorized }),
              facts.uncategorized > 0
                ? t('import.done.open', { count: facts.uncategorized })
                : t('import.done.allSorted'),
            ]
              .filter(Boolean)
              .join(' ')}
          </p>
        )}
        {result.promotedCount > 0 && (
          <p className="m-0">{t('import.done.promoted', { count: result.promotedCount })}</p>
        )}
      </div>

      {facts.uncategorized > 0 && (
        <Button className="mb-3.5 h-12 w-full rounded-[10px] text-m-row" onClick={onQueue}>
          {t('import.done.queue', { count: facts.uncategorized })}
        </Button>
      )}

      {visible.length > 0 && (
        <EntityRows>
          {visible.map((id) => {
            const row = rows[id];
            const finished = done.has(id);
            return (
              <EntityRow
                key={id}
                chip={
                  <RowChip tone={finished ? 'gain' : 'default'}>
                    {finished ? <Check strokeWidth={2} /> : row.icon}
                  </RowChip>
                }
                title={row.label}
                sub={row.hint}
                chevron
                muted={finished || row.disabled}
                onClick={finished || row.disabled ? undefined : row.onClick}
              />
            );
          })}
        </EntityRows>
      )}

      <Sheet
        open={confirmBalance}
        onOpenChange={setConfirmBalance}
        title={tb('readiness.confirmBalance')}
        description={tb(
          olderThanCurrent
            ? 'readiness.balanceOlderDescription'
            : 'readiness.balanceConfirmDescription',
          {
            amount: balanceText(statement?.amount ?? '0', statement?.currency ?? ''),
            date: statement ? fmt.day(statement.asOf) : tb('readiness.unknownDate'),
            currentDate: account?.balanceAsOf
              ? fmt.day(account.balanceAsOf)
              : tb('readiness.unknownDate'),
          }
        )}
        footer={
          <>
            <Button
              variant="ghost"
              className={FOOTER_GHOST}
              onClick={() => setConfirmBalance(false)}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button
              className={FOOTER_PRIMARY}
              disabled={balanceBlocked}
              loading={recordBalance.isPending}
              onClick={handleBalance}
            >
              {tb('readiness.confirmBalance')}
            </Button>
          </>
        }
      >
        {null}
      </Sheet>
    </>
  );
}
