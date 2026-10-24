import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bandage,
  Car,
  CreditCard,
  Ellipsis,
  Heart,
  House,
  Landmark,
  Plane,
  Plus,
  Scale,
  Shield,
  Stethoscope,
  Tag,
  type LucideIcon,
} from 'lucide-react';
import { toast } from 'sonner';
import type { BankAccountWithInstitution, InsertBankAccount } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import { useLoanText } from '@/hooks/use-loan-text';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { CHART_PERIODS, type ChartPeriod } from '@/utils/period';
import { BankAccountFormDialog } from '@/components/bank-accounts/BankAccountFormDialog';
import { EmptyState } from '@/components/common/EmptyState';
import { InsuranceFormDialog } from '@/components/insurance/InsuranceFormDialog';
import { LoanFormDialog } from '@/components/loans/LoanFormDialog';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { Hero } from '@/mobile/components/Hero';
import { SectionHead } from '@/mobile/components/SectionHead';
import { TrendCard } from '@/mobile/components/TrendCard';
import { useCashTrend } from '@/mobile/hooks/use-cash-trend';
import { useFinance, type Finance as FinanceData } from '@/mobile/hooks/use-finance';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import {
  accountSub,
  accountTrail,
  cashChange,
  sortAccounts,
  type AccountSort,
} from '@/mobile/lib/finance-rows';
import { queuePath } from '@/mobile/lib/categorize-queue';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { initials } from '@/mobile/lib/initials';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { accountPath, loanPath, policyPath } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

type AddKind = 'account' | 'loan' | 'policy';

const DEFAULT_PERIOD: ChartPeriod = '30D';

const SORT_LABELS: Record<AccountSort, string> = {
  balance: 'finance.menu.sortBalance',
  name: 'finance.menu.sortName',
  change: 'finance.menu.sortChange',
  last: 'finance.menu.sortLast',
};

/** The icon of a policy row by its type (`insurance:types.*`); an unknown type is the shield. */
const POLICY_ICONS: Record<string, LucideIcon> = {
  life: Heart,
  health: Stethoscope,
  property: House,
  vehicle: Car,
  travel: Plane,
  liability: Scale,
  accident: Bandage,
  other: Shield,
};

/** A policy's next payment day for sorting; one without a payment ahead goes last. */
const nextDay = (row: FinanceData['insurance']['rows'][number]) =>
  row.nextPayment ?? Number.MAX_SAFE_INTEGER;

/** The menu items of the `+` and `···` menus: 44 pt tall, body type (as `AgendaRow`'s menu). */
const MENU_ITEM = 'min-h-11 text-m-body';

/**
 * Finance tab root (spec §8.4, prototype frame 03): the compact total on accounts, the payments
 * without a category, then accounts, loans and policies as entity rows — each section head carries
 * its one number — and the cash trend. `+` adds an account, a loan or a policy (the desktop's
 * dialogs); `···` sorts the accounts and shows the paid-off loans and ended policies. The desktop's
 * list pages have no phone equivalent beyond this (summary stats, trajectory, payment calendar).
 */
