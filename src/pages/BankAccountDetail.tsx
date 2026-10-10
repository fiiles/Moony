import { ReportError } from '@/components/reports/ReportQualityNote';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { getCompleteBankHistory } from '@/lib/bank-history';
import { bookedInAccountCurrency, rowFlowLabel, type AccountConvertAt } from '@/utils/money-flows';
import { useDatedFx } from '@/hooks/use-fx';
import { useBalanceRecords } from '@/hooks/use-balance-records';
import { useState, useMemo, useEffect, useRef, useCallback, Fragment } from 'react';
import { useRoute, useLocation, useSearch } from 'wouter';
import { useQuery, useQueryClient, useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ArrowDown,
  ArrowDownLeft,
  ArrowUp,
  ArrowUpDown,
  CalendarSearch,
  Ellipsis,
  History,
  ListFilter,
  Pencil,
  Percent,
  Plus,
  Receipt,
  Search,
  Sparkles,
  Trash2,
  Upload,
  Wallet,
} from 'lucide-react';
import type { BankTransaction, CsvImportResult, CsvImportBatch, FlowKind } from '@shared/schema';
import { FLOW_KINDS } from '@shared/schema';
import { recordCurrency, type CurrencyCode } from '@shared/currencies';
import { bankAccountsApi, categorizationApi, cashflowApi } from '@/lib/tauri-api';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import {
  useBankTransactionMutations,
  useBankAccountMutations,
} from '@/hooks/use-bank-account-mutations';
import type { CategorizationResult, TransactionFilters } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { useCurrency } from '@/lib/currency';
import { useLanguage } from '@/i18n/I18nProvider';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { useShellPage } from '@/components/shell/shell-context';
import {
  getUtcPeriodRange,
  isoDateFromUtcTimestamp,
  utcDayStart,
  utcDayEnd,
  type PeriodTimeframe,
} from '@/utils/period';
import { utcDayFloor } from '@/utils/chart-axis';
import { completeAccountIdentifier } from '@/utils/iban-utils';
import { formatAmountWithCode, formatAmountInCurrency } from '@/utils/format-amount';
import {
  detailSections,
  hasRecentRows,
  ledgerPresetForMonth,
  monthEnd,
  monthOf,
  monthStart,
  newestMonthWithRows,
} from '@/utils/bank-activity';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import { Segmented } from '@/components/ui/segmented';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { BankAccountFormDialog } from '@/components/bank-accounts/BankAccountFormDialog';
import { AddTransactionModal } from '@/components/bank-accounts/AddTransactionModal';
import { BalanceSection } from '@/components/bank-accounts/BalanceSection';
import { FlowsCard } from '@/components/bank-accounts/FlowsCard';
import { BalanceRecordDialog } from '@/components/bank-accounts/BalanceRecordDialog';
import { BalanceHistoryDialog } from '@/components/bank-accounts/BalanceHistoryDialog';
import { ImportHistoryDialog } from '@/components/bank-accounts/ImportHistoryDialog';
import { InterestDialog } from '@/components/bank-accounts/InterestDialog';
import { CsvImportDialog } from '@/components/bank-accounts/CsvImportDialog';
import { CategorySelector } from '@/components/bank-accounts/CategorySelector';
import { newRuleHref } from '@/utils/rules-link';

import { cn } from '@/lib/utils';

type SortColumn = 'date' | 'description' | 'category' | 'amount';
type Kind = 'all' | 'income' | 'outcome';

const DAY = 86400;

