import { useState, useMemo, useCallback, useEffect, useRef, Fragment } from 'react';
import { useRoute, useLocation, useSearch } from 'wouter';
import { useQuery, useQueryClient, useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ArrowDown,
  ArrowDownLeft,
  ArrowUp,
  ArrowUpDown,
  Bot,
  CalendarSearch,
  Ellipsis,
  History,
  ListFilter,
  Pencil,
  Plus,
  Receipt,
  Search,
  Sparkles,
  Trash2,
  Undo2,
  Upload,
  X,
} from 'lucide-react';
import type { BankTransaction, CsvImportResult, SavingsAccountZone } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import { bankAccountsApi, categorizationApi } from '@/lib/tauri-api';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import {
  useBankTransactionMutations,
  useBankAccountMutations,
} from '@/hooks/use-bank-account-mutations';
import { useCategorization, type CategorizationResult } from '@/hooks/useCategorization';
import { useCategoryName } from '@/hooks/use-categories';
import { useCurrency } from '@/lib/currency';
import { useLanguage } from '@/i18n/I18nProvider';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { useShellPage } from '@/components/shell/shell-context';
import { calculateZonedInterest } from '@/utils/bank-account-zones';
import {
  CHART_PERIODS,
  chartPeriodStart,
  getUtcPeriodRange,
  isoDateFromUtcTimestamp,
  utcDayStart,
  utcDayEnd,
  type PeriodTimeframe,
} from '@/utils/period';
import { utcDayFloor } from '@/utils/chart-axis';
import { isCzechIBAN, ibanToBBAN, formatAccountNumber } from '@/utils/iban-utils';
import { formatAmountWithCode, formatAmountInCurrency } from '@/utils/format-amount';
import {
  MOVEMENT_FLOOR_BASE,
  movementThreshold,
  reconstructBalance,
  significantMovements,
  summarizeFlow,
  type MovementEvent,
} from '@/utils/bank-activity';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import TimePeriodSelector, { type Period } from '@/components/cashflow/TimePeriodSelector';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { NativeBalance } from '@/components/bank-accounts/NativeBalance';
import { CsvImportDialog } from '@/components/bank-accounts/CsvImportDialog';
import { CategorySelector } from '@/components/bank-accounts/CategorySelector';
import { newRuleHref } from '@/utils/rules-link';
import {
  MCP_CATEGORIZE_TIP_KEY,
  MCP_CATEGORIZE_TIP_LEGACY_KEY,
  dismissTip,
  hasCategorizedManually,
  isTipDismissed,
  markCategorizedManually,
} from '@/utils/tips';
import { cn } from '@/lib/utils';