export default function Finance() {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  // Money as text (the hero sizes itself to the shown text; the trend callout, the change line and
  // the rows only render it); masked while amounts are hidden
  const moneyText = useMoneyText();
  const finance = useFinance();
  const { accounts, totals, loans, insurance, mainCurrency, fxReady, isLoading } = finance;

  // How the tab was left is kept for the session (a visit to an account remounts it); the add
  // dialog is not, so it does not reopen on return
  const [sort, setSort] = useRemembered<AccountSort>('finance.sort', 'balance');
  const [showMatured, setShowMatured] = useRemembered('finance.showMatured', false);
  const [showEnded, setShowEnded] = useRemembered('finance.showEnded', false);
  const [period, setPeriod] = useRemembered<ChartPeriod>('finance.period', DEFAULT_PERIOD);
  const [addKind, setAddKind] = useState<AddKind | null>(null);
  const { createAccount } = useBankAccountMutations();
  const { createMutation: createLoan } = useLoanMutations();

  const money0 = (value: number) => moneyText(value, mainCurrency, { decimals: 0 });

  // Every account that counts toward the total is in a currency without a rate: nothing to sum
  const countedAccounts = accounts.filter((a) => !a.excludeFromBalance);
  const totalUnknown =
    countedAccounts.length > 0 &&
    countedAccounts.every((a) => totals.missing.includes(recordCurrency(a.currency)));
  const trend = useCashTrend(period, fxReady && !totalUnknown ? totals.balance : null);

  const loanRows = useMemo(
    () =>
      loans.rows
        .filter((row) => showMatured || !row.matured)
        .sort((a, b) => (b.converted?.outstanding ?? -1) - (a.converted?.outstanding ?? -1)),
    [loans.rows, showMatured]
  );
  const policyRows = useMemo(
    () =>
      insurance.rows
        .filter((row) => showEnded || !row.ended)
        .sort((a, b) => nextDay(a) - nextDay(b)),
    [insurance.rows, showEnded]
  );

  // Currencies the totals, the section heads or the history leave out for want of a rate
  const missing = [
    ...new Set([
      ...totals.missing,
      ...loans.metrics.missing,
      ...insurance.metrics.missing,
      ...trend.missing,
    ]),
  ];

  const subtitle = [
    accounts.length > 0 && t('finance.accounts', { count: accounts.length }),
    loanRows.length > 0 && t('finance.loans', { count: loanRows.length }),
    policyRows.length > 0 && t('finance.policies', { count: policyRows.length }),
  ]
    .filter(Boolean)
    .join(' · ');

  const change = totalUnknown ? null : cashChange(totals.balance, totals.net30);
  const isEmpty =
    !isLoading && accounts.length === 0 && loans.rows.length === 0 && insurance.rows.length === 0;
  const close = () => setAddKind(null);

  return (
    <Screen
      title={t('tabs.finance')}
      large
      subtitle={subtitle}
      trailing={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={t('finance.add.title')}>
                <Plus strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[200px]">
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => setAddKind('account')}>
                {t('finance.add.account')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => setAddKind('loan')}>
                {t('finance.add.loan')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => setAddKind('policy')}>
                {t('finance.add.policy')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={tc('labels.actions')}>
                <Ellipsis strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              <DropdownMenuLabel>{t('finance.menu.sort')}</DropdownMenuLabel>
              <DropdownMenuRadioGroup
                value={sort}
                onValueChange={(next) => setSort(next as AccountSort)}
              >
                {(Object.keys(SORT_LABELS) as AccountSort[]).map((key) => (
                  <DropdownMenuRadioItem key={key} value={key} className={MENU_ITEM}>
                    {t(SORT_LABELS[key])}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              <DropdownMenuSeparator />
              <DropdownMenuCheckboxItem
                className={MENU_ITEM}
                checked={showMatured}
                onCheckedChange={setShowMatured}
              >
                {t('finance.menu.showMatured')}
              </DropdownMenuCheckboxItem>
              <DropdownMenuCheckboxItem
                className={MENU_ITEM}
                checked={showEnded}
                onCheckedChange={setShowEnded}
              >
                {t('finance.menu.showEnded')}
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className={MENU_ITEM}
                onSelect={() => toast(t('placeholder.export'))}
              >
                {t('finance.menu.export')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    >
      {isLoading ? (
        <FinanceSkeleton />
      ) : isEmpty ? (
        <EmptyState
          icon={<Landmark />}
          title={tb('empty.title')}
          description={tb('empty.description')}
          className="px-5 py-10"
          action={
            <Button
              className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
              onClick={() => setAddKind('account')}
            >
              <Plus aria-hidden />
              {t('finance.add.account')}
            </Button>
          }
        />
      ) : (
        <>
          {/* Amounts left out of the totals for want of a rate (never counted as 0 or 1:1) */}
          <ConversionQualityNote
            quality={{ missingCurrencies: missing }}
            currency={mainCurrency}
            className="mx-0.5 mb-3.5"
          />
          {accounts.length > 0 && (
            <>
              <Hero
                label={t('finance.total')}
                value={totalUnknown ? null : totals.balance}
                formatValue={money0}
                change={
                  change
                    ? {
                        ratio: change.ratio,
                        text: t('finance.change30', {
                          amount: moneyText(change.delta, mainCurrency, {
                            signed: true,
                            decimals: 0,
                          }),
                        }),
                      }
                    : undefined
                }
                note={!totalUnknown && totals.net30 === 0 ? t('finance.noMovements') : undefined}
              />
              <UncategorizedRow finance={finance} />
              <AccountsSection finance={finance} sort={sort} />
            </>
          )}
          <LoansSection finance={finance} rows={loanRows} />
          <PoliciesSection finance={finance} rows={policyRows} />
          {accounts.length > 0 && (trend.points.length > 1 || period !== DEFAULT_PERIOD) && (
            <TrendCard
              title={t('finance.sections.cashTrend')}
              note={t(`overview.change.${period}`)}
              points={trend.points}
              period={period}
              periods={CHART_PERIODS}
              onPeriodChange={setPeriod}
              formatValue={money0}
              label={t('finance.cashTrendLabel')}
            />
          )}
        </>
      )}

      {/* The desktop's dialogs, as they are, opened from the `+` menu */}
      <BankAccountFormDialog
        open={addKind === 'account'}
        onOpenChange={(open) => !open && close()}
        onSubmit={(data, zones) =>
          createAccount.mutate({ data: data as InsertBankAccount, zones }, { onSuccess: close })
        }
        isLoading={createAccount.isPending}
      />
      <LoanFormDialog
        open={addKind === 'loan'}
        onOpenChange={(open) => !open && close()}
        onSubmit={(data) => {
          if ('id' in data) return;
          createLoan.mutate(data, { onSuccess: close });
        }}
        isLoading={createLoan.isPending}
      />
      <InsuranceFormDialog open={addKind === 'policy'} onOpenChange={(open) => !open && close()} />
    </Screen>
  );
}

/**
 * "Nezařazené platby" as a dark row above the accounts, only while there are any: one account
 * names itself (and the statement they came from), several are counted. It opens the categorize
 * queue, of that account when one holds the payments, else of every account.
 */
function UncategorizedRow({ finance }: { finance: FinanceData }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const { accounts, lastImportByAccount, uncategorized } = finance;
  if (uncategorized.count === 0) return null;

  const only = uncategorized.accountIds.length === 1 ? uncategorized.accountIds[0] : undefined;
  const name = accounts.find((a) => a.id === only)?.name ?? '';
  const last = only === undefined ? null : lastImportByAccount[only];
  const sub =
    only === undefined
      ? t('upkeep.accounts', { count: uncategorized.accountIds.length })
      : last != null
        ? t('upkeep.uncategorizedFrom', {
            account: name,
            date: fmt.day(last, shortDayOptions(last)),
          })
        : name;

  return (
    <EntityRows>
      <EntityRow
        chip={
          <RowChip tone="dark">
            <Tag strokeWidth={1.75} />
          </RowChip>
        }
        title={t('finance.uncategorized')}
        sub={sub}
        value={fmt.number(uncategorized.count)}
        href={queuePath(uncategorized.accountIds)}
      />
    </EntityRows>
  );
}

/** Účty: one row per account — bank chip, name, "Fio · CZK · výpis 10. 10.", balance and its trail. */
function AccountsSection({ finance, sort }: { finance: FinanceData; sort: AccountSort }) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { accounts, activity, lastImportByAccount, mainCurrency, convert } = finance;
  const todayDay = utcDayFloor(Date.now() / 1000);

  const sorted = useMemo(
    () =>
      sortAccounts(accounts, sort, {
        convert,
        activity: (id) => {
          const a = activity.get(id);
          return a ? { net: a.last30.net, lastDay: a.last?.bookingDate ?? null } : undefined;
        },
      }),
    [accounts, sort, convert, activity]
  );

  const row = (account: BankAccountWithInstitution) => {
    const sub = accountSub(account, lastImportByAccount[account.id] ?? null);
    const balance = parseFloat(account.balance || '0') || 0;
    const a = activity.get(account.id);
    const trail = accountTrail({
      currency: sub.currency,
      mainCurrency,
      last30: a ? { net: a.last30.net, count: a.last30.count } : undefined,
      stamp: sub.stamp,
      todayDay,
      converted: convert(balance, sub.currency),
    });
    let valueSub: string | undefined;
    switch (trail.kind) {
      case 'change':
        valueSub = t('finance.trail.change', {
          amount: moneyText(trail.net, sub.currency, { signed: true, decimals: 0 }),
        });
        break;
      case 'converted':
        valueSub =
          trail.value === null
            ? t('finance.trail.noRate', { currency: sub.currency })
            : t('finance.trail.approx', {
                amount: moneyText(trail.value, mainCurrency, { decimals: 0 }),
              });
        break;
      case 'stale':
        valueSub = t(
          trail.what === 'statement'
            ? 'finance.trail.staleStatement'
            : 'finance.trail.staleBalance',
          { count: trail.days }
        );
        break;
      case 'none':
        break;
    }
    return (
      <EntityRow
        key={account.id}
        chip={
          <RowChip>
            {initials(sub.institution ?? account.name) || <Landmark strokeWidth={1.75} />}
          </RowChip>
        }
        title={
          <>
            {account.name}
            {account.excludeFromBalance && (
              <span className="ml-1.5 inline-flex min-h-[18px] items-center rounded-r1 bg-well-2 px-1.5 py-px align-[2px] text-m-caption font-semibold leading-[1.3] text-ink-3">
                {tb('table.excluded')}
              </span>
            )}
          </>
        }
        sub={[
          sub.institution ?? (sub.manual ? t('finance.manual') : null),
          sub.currency,
          sub.stamp &&
            t(sub.stamp.kind === 'statement' ? 'finance.statementOn' : 'finance.balanceOn', {
              date: fmt.day(sub.stamp.day, shortDayOptions(sub.stamp.day)),
            }),
        ]
          .filter(Boolean)
          .join(' · ')}
        value={moneyText(balance, sub.currency, { decimals: 0 })}
        valueSub={valueSub}
        href={accountPath(account.id)}
      />
    );
  };

  return (
    <section>
      <SectionHead title={t('finance.sections.accounts')} />
      <EntityRows>{sorted.map(row)}</EntityRows>
    </section>
  );
}

/** Úvěry: the head carries what is outstanding; a row is the name, "2,9 % · fixace do 3/2027", the balance and the payment. */
function LoansSection({
  finance,
  rows,
}: {
  finance: FinanceData;
  rows: FinanceData['loans']['rows'];
}) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { monthYear } = useLoanText();
  if (rows.length === 0) return null;
  const { loans, mainCurrency } = finance;

  return (
    <section>
      <SectionHead
        title={t('finance.sections.loans')}
        note={t('loans:table.footer.outstanding', {
          amount: moneyText(loans.metrics.totalOutstanding, mainCurrency, { decimals: 0 }),
        })}
      />
      <EntityRows>
        {rows.map((row) => (
          <EntityRow
            key={row.loan.id}
            chip={
              <RowChip>
                <CreditCard strokeWidth={1.75} />
              </RowChip>
            }
            title={row.loan.name}
            sub={[
              fmt.percent(Number(row.loan.interestRate) / 100, 2),
              row.fixationEnd
                ? t('loans:table.fixedUntil', { month: monthYear(row.fixationEnd) })
                : t('loans:table.fixedRate'),
            ].join(' · ')}
            value={moneyText(row.outstanding, row.currency, { decimals: 0 })}
            valueSub={
              row.matured
                ? t('loans:table.matured')
                : t('finance.perMonth', {
                    amount: moneyText(row.payment, row.currency, { decimals: 0 }),
                  })
            }
            href={loanPath(row.loan.id)}
          />
        ))}
      </EntityRows>
    </section>
  );
}

/** Pojištění: the head carries the yearly premium; a row is the name, "Kooperativa · měsíčně", the premium and the next payment. */
function PoliciesSection({
  finance,
  rows,
}: {
  finance: FinanceData;
  rows: FinanceData['insurance']['rows'];
}) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  if (rows.length === 0) return null;
  const { insurance, mainCurrency } = finance;

  return (
    <section>
      <SectionHead
        title={t('finance.sections.insurance')}
        note={t('insurance:table.footer.yearly', {
          amount: moneyText(insurance.metrics.yearlyTotal, mainCurrency, { decimals: 0 }),
        })}
      />
      <EntityRows>
        {rows.map((row) => {
          const Icon = POLICY_ICONS[row.policy.type] ?? Shield;
          const { nextPayment } = row;
          return (
            <EntityRow
              key={row.policy.id}
              chip={
                <RowChip>
                  <Icon strokeWidth={1.75} />
                </RowChip>
              }
              title={row.policy.policyName}
              sub={[
                row.policy.provider,
                row.frequency && t(`insurance:frequencyShort.${row.frequency}`),
              ]
                .filter(Boolean)
                .join(' · ')}
              value={moneyText(row.premiumNative.amount, row.premiumNative.currency, {
                decimals: 0,
              })}
              valueSub={
                row.ended
                  ? t('insurance:table.ended')
                  : nextPayment
                    ? t('finance.nextPayment', {
                        date: fmt.day(nextPayment, shortDayOptions(nextPayment)),
                      })
                    : undefined
              }
              href={policyPath(row.policy.id)}
            />
          );
        })}
      </EntityRows>
    </section>
  );
}

/** While the numbers settle: the compact hero and three rows in outline. */
function FinanceSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3.5 pt-4 shadow-e1">
        <div className="skeleton h-3 w-24" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-40" />
      </div>
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
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
  );
}