export default function BankAccountDetail() {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const [, params] = useRoute('/bank-accounts/:id');
  const [, setLocation] = useLocation();
  const search = useSearch();
  const accountId = params?.id;
  const { account, isLoading } = useBankAccount(accountId);
  const { deleteTransaction } = useBankTransactionMutations(accountId);
  const { updateAccount, deleteAccount } = useBankAccountMutations();
  const { currencyCode } = useCurrency();
  const { getLocale } = useLanguage();
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const queryClient = useQueryClient();
  const [isCategorizingBatch, setIsCategorizingBatch] = useState(false);

  const [editDialogOpen, setEditDialogOpen] = useState(false);
  const deepLink = useMemo(() => new URLSearchParams(search), [search]);
  const [csvImportOpen, setCsvImportOpen] = useState(() => deepLink.get('import') === '1');
  const [deleteAccountOpen, setDeleteAccountOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<BankTransaction | null>(null);
  const [pendingUndo, setPendingUndo] = useState<CsvImportBatch | null>(null);
  const [recordOpen, setRecordOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [importsOpen, setImportsOpen] = useState(false);
  const [interestOpen, setInterestOpen] = useState(false);
  const [addTxOpen, setAddTxOpen] = useState(false);
  // Flows window: the month it ends with (null = the current month) and a clicked month
  const [windowEnd, setWindowEnd] = useState<number | null>(null);
  const [selectedMonth, setSelectedMonth] = useState<number | null>(null);

  // Ledger filters (UTC calendar, ADR 0008). The ledger opens on the newest month with
  // movements, set once from the data below; a ?category= deep link (from the list page)
  // widens the range to everything and starts ready.
  const linkedCategory = deepLink.get('category');
  const [ledgerReady, setLedgerReady] = useState<boolean>(!!linkedCategory);
  const [dateFrom, setDateFrom] = useState<string>('');
  const [dateTo, setDateTo] = useState<string>('');
  const [datePreset, setDatePreset] = useState<string>(linkedCategory ? 'allTime' : 'thisMonth');
  const [kind, setKind] = useState<Kind>('all');
  const [categoryFilter, setCategoryFilter] = useState<string>(linkedCategory ?? 'all');
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [ledgerPage, setLedgerPage] = useState(0);
  const [selectedTransaction, setSelectedTransaction] = useState<BankTransaction | null>(null);
  const [sortColumn, setSortColumn] = useState<SortColumn>('date');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const applyDatePreset = (preset: string) => {
    setLedgerPage(0);
    const presets: Record<string, [PeriodTimeframe, number] | 'allTime'> = {
      thisMonth: ['monthly', 0],
      lastMonth: ['monthly', -1],
      thisQuarter: ['quarterly', 0],
      lastQuarter: ['quarterly', -1],
      thisYear: ['yearly', 0],
      lastYear: ['yearly', -1],
      allTime: 'allTime',
    };
    const spec = presets[preset];
    if (!spec) {
      setDatePreset(preset);
      return;
    }
    if (spec === 'allTime') {
      setDateFrom('');
      setDateTo('');
    } else {
      const range = getUtcPeriodRange(spec[0], spec[1]);
      setDateFrom(isoDateFromUtcTimestamp(range.start));
      setDateTo(isoDateFromUtcTimestamp(range.end));
    }
    setDatePreset(preset);
  };

  const handleDateChange = (type: 'from' | 'to', value: string) => {
    setLedgerPage(0);
    if (type === 'from') setDateFrom(value);
    else setDateTo(value);
    setDatePreset('custom');
  };

  // Ledger filters without paging; auto-categorize works on the same set
  const ledgerFilters = {
    categoryId: categoryFilter === 'all' ? undefined : categoryFilter,
    txType: kind === 'all' ? undefined : kind === 'income' ? 'credit' : 'debit',
    search: searchQuery.trim() || undefined,
    dateFrom: dateFrom ? utcDayStart(dateFrom) : undefined,
    dateTo: dateTo ? utcDayEnd(dateTo) : undefined,
  } satisfies TransactionFilters;
  // Uncategorized rows within the filters; a specific category filter has none
  const uncategorizedFilters =
    categoryFilter === 'all' || categoryFilter === 'uncategorized'
      ? { ...ledgerFilters, categoryId: 'uncategorized' }
      : null;

  // Ledger transactions for the selected range
  const {
    data: transactionsResult,
    isLoading: txQueryLoading,
    isError: txError,
    refetch: retryTransactions,
  } = useQuery({
    queryKey: [
      'bank-transactions',
      accountId,
      dateFrom,
      dateTo,
      kind,
      categoryFilter,
      searchQuery,
      sortColumn,
      sortDirection,
      ledgerPage,
    ],
    queryFn: () =>
      accountId
        ? bankAccountsApi.getTransactions(accountId, {
            ...ledgerFilters,
            limit: 100,
            offset: ledgerPage * 100,
            sortBy: sortColumn,
            ascending: sortDirection === 'asc',
          })
        : null,
    enabled: !!accountId && ledgerReady,
  });
  // Until the initial period is known the ledger reads as loading, never as empty
  const txLoading = txQueryLoading || !ledgerReady;

  // How many filtered rows lack a category, across all ledger pages
  const { data: uncategorizedTotal = 0 } = useQuery({
    queryKey: ['bank-transactions', accountId, 'uncategorized-count', uncategorizedFilters],
    queryFn: async () =>
      accountId && uncategorizedFilters
        ? (await bankAccountsApi.getTransactions(accountId, { ...uncategorizedFilters, limit: 1 }))
            .total
        : 0,
    enabled: !!accountId && ledgerReady,
  });

  // Any transaction at all? Tells "nothing imported yet" from "nothing in this period".
  const { data: anyTransactionResult, isLoading: anyTransactionLoading } = useQuery({
    queryKey: ['bank-transactions', accountId, 'any'],
    queryFn: () => (accountId ? bankAccountsApi.getTransactions(accountId, { limit: 1 }) : null),
    enabled: !!accountId,
  });

  // Every transaction of the account: the flows window, the section rules and the FX notes
  const {
    data: history,
    isLoading: historyLoading,
    isError: historyError,
    refetch: retryHistory,
  } = useQuery({
    queryKey: ['bank-transactions', accountId, 'history'],
    queryFn: () => (accountId ? getCompleteBankHistory(accountId) : null),
    enabled: !!accountId,
  });

  const { data: zones, refetch: refetchZones } = useQuery({
    queryKey: ['bank-account-zones', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getZones(accountId) : null),
    enabled: !!accountId && !!account?.hasZoneDesignation,
  });

  const { data: importBatches } = useQuery({
    queryKey: ['import-batches', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getImportBatches(accountId) : null),
    enabled: !!accountId,
  });

  const { data: categories = [] } = useQuery({
    queryKey: ['transaction-categories'],
    queryFn: () => bankAccountsApi.getCategories(),
  });

  // show exactly the imported range after an import
  const handleImported = (result: CsvImportResult) => {
    setLedgerReady(true);
    if (result.dateRange) {
      setDateFrom(isoDateFromUtcTimestamp(result.dateRange.from));
      setDateTo(isoDateFromUtcTimestamp(result.dateRange.to));
      setDatePreset('custom');
    } else {
      applyDatePreset('allTime');
    }
    setKind('all');
    setCategoryFilter('all');
    setSearchQuery('');
    queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
    queryClient.invalidateQueries({ queryKey: ['import-batches', accountId] });
    queryClient.invalidateQueries({ queryKey: ['bank-account', accountId] });
  };

  const deleteBatchMutation = useMutation({
    mutationFn: (batchId: string) => bankAccountsApi.deleteImportBatch(batchId),
    onSuccess: async (count) => {
      await refreshMoneyFlows(queryClient);
      toast(t('readiness.batchDeleted', { count }));
      setPendingUndo(null);
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  const transactions = useMemo(() => transactionsResult?.transactions || [], [transactionsResult]);

  const getEffectiveCategory = (tx: BankTransaction) => tx.categoryId;

  const filteredTransactions = transactions;

  const effectiveFlowIds = useMemo(
    () => [
      ...new Set([
        ...transactions.map((tx) => tx.id),
        ...(selectedTransaction ? [selectedTransaction.id] : []),
      ]),
    ],
    [transactions, selectedTransaction]
  );
  const {
    data: effectiveFlows = [],
    isFetching: flowsLoading,
    isError: flowsError,
    refetch: retryFlows,
  } = useQuery({
    queryKey: ['effective-transaction-flows', effectiveFlowIds],
    queryFn: () => cashflowApi.getEffectiveFlows(effectiveFlowIds),
    enabled: effectiveFlowIds.length > 0,
  });
  const flowById = new Map(effectiveFlows.map((flow) => [flow.id, flow]));
  const missingFlows =
    !flowsLoading && !flowsError && effectiveFlowIds.some((id) => !flowById.has(id));
  const flowLabel = (id: string, withSource = false) => {
    if (flowsError) return t('readiness.flowUnavailable');
    if (flowsLoading) return t('readiness.flowLoading');
    const flow = flowById.get(id);
    if (!flow) return t('readiness.flowUnresolved');
    return `${t(`readiness.flow.${flow.effectiveFlowKind}`)}${withSource ? ` · ${t(`readiness.source.${flow.flowSource}`)}` : ''}`;
  };
  // Ledger rows name a meaning only when the sign does not already say it;
  // loading/unresolved states are reported once above the table.
  const rowFlow = (id: string, isCredit: boolean, hasCategory: boolean) => {
    const flow = flowById.get(id);
    return flow ? rowFlowLabel(flow, isCredit, hasCategory) : null;
  };
  const flowReadError = flowsError ? (
    <ReportError onRetry={() => void retryFlows()} />
  ) : missingFlows ? (
    <p className="mb-3 text-micro text-ink-4" role="status">
      {t('readiness.flowUnresolved')}{' '}
      <Button variant="ghost" size="sm" onClick={() => void retryFlows()}>
        {t('readiness.retry')}
      </Button>
    </p>
  ) : null;
  // The flows card is in the account currency: rows from another currency
  // pocket are converted at the rate of their booking day, unconvertible ones left out.
  const accountCurrency = recordCurrency(account?.currency) as CurrencyCode;
  const formatAccountAmount = (value: number) =>
    formatAmountInCurrency(value, accountCurrency, getLocale(), currencyCode);

  const firstForeignDay = useMemo(() => {
    const days = (history?.transactions ?? [])
      .filter((tx) => tx.status === 'booked' && tx.currency !== accountCurrency)
      .map((tx) => tx.bookingDate);
    return days.length > 0 ? Math.min(...days) : undefined;
  }, [history, accountCurrency]);
  const { ready: foreignRatesReady, convertAt } = useDatedFx(firstForeignDay);
  const toAccountCurrency = useCallback<AccountConvertAt>(
    (amount, from, timestamp) =>
      foreignRatesReady ? convertAt(amount, from, accountCurrency, timestamp) : null,
    [foreignRatesReady, convertAt, accountCurrency]
  );
  const historyRows = useMemo(
    () => bookedInAccountCurrency(history?.transactions ?? [], accountCurrency, toAccountCurrency),
    [history, accountCurrency, toAccountCurrency]
  );
  // Currencies named in the notes under the flows: converted, or left out for lack of a rate
  // (only once the daily rates have loaded, so loading never reads as missing)
  const foreignNote = useMemo(
    () => ({
      converted: historyRows.converted,
      missing: foreignRatesReady ? historyRows.missing : [],
    }),
    [historyRows, foreignRatesReady]
  );
  const foreignNotes = (
    <>
      {foreignNote.converted.length > 0 && (
        <p className="mt-2 text-micro text-ink-4">
          {t('readiness.foreignConverted', {
            currencies: foreignNote.converted.join(', '),
            currency: accountCurrency,
          })}
        </p>
      )}
      {foreignNote.missing.length > 0 && (
        <p className="mt-2 text-micro text-loss">
          {t('readiness.foreignMissing', { currencies: foreignNote.missing.join(', ') })}
        </p>
      )}
    </>
  );

  // ---- Sections by data (spec "Section combinations") ----
  const { records } = useBalanceRecords(accountId);
  const today = utcDayFloor(Date.now() / 1000);
  const currentMonth = monthOf(today);
  const balance = parseFloat(account?.balance || '0');
  const hasAnyTransaction = (anyTransactionResult?.total ?? 0) > 0;
  const sections = detailSections({
    balance,
    hasAnyTransaction,
    hasRecentRows: hasRecentRows(historyRows.rows, currentMonth),
  });
  const effectiveWindowEnd = windowEnd ?? currentMonth;

  // Newest booked transaction → ledger preset, once. Every currency counts (the ledger lists
  // them all), so the choice never waits for the day rates of a foreign pocket. Done during
  // render instead of in an effect: React re-renders right away (same pattern as the
  // account form dialog).
  const newestMonth = useMemo(() => newestMonthWithRows(history?.transactions ?? []), [history]);
  if (!ledgerReady && !historyLoading) {
    setLedgerReady(true);
    if (newestMonth === null) {
      applyDatePreset('thisMonth');
    } else {
      const preset = ledgerPresetForMonth(newestMonth, currentMonth);
      if (preset === 'custom') {
        setDateFrom(isoDateFromUtcTimestamp(monthStart(newestMonth)));
        setDateTo(isoDateFromUtcTimestamp(monthEnd(newestMonth)));
        setDatePreset('custom');
      } else {
        applyDatePreset(preset);
      }
    }
  }

  // Auto-categorize every uncategorized transaction matching the filters, not just this page
  const handleAutoCategorize = async () => {
    if (!accountId || !uncategorizedFilters) return;

    setIsCategorizingBatch(true);
    let uncategorized: BankTransaction[];
    let results: CategorizationResult[];
    try {
      uncategorized = (await getCompleteBankHistory(accountId, uncategorizedFilters)).transactions;
      if (uncategorized.length === 0) {
        setIsCategorizingBatch(false);
        return;
      }
      results = await categorizationApi.categorizeBatch(
        uncategorized.map((tx) => ({
          id: tx.id,
          description: tx.description ?? undefined,
          counterparty: tx.counterpartyName ?? undefined,
          counterpartyIban: tx.counterpartyIban ?? undefined,
          counterpartyBankCode: tx.counterpartyBankCode ?? undefined,
          counterpartyCountry: tx.counterpartyCountry ?? undefined,
          amount: Number(tx.amount),
          isCredit: tx.type === 'credit',
          bankAccountId: tx.bankAccountId,
        }))
      );
    } catch (error) {
      toast.error(translateApiError(error as Error, tc));
      setIsCategorizingBatch(false);
      return;
    }
    let matchCount = 0;
    const matchesToPersist: Array<{ txId: string; categoryId: string }> = [];
    uncategorized.forEach((tx, i) => {
      if (results[i] && results[i].type === 'Match') {
        matchCount++;
        matchesToPersist.push({ txId: tx.id, categoryId: results[i].data.categoryId });
      }
    });
    // Bounded chunks: a whole-history run can match thousands of rows
    const writes: PromiseSettledResult<void>[] = [];
    for (let i = 0; i < matchesToPersist.length; i += 50) {
      writes.push(
        ...(await Promise.allSettled(
          matchesToPersist
            .slice(i, i + 50)
            .map(({ txId, categoryId }) =>
              bankAccountsApi.updateTransactionCategory(txId, categoryId)
            )
        ))
      );
    }
    const succeeded = writes.filter((write) => write.status === 'fulfilled').length;
    const failed = writes.length - succeeded;
    try {
      if (succeeded) await refreshMoneyFlows(queryClient);
    } catch (error) {
      toast.error(translateApiError(error as Error, tc));
    } finally {
      setIsCategorizingBatch(false);
    }
    toast(t('readiness.autoResult', { succeeded, failed }));
    if (!matchCount) toast(t('categorization.noMatches'));
  };

  // The import dialog calls through a ref so it always runs the render with the new rows
  const autoCategorizeRef = useRef(handleAutoCategorize);
  useEffect(() => {
    autoCategorizeRef.current = handleAutoCategorize;
  });

  const uncategorizedCount = uncategorizedTotal;

  const autoCategorizeDisabledReason = isCategorizingBatch
    ? null
    : transactions.length === 0
      ? t('categorization.autoCategorizeNothingLoaded')
      : uncategorizedCount === 0
        ? t('categorization.autoCategorizeAllDone')
        : null;

  const handleCategoryChange = async (txId: string, categoryId: string | null) => {
    if (!categoryId) return;
    try {
      await bankAccountsApi.updateTransactionCategory(txId, categoryId);
      await refreshMoneyFlows(queryClient);
    } catch (error) {
      console.error('Failed to update category:', error);
      toast.error(t('messages.error'), { description: translateApiError(error as Error, tc) });
      throw error;
    }
  };

  const handleAcceptSuggestion = async (txId: string) => {
    try {
      await categorizationApi.acceptSuggestion(txId);
      toast(t('categorization.suggestionAccepted'));
      await refreshMoneyFlows(queryClient);
      queryClient.invalidateQueries({ queryKey: ['budgeting-report'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      queryClient.invalidateQueries({ queryKey: ['learnedPayees'] });
    } catch (error) {
      console.error('Failed to accept suggestion:', error);
      toast.error(t('messages.error'), { description: String(error) });
    }
  };

  const handleDeclineSuggestion = async (txId: string) => {
    try {
      await categorizationApi.declineSuggestion(txId);
      await refreshMoneyFlows(queryClient);
    } catch (error) {
      console.error('Failed to decline suggestion:', error);
    }
  };

  const pendingDeleteDescription = (() => {
    if (!pendingDelete) return '';
    const amount =
      (pendingDelete.type === 'credit' ? '+' : '−') +
      formatAmountWithCode(parseFloat(pendingDelete.amount), pendingDelete.currency, fmt.locale);
    const name = pendingDelete.counterpartyName || pendingDelete.description;
    const date = fmt.day(pendingDelete.bookingDate);
    return name
      ? t('confirmDeleteTransaction.description', { name, date, amount })
      : t('confirmDeleteTransaction.descriptionNoName', { date, amount });
  })();

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const handleEditSubmit = async (data: any, zonesInput?: any[]) => {
    const { id, ...updateData } = data;
    if (zonesInput && accountId) {
      try {
        const existingZones = await bankAccountsApi.getZones(accountId);
        for (const existingZone of existingZones) {
          if (!zonesInput.some((z) => z.id === existingZone.id)) {
            await bankAccountsApi.deleteZone(existingZone.id);
          }
        }
        for (const zone of zonesInput) {
          if (!zone.id) {
            await bankAccountsApi.createZone({
              savingsAccountId: accountId,
              fromAmount: zone.fromAmount,
              toAmount: zone.toAmount === '' ? null : (zone.toAmount ?? null),
              interestRate: zone.interestRate,
            });
          }
        }
        refetchZones();
      } catch (e) {
        console.error('Error saving zones:', e);
      }
    }
    updateAccount.mutate(
      { id, data: updateData },
      {
        onSuccess: () => {
          setEditDialogOpen(false);
          queryClient.invalidateQueries({ queryKey: ['bank-account', accountId] });
          queryClient.invalidateQueries({ queryKey: ['bank-accounts'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
          queryClient.invalidateQueries({ queryKey: ['bank-account-zones', accountId] });
        },
      }
    );
  };

  // ---- Head and top-bar status ----
  const currency = recordCurrency(account?.currency);
  const lastImport = (importBatches ?? []).reduce<number | null>(
    (max, b) => (max === null || b.importedAt > max ? b.importedAt : max),
    null
  );
  const latestTx = anyTransactionResult?.transactions[0] ?? null;

  useShellPage({
    crumb: account?.name,
    status:
      lastImport !== null
        ? { text: t('status.lastImport', { date: fmt.day(lastImport) }), tone: 'neutral' }
        : latestTx
          ? {
              text: t('status.lastMovement', { date: fmt.day(latestTx.bookingDate) }),
              tone: 'neutral',
            }
          : account?.balanceAsOf
            ? {
                text: t('status.balanceAsOf', { date: fmt.day(account.balanceAsOf) }),
                tone: 'neutral',
              }
            : { text: t('status.noData'), tone: 'neutral' },
  });

  // Sorting: a third click on the active column returns to the date order
  const handleSort = (column: SortColumn) => {
    setLedgerPage(0);
    if (sortColumn !== column) {
      setSortColumn(column);
      setSortDirection('desc');
    } else if (sortDirection === 'desc') {
      setSortDirection('asc');
    } else {
      setSortColumn('date');
      setSortDirection('desc');
    }
  };
  const SortHead = ({
    column,
    children,
    align = 'left',
    className,
  }: {
    column: SortColumn;
    children: React.ReactNode;
    align?: 'left' | 'right';
    className?: string;
  }) => {
    const active = sortColumn === column;
    const Icon = !active ? ArrowUpDown : sortDirection === 'asc' ? ArrowUp : ArrowDown;
    return (
      <TableHead className={cn(align === 'right' && 'text-right', className)}>
        <button
          type="button"
          onClick={() => handleSort(column)}
          className={cn(
            'inline-flex items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
            active && 'text-ink'
          )}
        >
          {children}
          <Icon className={cn('size-3', !active && 'opacity-60')} aria-hidden />
        </button>
      </TableHead>
    );
  };

  const dayLabel = (ts: number) => {
    const day = utcDayFloor(ts);
    if (day === today) return tc('time.today');
    if (day === today - DAY) return tc('time.yesterday');
    const sameYear = new Date(ts * 1000).getUTCFullYear() === new Date().getUTCFullYear();
    return fmt.day(
      ts,
      sameYear
        ? { day: 'numeric', month: 'short' }
        : { day: 'numeric', month: 'short', year: 'numeric' }
    );
  };

  if (isLoading) {
    return (
      <>
        <BackLink href="/bank-accounts">{t('detail.backToList')}</BackLink>
        <div className="mb-[26px]">
          <div className="skeleton h-2.5 w-32" />
          <div className="skeleton mt-3 h-9 w-72" />
          <div className="skeleton mt-3 h-3 w-96" />
        </div>
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-56" />
          <div className="skeleton mt-6 h-[180px] w-full" />
        </Card>
      </>
    );
  }

  if (!account) {
    return (
      <>
        <BackLink href="/bank-accounts">{t('detail.backToList')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      </>
    );
  }

  const accountNumber = completeAccountIdentifier(account.iban, account.bban);

  const grouped = sortColumn === 'date';
  let lastGroup = '';

  return (
    <>
      <BackLink href="/bank-accounts">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[account.institution?.name, currency, t(`accountTypes.${account.accountType}`)]}
        title={account.name}
        description={accountNumber || undefined}
        actions={
          <>
            <Button onClick={() => setRecordOpen(true)}>
              <Wallet />
              {t('actions.recordBalance')}
            </Button>
            <Button onClick={() => setCsvImportOpen(true)}>
              <Upload />
              {t('actions.importStatement')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setEditDialogOpen(true)}>
                  <Pencil />
                  {t('editAccount')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setAddTxOpen(true)}>
                  <Plus />
                  {t('transaction.add')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setHistoryOpen(true)}>
                  <History />
                  {t('menu.balanceHistory')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setImportsOpen(true)}>
                  <Receipt />
                  {t('importHistory.title')}
                </DropdownMenuItem>
                {(account.hasZoneDesignation || parseFloat(account.interestRate ?? '0') > 0) && (
                  <DropdownMenuItem onSelect={() => setInterestOpen(true)}>
                    <Percent />
                    {t('menu.interest')}
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem onSelect={() => setLocation('/settings/categorization-rules')}>
                  <ListFilter />
                  {tc('nav.categorizationRules')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setDeleteAccountOpen(true)}>
                  <Trash2 />
                  {t('deleteAccount')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      {historyError && <ReportError onRetry={() => void retryHistory()} />}
      {sections.balance && (
        <BalanceSection
          account={account}
          records={records}
          zones={zones ?? []}
          today={today}
          currency={accountCurrency}
          formatValue={formatAccountAmount}
        />
      )}
      {sections.balance && !sections.flows && foreignNotes}
      {sections.flows && (
        <div className={sections.balance ? 'mt-[17px]' : undefined}>
          <FlowsCard
            transactions={historyRows.rows}
            currency={accountCurrency}
            currentMonth={currentMonth}
            windowEnd={effectiveWindowEnd}
            onWindowEndChange={setWindowEnd}
            selectedOverride={selectedMonth}
            onSelect={setSelectedMonth}
            hero={!sections.balance}
            notes={foreignNotes}
          />
        </div>
      )}
      {!sections.balance &&
        !sections.flows &&
        !sections.ledger &&
        !historyLoading &&
        !anyTransactionLoading && (
          <EmptyState
            icon={<Wallet />}
            title={t('empty.noDataTitle')}
            description={t('empty.noDataDescription')}
          />
        )}
      {sections.ledger && (
        <div className={sections.balance || sections.flows ? 'mt-[17px]' : undefined}>
          <Card variant="table">
            <CardHeader className="flex-wrap">
              <div>
                <CardTitle>{t('transactions')}</CardTitle>
                <CardDescription>
                  {grouped ? t('ledger.subGrouped') : t('ledger.subSorted')}
                </CardDescription>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Button variant="outline" size="sm" onClick={() => setAddTxOpen(true)}>
                  <Plus />
                  {t('transaction.add')}
                </Button>
                <Segmented
                  value={kind}
                  onValueChange={(value) => {
                    setKind(value);
                    setLedgerPage(0);
                  }}
                  options={[
                    { value: 'all', label: t('filters.allTransactions') },
                    { value: 'income', label: t('transaction.credit') },
                    { value: 'outcome', label: t('transaction.debit') },
                  ]}
                />
                <Select
                  value={categoryFilter}
                  onValueChange={(value) => {
                    setCategoryFilter(value);
                    setLedgerPage(0);
                  }}
                >
                  <SelectTrigger className="h-[35px] w-[180px] text-table">
                    <SelectValue placeholder={t('filters.allCategories')} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">{t('filters.allCategories')}</SelectItem>
                    <SelectItem value="uncategorized">{t('filters.uncategorized')}</SelectItem>
                    {categories.map((cat) => (
                      <SelectItem key={cat.id} value={cat.id}>
                        {categoryName(cat)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <InputWrap icon={<Search />} className="w-60">
                  <Input
                    className="h-[35px] text-table"
                    placeholder={t('filters.searchDescription')}
                    value={searchQuery}
                    onChange={(e) => {
                      setSearchQuery(e.target.value);
                      setLedgerPage(0);
                    }}
                  />
                </InputWrap>
              </div>
            </CardHeader>
            <div className="flex flex-wrap items-center gap-2 px-[21px] pb-4">
              <Select value={datePreset} onValueChange={applyDatePreset}>
                <SelectTrigger className="h-[35px] w-[160px] text-table">
                  <SelectValue placeholder={t('filters.selectPeriod')} />
                </SelectTrigger>
                <SelectContent>
                  {[
                    'thisMonth',
                    'lastMonth',
                    'thisQuarter',
                    'lastQuarter',
                    'thisYear',
                    'lastYear',
                    'allTime',
                    'custom',
                  ].map((p) => (
                    <SelectItem key={p} value={p}>
                      {t(`filters.${p}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {datePreset === 'custom' && (
                <>
                  <Input
                    type="date"
                    value={dateFrom}
                    onChange={(e) => handleDateChange('from', e.target.value)}
                    className="h-[35px] w-[140px] text-table"
                    aria-label={t('filters.from')}
                  />
                  <span className="text-ink-5">→</span>
                  <Input
                    type="date"
                    value={dateTo}
                    onChange={(e) => handleDateChange('to', e.target.value)}
                    className="h-[35px] w-[140px] text-table"
                    aria-label={t('filters.to')}
                  />
                </>
              )}
              <div className="ml-auto">
                {autoCategorizeDisabledReason ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span tabIndex={0} className="inline-flex">
                        <Button
                          disabled
                          size="sm"
                          variant="outline"
                          className="pointer-events-none"
                        >
                          <Sparkles />
                          {t('categorization.autoCategorize')}
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>{autoCategorizeDisabledReason}</TooltipContent>
                  </Tooltip>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={handleAutoCategorize}
                    disabled={isCategorizingBatch}
                    loading={isCategorizingBatch}
                  >
                    <Sparkles />
                    {t('categorization.autoCategorize')}
                    {uncategorizedCount > 0 && <Badge variant="dark">{uncategorizedCount}</Badge>}
                  </Button>
                )}
              </div>
            </div>

            {flowReadError}
            {txError ? (
              <ReportError onRetry={() => void retryTransactions()} />
            ) : txLoading ? (
              <div className="border-t border-line p-[14px]" aria-hidden>
                {Array.from({ length: 5 }, (_, i) => (
                  <div
                    key={i}
                    className="flex items-center gap-[10px] border-b border-line-soft py-3 last:border-0"
                  >
                    <div className="skeleton size-[30px] rounded-full" />
                    <div className="flex-1">
                      <div className="skeleton h-3 w-2/5" />
                      <div className="skeleton mt-1.5 h-2.5 w-1/4" />
                    </div>
                    <div className="skeleton h-3 w-20" />
                  </div>
                ))}
              </div>
            ) : filteredTransactions.length === 0 ? (
              <div className="grid place-items-center border-t border-line px-6 py-12 text-center text-ink-3">
                <CalendarSearch className="mb-3 size-7 text-ink-5" aria-hidden />
                <h3 className="mb-1.5 text-h3 text-ink">
                  {transactions.length === 0 && hasAnyTransaction
                    ? t('empty.noTransactionsInPeriod')
                    : t('empty.noMatchingTransactionsTitle')}
                </h3>
                <p className="m-0 max-w-[320px] text-table">
                  {transactions.length === 0 && hasAnyTransaction
                    ? t('empty.noTransactionsInPeriodHint')
                    : t('empty.noMatchingTransactions')}
                </p>
                {datePreset !== 'allTime' && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-4"
                    onClick={() => applyDatePreset('allTime')}
                  >
                    {t('empty.showAll')}
                  </Button>
                )}
              </div>
            ) : (
              <Table className="table-fixed">
                <TableHeader>
                  <TableRow>
                    <SortHead column="description" className="w-[27%]">
                      {t('transaction.counterparty')}
                    </SortHead>
                    <TableHead className="w-[28%]">{t('transaction.detail')}</TableHead>
                    <SortHead column="category" className="w-[22%]">
                      {t('transaction.category')}
                    </SortHead>
                    <SortHead column="amount" align="right">
                      {t('transaction.amount')}
                    </SortHead>
                    <TableHead className="w-12">
                      <span className="sr-only">{tc('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredTransactions.map((tx) => {
                    const group = grouped ? dayLabel(tx.bookingDate) : '';
                    const showGroup = grouped && group !== lastGroup;
                    if (showGroup) lastGroup = group;
                    const credit = tx.type === 'credit';
                    const amount = Math.abs(parseFloat(tx.amount) || 0);
                    const counterparty = counterpartyLabel(tx);
                    const detail = [
                      ...transactionDetails(tx),
                      !grouped ? fmt.day(tx.bookingDate) : null,
                    ]
                      .filter(Boolean)
                      .join(' · ');
                    const flowKey = rowFlow(tx.id, credit, !!getEffectiveCategory(tx));
                    return (
                      <Fragment key={tx.id}>
                        {showGroup && (
                          <TableRow className="h-auto hover:bg-transparent">
                            <TableCell
                              colSpan={5}
                              className="bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4"
                            >
                              {group}
                            </TableCell>
                          </TableRow>
                        )}
                        <TableRow id={`tx-${tx.id}`} className="h-[52px]">
                          <TableCell>
                            <div className="flex min-w-0 items-center gap-[10px] text-ink">
                              <span className="grid size-[30px] shrink-0 place-items-center rounded-full bg-well-2 text-ink-2">
                                {credit ? (
                                  <ArrowDownLeft
                                    className="size-3.5"
                                    strokeWidth={1.75}
                                    aria-hidden
                                  />
                                ) : (
                                  <Receipt className="size-3.5" strokeWidth={1.75} aria-hidden />
                                )}
                              </span>
                              <div className="min-w-0">
                                {counterparty.title ? (
                                  <b
                                    className="block truncate text-table font-650"
                                    title={counterparty.title}
                                  >
                                    {counterparty.title}
                                  </b>
                                ) : (
                                  <span className="text-ink-4">—</span>
                                )}
                                {counterparty.subtitle && (
                                  <small
                                    className="block truncate text-micro font-500 text-ink-4 num"
                                    title={counterparty.subtitle}
                                  >
                                    {counterparty.subtitle}
                                  </small>
                                )}
                              </div>
                              {tx.status === 'pending' && (
                                <Badge className="shrink-0">{t('readiness.pending')}</Badge>
                              )}
                              {flowKey && (
                                <Badge variant="outline" className="shrink-0">
                                  {flowKey === 'refund'
                                    ? t('readiness.flowRefund')
                                    : t(`readiness.flow.${flowKey}`)}
                                </Badge>
                              )}
                            </div>
                          </TableCell>
                          <TableCell className="truncate" title={detail || undefined}>
                            {detail}
                          </TableCell>
                          <TableCell>
                            <CategorySelector
                              currentCategoryId={getEffectiveCategory(tx)}
                              transactionId={tx.id}
                              suggestedCategoryId={tx.suggestedCategoryId}
                              counterpartyName={tx.counterpartyName}
                              counterpartyIban={tx.counterpartyIban}
                              bankAccountId={tx.bankAccountId}
                              isCredit={tx.type === 'credit'}
                              counterpartyBankCode={tx.counterpartyBankCode}
                              counterpartyCountry={tx.counterpartyCountry}
                              categories={categories}
                              onCategoryChange={(catId) => handleCategoryChange(tx.id, catId)}
                              onAcceptSuggestion={() => handleAcceptSuggestion(tx.id)}
                              onDeclineSuggestion={() => handleDeclineSuggestion(tx.id)}
                              rowLabel={tx.description || tx.counterpartyName || undefined}
                              onCreateRule={() =>
                                setLocation(
                                  newRuleHref({
                                    payee: tx.counterpartyName || tx.description,
                                    categoryId: getEffectiveCategory(tx),
                                  })
                                )
                              }
                              compact
                            />
                          </TableCell>
                          <TableCell
                            className={cn(
                              'text-right font-650 num',
                              credit ? 'text-gain' : 'text-ink'
                            )}
                          >
                            {fmt.money(credit ? amount : -amount, tx.currency, { signed: true })}
                          </TableCell>
                          <TableCell className="pl-0 pr-2 text-right">
                            <div data-row-actions>
                              <DropdownMenu>
                                <DropdownMenuTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="icon-sm"
                                    aria-label={tc('labels.actions')}
                                  >
                                    <Ellipsis />
                                  </Button>
                                </DropdownMenuTrigger>
                                <DropdownMenuContent align="end">
                                  <DropdownMenuItem onSelect={() => setSelectedTransaction(tx)}>
                                    {t('readiness.transactionDetails')}
                                  </DropdownMenuItem>
                                  <DropdownMenuItem
                                    variant="danger"
                                    onSelect={() => setPendingDelete(tx)}
                                  >
                                    <Trash2 />
                                    {t('transaction.delete')}
                                  </DropdownMenuItem>
                                </DropdownMenuContent>
                              </DropdownMenu>
                            </div>
                          </TableCell>
                        </TableRow>
                      </Fragment>
                    );
                  })}
                </TableBody>
              </Table>
            )}
            <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('readiness.pageCount', {
                  shown: filteredTransactions.length,
                  total: transactionsResult?.total ?? 0,
                  page: ledgerPage + 1,
                })}
                {datePreset !== 'allTime' && datePreset !== 'custom'
                  ? ` · ${t(`filters.${datePreset}`).toLowerCase()}`
                  : ''}
              </span>
              <div className="flex gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={ledgerPage === 0 || txLoading}
                  onClick={() => setLedgerPage((page) => page - 1)}
                >
                  {t('readiness.previous')}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={(ledgerPage + 1) * 100 >= (transactionsResult?.total ?? 0) || txLoading}
                  onClick={() => setLedgerPage((page) => page + 1)}
                >
                  {t('readiness.next')}
                </Button>
              </div>
              {datePreset !== 'allTime' && (
                <button
                  type="button"
                  onClick={() => applyDatePreset('allTime')}
                  className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
                >
                  {t('empty.showAll')} →
                </button>
              )}
            </div>
          </Card>
        </div>
      )}

      <Dialog
        open={selectedTransaction !== null}
        onOpenChange={(open) => !open && setSelectedTransaction(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('readiness.transactionDetails')}</DialogTitle>
            <DialogDescription>
              {selectedTransaction?.counterpartyName || selectedTransaction?.description}
            </DialogDescription>
          </DialogHeader>
          {selectedTransaction && (
            <div className="space-y-3 text-table">
              <dl className="grid grid-cols-[1fr_1fr] gap-2 break-words">
                {[
                  ['bookingDate', fmt.day(selectedTransaction.bookingDate)],
                  [
                    'valueDate',
                    selectedTransaction.valueDate ? fmt.day(selectedTransaction.valueDate) : '—',
                  ],
                  [
                    'status',
                    t(
                      selectedTransaction.status === 'pending'
                        ? 'readiness.pending'
                        : 'readiness.booked'
                    ),
                  ],
                  ['bankId', selectedTransaction.transactionId],
                  [
                    'identifier',
                    completeAccountIdentifier(
                      selectedTransaction.counterpartyIban,
                      null,
                      selectedTransaction.counterpartyBankCode
                    ),
                  ],
                  ['originalIdentifier', selectedTransaction.counterpartyIban],
                  ['bankCode', selectedTransaction.counterpartyBankCode],
                  ['country', selectedTransaction.counterpartyCountry],
                  ['batch', selectedTransaction.importBatchId],
                  ['file', selectedTransaction.importFileName],
                  ['line', selectedTransaction.importLine],
                  [
                    'rowBalance',
                    selectedTransaction.rowBalance
                      ? fmt.money(
                          Number(selectedTransaction.rowBalance),
                          selectedTransaction.currency
                        )
                      : null,
                  ],
                  ['effectiveFlow', flowLabel(selectedTransaction.id, true)],
                ].map(([key, value]) => (
                  <Fragment key={key}>
                    <dt className="text-ink-4">{t(`readiness.detail.${key}`)}</dt>
                    <dd>{value ?? '—'}</dd>
                  </Fragment>
                ))}
              </dl>
              {flowReadError}
              <Select
                value={selectedTransaction.flowKind ?? 'auto'}
                onValueChange={async (value) => {
                  try {
                    await bankAccountsApi.updateTransactionFlow(
                      selectedTransaction.id,
                      value === 'auto' ? null : (value as FlowKind)
                    );
                    setSelectedTransaction({
                      ...selectedTransaction,
                      flowKind: value === 'auto' ? null : (value as FlowKind),
                    });
                    await refreshMoneyFlows(queryClient);
                  } catch (error) {
                    toast.error(translateApiError(error as Error, tc));
                  }
                }}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {['auto', ...FLOW_KINDS].map((value) => (
                    <SelectItem key={value} value={value}>
                      {t(`readiness.flow.${value}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </DialogContent>
      </Dialog>
      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={t('confirmDeleteTransaction.title')}
        description={pendingDeleteDescription}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteTransaction.mutate(pendingDelete.id, { onSuccess: () => setPendingDelete(null) });
        }}
        isPending={deleteTransaction.isPending}
        confirmLabel={t('transaction.delete')}
      />
      <ConfirmDeleteDialog
        open={pendingUndo !== null}
        onOpenChange={(open) => {
          if (!open) setPendingUndo(null);
        }}
        title={t(pendingUndo?.linkedCount ? 'readiness.deleteImported' : 'readiness.deleteRecord')}
        description={[
          t('readiness.deleteBatchDescription', {
            count: pendingUndo?.linkedCount ?? 0,
            file: pendingUndo?.fileName,
            period: pendingUndo?.dateRange?.from
              ? `${fmt.day(pendingUndo.dateRange.from)} – ${fmt.day(pendingUndo.dateRange.to ?? pendingUndo.dateRange.from)}`
              : t('readiness.unknownDate'),
          }),
          pendingUndo?.promotedCount
            ? t('readiness.deleteBatchPromoted', { count: pendingUndo.promotedCount })
            : null,
        ]
          .filter(Boolean)
          .join(' ')}
        onConfirm={() => pendingUndo && deleteBatchMutation.mutate(pendingUndo.id)}
        isPending={deleteBatchMutation.isPending}
        confirmLabel={t(
          pendingUndo?.linkedCount ? 'readiness.deleteImported' : 'readiness.deleteRecord'
        )}
      />
      <ConfirmDeleteDialog
        open={deleteAccountOpen}
        onOpenChange={setDeleteAccountOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.descriptionNamed', { name: account.name })}
        onConfirm={() =>
          deleteAccount.mutate(accountId!, { onSuccess: () => setLocation('/bank-accounts') })
        }
        isPending={deleteAccount.isPending}
        confirmLabel={t('deleteAccount')}
      />

      <BankAccountFormDialog
        open={editDialogOpen}
        onOpenChange={setEditDialogOpen}
        onSubmit={handleEditSubmit}
        account={account}
        isLoading={updateAccount.isPending}
        onRecordBalance={() => setRecordOpen(true)}
        initialZones={zones?.map((z) => ({
          id: z.id,
          fromAmount: z.fromAmount,
          toAmount: z.toAmount || '',
          interestRate: z.interestRate,
        }))}
      />

      <CsvImportDialog
        open={csvImportOpen}
        onOpenChange={setCsvImportOpen}
        accountId={accountId || ''}
        institutionId={account.institutionId}
        onImported={handleImported}
        onReviewUncategorized={() => setCategoryFilter('uncategorized')}
        onAutoCategorize={() => autoCategorizeRef.current()}
      />

      <AddTransactionModal
        accountId={accountId || ''}
        accountCurrency={currency}
        open={addTxOpen}
        onOpenChange={setAddTxOpen}
      />
      <BalanceRecordDialog
        open={recordOpen}
        onOpenChange={setRecordOpen}
        account={account}
        records={records}
      />
      <BalanceHistoryDialog
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        accountId={account.id}
        currency={accountCurrency}
        records={records}
      />
      <ImportHistoryDialog
        open={importsOpen}
        onOpenChange={setImportsOpen}
        batches={importBatches ?? []}
        onUndo={(batch) => {
          setImportsOpen(false);
          setPendingUndo(batch);
        }}
      />
      <InterestDialog
        open={interestOpen}
        onOpenChange={setInterestOpen}
        account={account}
        zones={zones ?? []}
        balance={balance}
        currency={accountCurrency}
      />
    </>
  );
}