type SortColumn = 'date' | 'description' | 'category' | 'amount';
type Kind = 'all' | 'income' | 'outcome';
type BalanceEvent = ChartEvent & { move: MovementEvent };

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
  const { formatCurrency, formatCurrencyRaw, convert, currencyCode } = useCurrency();
  const { getLocale } = useLanguage();
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const queryClient = useQueryClient();
  const { categorizeBatch, clearCache, isLoading: isCategorizingBatch } = useCategorization();
  const [categorizationResults, setCategorizationResults] = useState<
    Map<string, CategorizationResult>
  >(new Map());

  const [editDialogOpen, setEditDialogOpen] = useState(false);
  const deepLink = useMemo(() => new URLSearchParams(search), [search]);
  const [csvImportOpen, setCsvImportOpen] = useState(() => deepLink.get('import') === '1');
  const [importHistoryOpen, setImportHistoryOpen] = useState(false);
  const [deleteAccountOpen, setDeleteAccountOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<BankTransaction | null>(null);
  const [pendingUndo, setPendingUndo] = useState<{ id: string; count: number } | null>(null);
  const [hotEventId, setHotEventId] = useState<string | null>(null);
  const [chartPeriod, setChartPeriod] = useState<Period>('30D');

  // Ledger filters: default to the current month (UTC calendar, ADR 0008)
  // A ?category= deep link (from the list page) widens the range to everything
  const initialRange = getUtcPeriodRange('monthly', 0);
  const linkedCategory = deepLink.get('category');
  const [dateFrom, setDateFrom] = useState<string>(
    linkedCategory ? '' : isoDateFromUtcTimestamp(initialRange.start)
  );
  const [dateTo, setDateTo] = useState<string>(
    linkedCategory ? '' : isoDateFromUtcTimestamp(initialRange.end)
  );
  const [datePreset, setDatePreset] = useState<string>(linkedCategory ? 'allTime' : 'thisMonth');
  const [kind, setKind] = useState<Kind>('all');
  const [categoryFilter, setCategoryFilter] = useState<string>(linkedCategory ?? 'all');
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [sortColumn, setSortColumn] = useState<SortColumn>('date');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const applyDatePreset = (preset: string) => {
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
    if (type === 'from') setDateFrom(value);
    else setDateTo(value);
    setDatePreset('custom');
  };

  // Ledger transactions for the selected range
  const { data: transactionsResult, isLoading: txLoading } = useQuery({
    queryKey: ['bank-transactions', accountId, dateFrom, dateTo],
    queryFn: () =>
      accountId
        ? bankAccountsApi.getTransactions(accountId, {
            limit: 1000,
            dateFrom: dateFrom ? utcDayStart(dateFrom) : undefined,
            dateTo: dateTo ? utcDayEnd(dateTo) : undefined,
          })
        : null,
    enabled: !!accountId,
  });

  // Any transaction at all? Tells "nothing imported yet" from "nothing in this period".
  const { data: anyTransactionResult, isLoading: anyTransactionLoading } = useQuery({
    queryKey: ['bank-transactions', accountId, 'any'],
    queryFn: () => (accountId ? bankAccountsApi.getTransactions(accountId, { limit: 1 }) : null),
    enabled: !!accountId,
  });

  // This month and last month, for the stats and the hero delta
  const thisMonth = getUtcPeriodRange('monthly', 0);
  const lastMonth = getUtcPeriodRange('monthly', -1);
  const { data: thisMonthResult } = useQuery({
    queryKey: ['bank-transactions', accountId, 'month', thisMonth.start],
    queryFn: () =>
      accountId
        ? bankAccountsApi.getTransactions(accountId, {
            dateFrom: thisMonth.start,
            dateTo: thisMonth.end,
            limit: 5000,
          })
        : null,
    enabled: !!accountId,
  });
  const { data: lastMonthResult } = useQuery({
    queryKey: ['bank-transactions', accountId, 'month', lastMonth.start],
    queryFn: () =>
      accountId
        ? bankAccountsApi.getTransactions(accountId, {
            dateFrom: lastMonth.start,
            dateTo: lastMonth.end,
            limit: 5000,
          })
        : null,
    enabled: !!accountId,
  });

  // Transactions behind the balance chart (the chart span)
  const today = utcDayFloor(Date.now() / 1000);
  const chartFrom = chartPeriodStart(chartPeriod, today);
  const { data: chartResult } = useQuery({
    queryKey: ['bank-transactions', accountId, 'chart', chartFrom ?? 'all'],
    queryFn: () =>
      accountId
        ? bankAccountsApi.getTransactions(accountId, { dateFrom: chartFrom, limit: 5000 })
        : null,
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['import-batches', accountId] });
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
      queryClient.invalidateQueries({ queryKey: ['bank-account', accountId] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      queryClient.invalidateQueries({ queryKey: ['budgeting-report'] });
      toast(t('importHistory.undone'));
      setPendingUndo(null);
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  const transactions = useMemo(() => transactionsResult?.transactions || [], [transactionsResult]);

  const getEffectiveCategory = useCallback(
    (tx: BankTransaction) => {
      const localResult = categorizationResults.get(tx.id);
      if (localResult && localResult.type === 'Match') return localResult.data.categoryId;
      return tx.categoryId;
    },
    [categorizationResults]
  );

  const filteredTransactions = useMemo(() => {
    let filtered = transactions;
    if (kind === 'income') filtered = filtered.filter((tx) => tx.type === 'credit');
    else if (kind === 'outcome') filtered = filtered.filter((tx) => tx.type === 'debit');

    if (categoryFilter === 'uncategorized') {
      filtered = filtered.filter((tx) => !getEffectiveCategory(tx));
    } else if (categoryFilter !== 'all') {
      filtered = filtered.filter((tx) => getEffectiveCategory(tx) === categoryFilter);
    }

    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase().trim();
      filtered = filtered.filter(
        (tx) =>
          (tx.counterpartyName || '').toLowerCase().includes(q) ||
          (tx.description || '').toLowerCase().includes(q)
      );
    }

    return [...filtered].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'date':
          c = a.bookingDate - b.bookingDate;
          break;
        case 'description':
          c = (a.counterpartyName || a.description || '').localeCompare(
            b.counterpartyName || b.description || ''
          );
          break;
        case 'category':
          c = (getEffectiveCategory(a) || '').localeCompare(getEffectiveCategory(b) || '');
          break;
        case 'amount':
          c =
            parseFloat(a.amount) * (a.type === 'credit' ? 1 : -1) -
            parseFloat(b.amount) * (b.type === 'credit' ? 1 : -1);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [
    transactions,
    kind,
    categoryFilter,
    searchQuery,
    sortColumn,
    sortDirection,
    getEffectiveCategory,
  ]);

  // Account-currency amounts: transactions in another currency convert at today's rate
  const accountCurrency = (account?.currency || 'CZK') as CurrencyCode;
  const formatAccountAmount = (value: number) =>
    formatAmountInCurrency(value, accountCurrency, getLocale());

  const monthNow = useMemo(
    () => summarizeFlow(thisMonthResult?.transactions ?? []),
    [thisMonthResult]
  );
  const monthPrev = useMemo(
    () => summarizeFlow(lastMonthResult?.transactions ?? []),
    [lastMonthResult]
  );

  // Auto-categorize all uncategorized transactions of the loaded range
  const handleAutoCategorize = async () => {
    clearCache();
    const uncategorized = transactions.filter((tx) => !getEffectiveCategory(tx));
    if (uncategorized.length === 0) return;

    const results = await categorizeBatch(uncategorized);
    const newResults = new Map(categorizationResults);
    let matchCount = 0;
    const matchesToPersist: Array<{ txId: string; categoryId: string }> = [];
    uncategorized.forEach((tx, i) => {
      if (results[i] && results[i].type === 'Match') {
        newResults.set(tx.id, results[i]);
        matchCount++;
        matchesToPersist.push({ txId: tx.id, categoryId: results[i].data.categoryId });
      }
    });
    setCategorizationResults(newResults);

    if (matchesToPersist.length > 0) {
      try {
        await Promise.all(
          matchesToPersist.map(({ txId, categoryId }) =>
            bankAccountsApi.updateTransactionCategory(txId, categoryId)
          )
        );
        queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
      } catch (error) {
        console.error('Failed to persist some categories:', error);
      }
    }

    if (matchCount > 0) {
      toast(t('categorization.categorizeComplete'), {
        description: t('categorization.categorizeResult', { matches: matchCount }),
        duration: 5000,
      });
    } else {
      toast.error(t('categorization.noMatches'), {
        description: t('categorization.noMatchesDesc'),
      });
    }
  };

  // The import dialog calls through a ref so it always runs the render with the new rows
  const autoCategorizeRef = useRef(handleAutoCategorize);
  useEffect(() => {
    autoCategorizeRef.current = handleAutoCategorize;
  });

  const uncategorizedCount = transactions.filter((tx) => !getEffectiveCategory(tx)).length;

  const transactionsSettled = !txLoading && !anyTransactionLoading;
  const hasAnyTransaction = (anyTransactionResult?.total ?? 0) > 0;
  const noTransactionsAtAll =
    transactionsSettled && anyTransactionResult != null && !hasAnyTransaction;

  const autoCategorizeDisabledReason = isCategorizingBatch
    ? null
    : transactions.length === 0
      ? t('categorization.autoCategorizeNothingLoaded')
      : uncategorizedCount === 0
        ? t('categorization.autoCategorizeAllDone')
        : null;

  // the AI-assistant (MCP) hint stays out of the way until the user has
  // categorized a transaction by hand; a dismissal is remembered for good.
  const [mcpHintDismissed, setMcpHintDismissed] = useState(() =>
    isTipDismissed(MCP_CATEGORIZE_TIP_KEY, MCP_CATEGORIZE_TIP_LEGACY_KEY)
  );
  const [categorizedManually, setCategorizedManually] = useState(hasCategorizedManually);
  const dismissMcpHint = () => {
    dismissTip(MCP_CATEGORIZE_TIP_KEY);
    setMcpHintDismissed(true);
  };

  const handleCategoryChange = async (txId: string, categoryId: string | null) => {
    if (!categoryId) return;
    try {
      await bankAccountsApi.updateTransactionCategory(txId, categoryId);
      markCategorizedManually();
      setCategorizedManually(true);
      setCategorizationResults((prev) => {
        const updated = new Map(prev);
        updated.set(txId, { type: 'Match', data: { categoryId, source: { type: 'Manual' } } });
        return updated;
      });
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
    } catch (error) {
      console.error('Failed to update category:', error);
      toast.error(t('messages.error'), { description: String(error) });
    }
  };

  const handleAcceptSuggestion = async (txId: string) => {
    try {
      await categorizationApi.acceptSuggestion(txId);
      toast(t('categorization.suggestionAccepted'));
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
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
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
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

  // ---- Derived figures for the hero and the chart ----
  const balance = parseFloat(account?.balance || '0');
  const currency = account?.currency || 'CZK';
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
          : undefined,
  });

  const chartTxs = useMemo(() => chartResult?.transactions ?? [], [chartResult]);
  const chartPoints = useMemo(() => {
    const firstTx = chartTxs.reduce<number | null>(
      (min, tx) => (min === null || tx.bookingDate < min ? tx.bookingDate : min),
      null
    );
    const from = chartFrom ?? (firstTx !== null ? utcDayFloor(firstTx) : today - 30 * DAY);
    return reconstructBalance(balance, chartTxs, from, today).map((p) => ({
      t: p.t,
      value: convert(p.value, accountCurrency, currencyCode),
    }));
  }, [chartTxs, chartFrom, today, balance, convert, accountCurrency, currencyCode]);

  const floorInAccount = convert(MOVEMENT_FLOOR_BASE, 'CZK', accountCurrency);
  const threshold = movementThreshold(balance, floorInAccount);
  const events = useMemo<BalanceEvent[]>(
    () =>
      significantMovements(chartTxs, threshold).map((move) => ({
        id: move.id,
        t: move.t,
        type: move.type,
        move,
      })),
    [chartTxs, threshold]
  );

  const eventTip = (cluster: EventCluster<BalanceEvent>) => {
    if (cluster.events.length > 1) {
      return {
        title: t('chart.clusterTitle', { count: cluster.events.length }),
        lines: [
          `${fmt.day(cluster.events[0].t)} – ${fmt.day(cluster.events[cluster.events.length - 1].t)}`,
        ],
      };
    }
    const { tx } = cluster.events[0].move;
    const amount = Math.abs(Number(tx.amount) || 0);
    return {
      title: `${tx.counterpartyName || tx.description || t('table.movement')} ${fmt.money(
        tx.type === 'credit' ? amount : -amount,
        accountCurrency,
        { signed: true, decimals: 0 }
      )}`,
      lines: [fmt.day(tx.bookingDate)],
    };
  };

  const scrollToTx = (ids: string[]) => {
    const el = document.getElementById(`tx-${ids[0]}`);
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    else {
      // The row is outside the ledger's range: widen it
      applyDatePreset('allTime');
      setTimeout(
        () => document.getElementById(`tx-${ids[0]}`)?.scrollIntoView({ block: 'center' }),
        400
      );
    }
  };

  // Sorting: a third click on the active column returns to the date order
  const handleSort = (column: SortColumn) => {
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

  // Interest on this account
  let yearlyInterest = 0;
  let effectiveRate = 0;
  if (account.hasZoneDesignation && zones && zones.length > 0) {
    yearlyInterest = calculateZonedInterest(balance, zones);
    effectiveRate = balance > 0 ? (yearlyInterest / balance) * 100 : 0;
  } else if (account.interestRate) {
    const rate = parseFloat(account.interestRate);
    yearlyInterest = balance * (rate / 100);
    effectiveRate = rate;
  }

  const todayCount = (thisMonthResult?.transactions ?? []).filter(
    (tx) => utcDayFloor(tx.bookingDate) === today
  ).length;
  const accountNumber = account.bban
    ? account.bban
    : account.iban
      ? isCzechIBAN(account.iban)
        ? ibanToBBAN(account.iban)
        : formatAccountNumber(account.iban)
      : null;
  const description = [
    accountNumber,
    effectiveRate > 0
      ? t('detail.interestNote', {
          rate: fmt.percent(effectiveRate / 100, 2),
          amount: formatAccountAmount(yearlyInterest),
        })
      : null,
    todayCount > 0 ? t('detail.todayCount', { count: todayCount }) : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const monthLabel = fmt.month(new Date(thisMonth.start * 1000), { month: 'long' });
  const prevMonthLabel = fmt.month(new Date(lastMonth.start * 1000), { month: 'long' });
  const topCategory = monthNow.topExpenseCategory
    ? categories.find((c) => c.id === monthNow.topExpenseCategory!.id)
    : undefined;

  const grouped = sortColumn === 'date';
  let lastGroup = '';

  return (
    <>
      <BackLink href="/bank-accounts">{t('detail.backToList')}</BackLink>
      <PageHead
        eyebrow={[account.institution?.name, currency, t(`accountTypes.${account.accountType}`)]}
        title={account.name}
        description={description || undefined}
        actions={
          <>
            <Button variant="outline" onClick={() => setCsvImportOpen(true)}>
              <Upload />
              {t('csvImport.importCsv')}
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
                <DropdownMenuItem onSelect={() => setImportHistoryOpen((v) => !v)}>
                  <History />
                  {t('importHistory.title')}
                </DropdownMenuItem>
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
            <AddTransactionModal
              accountId={accountId || ''}
              accountCurrency={currency}
              trigger={
                <Button>
                  <Plus />
                  {t('transaction.add')}
                </Button>
              }
            />
          </>
        }
      />

      <HeroCard
        aside={
          <TimePeriodSelector
            value={chartPeriod}
            onChange={setChartPeriod}
            options={CHART_PERIODS}
          />
        }
        chart={
          <>
            <MoonyLineChart<BalanceEvent>
              className="-mx-1 mt-[18px]"
              points={chartPoints}
              height={180}
              zeroBaseline={chartPeriod === 'All'}
              formatValue={(v) => formatCurrencyRaw(v)}
              renderTip={(p) => ({
                title: formatCurrencyRaw(p.value),
                lines: [`${fmt.day(p.t)} · ${t('chart.balance').toLowerCase()}`],
              })}
              events={events}
              renderEventTip={eventTip}
              hotEventId={hotEventId}
              onHotEventChange={setHotEventId}
              onEventClick={(cluster) => scrollToTx(cluster.events.map((e) => e.id))}
            />
            <ChartLegend
              items={[
                { label: t('chart.balance'), swatch: { kind: 'line' } },
                { label: t('chart.largeIncome'), swatch: { kind: 'event', type: 'income' } },
                { label: t('chart.largeExpense'), swatch: { kind: 'event', type: 'expense' } },
              ]}
              className="items-center"
            ></ChartLegend>
            <p className="mt-1.5 text-micro font-500 text-ink-4">
              {t('chart.thresholdNote', { amount: formatAccountAmount(threshold) })}
              {chartFrom === undefined ? ` · ${t('chart.reconstructed')}` : ''}
            </p>
          </>
        }
      >
        <HeroValue
          label={t('chart.balance')}
          value={
            <NativeBalance
              amount={balance}
              currency={currency}
              align="start"
              className=""
              secondaryClassName="ml-2 text-table font-500 text-ink-4"
            />
          }
          tone={monthNow.net > 0 ? 'gain' : monthNow.net < 0 ? 'loss' : 'neutral'}
          delta={
            monthNow.count > 0
              ? `${monthNow.net >= 0 ? '↗' : '↘'} ${fmt.money(monthNow.net, accountCurrency, { signed: true, decimals: 0 })}`
              : undefined
          }
          deltaNote={
            monthNow.count > 0
              ? `${t('hero.thisMonth')} · ${
                  monthNow.net < 0
                    ? t('hero.expensesExceed')
                    : monthNow.net > 0
                      ? t('hero.incomeExceeds')
                      : t('hero.balanced')
                }`
              : t('hero.noMovementsThisMonth')
          }
        />
      </HeroCard>

      <Stats columns={3} className="mt-[17px]">
        <Stat
          label={t('stats.incomeIn', { month: monthLabel })}
          value={fmt.money(monthNow.income, accountCurrency, {
            signed: monthNow.income > 0,
            decimals: 0,
          })}
          tone={monthNow.income > 0 ? 'gain' : 'neutral'}
          noteTone="neutral"
          note={
            monthNow.incomeCount > 0
              ? t('stats.incomeCount', { count: monthNow.incomeCount })
              : t('stats.none')
          }
        />
        <Stat
          label={t('stats.expenseIn', { month: monthLabel })}
          value={fmt.money(monthNow.expense, accountCurrency, { decimals: 0 })}
          note={
            monthNow.expenseCount > 0
              ? `${t('stats.txCount', { count: monthNow.expenseCount })}${
                  topCategory
                    ? ` · ${t('stats.mostIn', { category: categoryName(topCategory) })}`
                    : ''
                }`
              : t('stats.none')
          }
        />
        <Stat
          label={t('stats.monthBalance')}
          value={fmt.money(monthNow.net, accountCurrency, { signed: true, decimals: 0 })}
          tone={monthNow.net > 0 ? 'gain' : monthNow.net < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            monthPrev.count > 0
              ? t('stats.prevMonth', {
                  month: prevMonthLabel,
                  amount: fmt.money(monthPrev.net, accountCurrency, { signed: true, decimals: 0 }),
                })
              : t('stats.noPrevMonth', { month: prevMonthLabel })
          }
        />
      </Stats>

      {account.hasZoneDesignation && zones && zones.length > 0 && (
        <Card variant="flat" className="mb-[17px]">
          <CardHeader className="pb-1.5">
            <div>
              <CardTitle className="text-h3">{t('zones.title')}</CardTitle>
              <CardDescription>
                {t('zones.effectiveNote', {
                  rate: fmt.percent(effectiveRate / 100, 2),
                  amount: formatAccountAmount(yearlyInterest),
                })}
              </CardDescription>
            </div>
          </CardHeader>
          <CardContent>
            {zones.map((zone: SavingsAccountZone) => (
              <div
                key={zone.id}
                className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0"
              >
                <span className="num">
                  {formatCurrency(
                    convertToCzK(parseFloat(zone.fromAmount || '0'), accountCurrency)
                  )}
                  {' – '}
                  {zone.toAmount
                    ? formatCurrency(convertToCzK(parseFloat(zone.toAmount), accountCurrency))
                    : t('zones.unlimited')}
                </span>
                <b className="font-600 text-ink num">
                  {fmt.percent(parseFloat(zone.interestRate || '0') / 100, 2)}
                </b>
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {categorizedManually && !mcpHintDismissed && uncategorizedCount > 0 && (
        <Alert className="mb-[17px]">
          <Bot />
          <AlertTitle>{t('categorization.mcpHint.title')}</AlertTitle>
          <AlertDescription>
            {t('categorization.mcpHint.body', { count: uncategorizedCount })}
          </AlertDescription>
          <AlertActions>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setLocation('/settings/integrations#mcp')}
            >
              {t('categorization.mcpHint.setupLink')}
            </Button>
            <Button variant="ghost" size="sm" onClick={dismissMcpHint}>
              {t('categorization.mcpHint.dismiss')}
            </Button>
          </AlertActions>
        </Alert>
      )}

      {importHistoryOpen && (
        <Card variant="table" className="mb-[17px]">
          <CardHeader>
            <CardTitle className="text-h3">{t('importHistory.title')}</CardTitle>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setImportHistoryOpen(false)}
              aria-label={tc('buttons.close')}
            >
              <X />
            </Button>
          </CardHeader>
          {importBatches && importBatches.length > 0 ? (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{tc('labels.date')}</TableHead>
                  <TableHead>{t('importHistory.fileName')}</TableHead>
                  <TableHead className="text-right">{t('importHistory.imported')}</TableHead>
                  <TableHead className="text-right" title={t('importHistory.duplicatesHelp')}>
                    {t('importHistory.duplicates')}
                  </TableHead>
                  <TableHead className="text-right">{t('importHistory.errors')}</TableHead>
                  <TableHead className="w-40">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {importBatches.map((batch) => (
                  <TableRow key={batch.id} className="h-[50px]">
                    <TableCell>{fmt.dateTime(batch.importedAt)}</TableCell>
                    <TableCell className="font-650 text-ink">{batch.fileName}</TableCell>
                    <TableCell className="text-right num">{batch.importedCount}</TableCell>
                    <TableCell className="text-right num">{batch.duplicateCount}</TableCell>
                    <TableCell className="text-right num">{batch.errorCount}</TableCell>
                    <TableCell className="text-right">
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={() => setPendingUndo({ id: batch.id, count: batch.importedCount })}
                      >
                        <Undo2 />
                        {t('importHistory.undoAction')}
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          ) : (
            <p className="border-t border-line px-[21px] py-8 text-center text-table text-ink-3">
              {t('importHistory.empty')}
            </p>
          )}
        </Card>
      )}

      {noTransactionsAtAll ? (
        <EmptyState
          icon={<Upload />}
          title={t('empty.noTransactionsTitle')}
          description={t('empty.noTransactionsDescription')}
          action={
            <div className="flex flex-wrap items-center justify-center gap-2">
              <Button onClick={() => setCsvImportOpen(true)}>
                <Upload />
                {t('empty.importCsv')}
              </Button>
              <AddTransactionModal
                accountId={accountId || ''}
                accountCurrency={currency}
                trigger={
                  <Button variant="outline">
                    <Plus />
                    {t('empty.addManually')}
                  </Button>
                }
              />
            </div>
          }
        />
      ) : (
        <Card variant="table">
          <CardHeader className="flex-wrap">
            <div>
              <CardTitle>{t('transactions')}</CardTitle>
              <CardDescription>
                {grouped ? t('ledger.subGrouped') : t('ledger.subSorted')}
              </CardDescription>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Segmented
                value={kind}
                onValueChange={setKind}
                options={[
                  { value: 'all', label: t('filters.allTransactions') },
                  { value: 'income', label: t('transaction.credit') },
                  { value: 'outcome', label: t('transaction.debit') },
                ]}
              />
              <Select value={categoryFilter} onValueChange={setCategoryFilter}>
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
                  onChange={(e) => setSearchQuery(e.target.value)}
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
            <div className="ml-auto">
              {autoCategorizeDisabledReason ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span tabIndex={0} className="inline-flex">
                      <Button disabled size="sm" variant="outline" className="pointer-events-none">
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

          {txLoading ? (
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
            <Table>
              <TableHeader>
                <TableRow>
                  <SortHead column="description" className="w-[44%]">
                    {t('transaction.description')}
                  </SortHead>
                  <SortHead column="category">{t('transaction.category')}</SortHead>
                  <SortHead column="amount" align="right">
                    {t('transaction.amount')}
                  </SortHead>
                  <TableHead className="w-10">
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
                  const title = tx.counterpartyName || tx.description || t('table.movement');
                  const sub = [
                    tx.counterpartyName ? tx.description : null,
                    tx.counterpartyIban
                      ? isCzechIBAN(tx.counterpartyIban)
                        ? ibanToBBAN(tx.counterpartyIban)
                        : formatAccountNumber(tx.counterpartyIban)
                      : null,
                    !grouped ? fmt.day(tx.bookingDate) : null,
                  ]
                    .filter(Boolean)
                    .join(' · ');
                  return (
                    <Fragment key={tx.id}>
                      {showGroup && (
                        <TableRow className="h-auto hover:bg-transparent">
                          <TableCell
                            colSpan={4}
                            className="bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4"
                          >
                            {group}
                          </TableCell>
                        </TableRow>
                      )}
                      <TableRow
                        id={`tx-${tx.id}`}
                        className="h-[52px]"
                        data-hot={hotEventId === tx.id || undefined}
                        onMouseEnter={() => setHotEventId(tx.id)}
                        onMouseLeave={() => setHotEventId(null)}
                      >
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
                              <b className="block truncate text-table font-650">{title}</b>
                              {sub && (
                                <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                                  {sub}
                                </small>
                              )}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>
                          <CategorySelector
                            currentCategoryId={getEffectiveCategory(tx)}
                            suggestedCategoryId={tx.suggestedCategoryId}
                            counterpartyName={tx.counterpartyName}
                            counterpartyIban={tx.counterpartyIban}
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
                        <TableCell className="text-right">
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
              {t('ledger.footer', { count: filteredTransactions.length })}
              {datePreset !== 'allTime' && datePreset !== 'custom'
                ? ` · ${t(`filters.${datePreset}`).toLowerCase()}`
                : ''}
            </span>
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
      )}

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
        title={t('importHistory.undoTitle')}
        description={t('importHistory.undoDescription', { count: pendingUndo?.count ?? 0 })}
        onConfirm={() => pendingUndo && deleteBatchMutation.mutate(pendingUndo.id)}
        isPending={deleteBatchMutation.isPending}
        confirmLabel={t('importHistory.undoAction')}
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
    </>
  );
}
