import { useMemo, useState, type ReactNode } from 'react';
import { Link } from 'wouter';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Pencil, Plus, Repeat, Trash2 } from 'lucide-react';
import type {
  CashflowReport,
  CashflowReportItem,
  CashflowCategory as CashflowCategoryType,
  CashflowSection,
} from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Segmented } from '@/components/ui/segmented';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { FlowBar, type FlowSegment } from '@/components/cashflow/FlowBar';
import { flowColor } from '@/components/cashflow/flow-colors';
import AddCashflowItemDialog from '@/components/cashflow/AddCashflowItemDialog';
import { cn } from '@/lib/utils';

type ViewType = 'monthly' | 'yearly';
type SectionId = 'personal' | 'investments';

/** The module an automatic item is derived from, by category key (README §11). */
const SOURCE_BY_CATEGORY: Record<string, string> = {
  personalLoans: 'nav.loans',
  investmentLoans: 'nav.loans',
  personalInsurance: 'nav.insurance',
  investmentInsurance: 'nav.insurance',
  personalRealEstateCosts: 'nav.realEstate',
  investmentRealEstateCosts: 'nav.realEstate',
  rentalIncome: 'nav.realEstate',
  stockDividends: 'nav.stocks',
  bondsInterest: 'nav.bonds',
  interestIncome: 'nav.bankAccounts',
};

/**
 * Planned cashflow (design system §7 Plan, §11): today's cashflow page,
 * renamed. Recurring items in the personal and investment sections, items
 * derived from Úvěry / Pojištění / Nemovitosti / Akcie / Dluhopisy marked as
 * automatic, a monthly/yearly switch and a 100 % flow bar instead of the Sankey.
 */
export default function CashflowPlanning() {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { formatCurrency, formatCurrencySigned } = useCurrency();
  const fmt = useFormat();
  const queryClient = useQueryClient();

  const [viewType, setViewType] = useState<ViewType>('monthly');
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [addDialogType, setAddDialogType] = useState<'income' | 'expense'>('expense');
  const [addDialogSection, setAddDialogSection] = useState<SectionId>('personal');
  const [editItem, setEditItem] = useState<CashflowReportItem | null>(null);
  const [editCategory, setEditCategory] = useState<string>('');
  const [editSection, setEditSection] = useState<SectionId>('personal');
  const [deleteItem, setDeleteItem] = useState<CashflowReportItem | null>(null);

  useShellPage({ status: { text: t('planning.status'), tone: 'neutral' } });

  const { data: report, isLoading } = useQuery<CashflowReport>({
    queryKey: ['cashflow-report', viewType],
    queryFn: () => cashflowApi.getReport(viewType),
    refetchOnMount: 'always',
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
  };

  const createMutation = useMutation({
    mutationFn: cashflowApi.createItem,
    onSuccess: () => {
      invalidate();
      setAddDialogOpen(false);
      toast(t('planning.toast.added'));
    },
    onError: (error: Error) => toast.error(tc('status.error'), { description: error.message }),
  });

  const updateMutation = useMutation({
    mutationFn: ({
      id,
      data,
    }: {
      id: string;
      data: Parameters<typeof cashflowApi.updateItem>[1];
    }) => cashflowApi.updateItem(id, data),
    onSuccess: () => {
      invalidate();
      setEditItem(null);
      setEditCategory('');
      toast(t('planning.toast.updated'));
    },
    onError: (error: Error) => toast.error(tc('status.error'), { description: error.message }),
  });

  const deleteMutation = useMutation({
    mutationFn: cashflowApi.deleteItem,
    onSuccess: () => {
      invalidate();
      setDeleteItem(null);
      toast(t('planning.toast.deleted'));
    },
    onError: (error: Error) => toast.error(tc('status.error'), { description: error.message }),
  });

  const openAdd = (section: SectionId, type: 'income' | 'expense') => {
    setAddDialogSection(section);
    setAddDialogType(type);
    setAddDialogOpen(true);
  };

  const money = (amount: number) => formatCurrency(Math.abs(amount) < 0.01 ? 0 : amount);

  const sectionOf = (id: SectionId): CashflowSection | undefined =>
    id === 'personal' ? report?.personal : report?.investments;

  const dialogCategories = (
    section: SectionId,
    type: 'income' | 'expense'
  ): CashflowCategoryType[] => {
    const s = sectionOf(section);
    if (!s) return [];
    return type === 'income' ? s.income : s.expenses;
  };
  const editType = (): 'income' | 'expense' => {
    const s = sectionOf(editSection);
    return s && s.income.some((c) => c.key === editCategory) ? 'income' : 'expense';
  };

  // Automatic expenses (derived from other modules) and the flow bar segments
  const autoExpenses = useMemo(() => {
    if (!report) return 0;
    return [report.personal, report.investments]
      .flatMap((s) => s.expenses)
      .flatMap((c) => c.items)
      .filter((i) => !i.isUserDefined)
      .reduce((sum, i) => sum + i.amount, 0);
  }, [report]);

  const flow = useMemo((): FlowSegment[] => {
    if (!report || report.totalIncome <= 0) return [];
    const byCategory = [report.personal, report.investments]
      .flatMap((s) => s.expenses)
      .filter((c) => c.total > 0)
      .map((c) => ({
        key: c.key,
        label: t(`categories.${c.key}`, { defaultValue: c.name }),
        value: c.total,
      }))
      .sort((a, b) => b.value - a.value);
    const head = byCategory.slice(0, 4);
    const rest = byCategory.slice(4);
    const segments: FlowSegment[] = head.map((c) => ({
      key: c.key,
      label: c.label,
      value: c.value,
      share: c.value / report.totalIncome,
    }));
    if (rest.length > 0) {
      const value = rest.reduce((s, c) => s + c.value, 0);
      segments.push({
        key: 'other',
        label: t('planning.flow.other'),
        value,
        share: value / report.totalIncome,
      });
    }
    if (report.netCashflow > 0) {
      segments.push({
        key: 'rest',
        label: t('planning.flow.rest'),
        value: report.netCashflow,
        share: report.netCashflow / report.totalIncome,
        kind: 'rest',
      });
    }
    return segments;
  }, [report, t]);

  const freeShare = report && report.totalIncome > 0 ? report.netCashflow / report.totalIncome : 0;

  const sectionCard = (id: SectionId, title: string, sub: string) => {
    const section = sectionOf(id);
    if (!section) return null;
    const incomeItems = section.income.flatMap((c) =>
      c.items.map((i) => ({ item: i, category: c }))
    );
    const expenseItems = section.expenses.flatMap((c) =>
      c.items.map((i) => ({ item: i, category: c }))
    );
    const row = (
      { item, category }: { item: CashflowReportItem; category: CashflowCategoryType },
      isIncome: boolean
    ) => {
      const sourceKey = !item.isUserDefined ? SOURCE_BY_CATEGORY[category.key] : undefined;
      const source = sourceKey ? tc(sourceKey) : undefined;
      return (
        <TableRow key={item.id} className="h-12">
          <TableCell>
            <b className="block text-table font-650 text-ink">{item.name}</b>
            <span className="mt-[3px] inline-flex items-center gap-[5px] text-micro font-500 text-ink-4">
              {source ? (
                <>
                  <Repeat className="size-[11px]" aria-hidden />
                  {t('planning.table.auto', { module: source })}
                </>
              ) : (
                t('planning.table.manual')
              )}
            </span>
          </TableCell>
          <TableCell>{t(`categories.${category.key}`, { defaultValue: category.name })}</TableCell>
          <TableCell className="text-ink-4">
            {item.originalFrequency === 'yearly'
              ? `${t('planning.table.perYear')} · ${fmt.money(item.originalAmount, item.originalCurrency, { decimals: 0 })}`
              : item.originalFrequency === 'monthly' && viewType === 'yearly'
                ? `${t('planning.table.perMonth')} · ${fmt.money(item.originalAmount, item.originalCurrency, { decimals: 0 })}`
                : t('planning.table.perMonth')}
          </TableCell>
          <TableCell className={cn('text-right font-650 num', isIncome ? 'text-gain' : 'text-ink')}>
            {formatCurrencySigned(isIncome ? item.amount : -item.amount)}
          </TableCell>
          <TableCell className="text-right">
            {item.isUserDefined ? (
              <div data-row-actions className="inline-flex gap-0.5">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={tc('buttons.edit')}
                  title={tc('buttons.edit')}
                  onClick={() => {
                    setEditItem(item);
                    setEditCategory(category.key);
                    setEditSection(id);
                  }}
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={tc('buttons.delete')}
                  title={tc('buttons.delete')}
                  onClick={() => setDeleteItem(item)}
                >
                  <Trash2 />
                </Button>
              </div>
            ) : (
              <Badge
                variant="outline"
                title={source ? t('planning.table.editInModule', { module: source }) : undefined}
              >
                {source}
              </Badge>
            )}
          </TableCell>
        </TableRow>
      );
    };
    const groupRow = (label: string): ReactNode => (
      <TableRow className="h-auto hover:bg-transparent">
        <TableCell
          colSpan={5}
          className="bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4"
        >
          {label}
        </TableCell>
      </TableRow>
    );
    return (
      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{title}</CardTitle>
            <CardDescription>{sub}</CardDescription>
          </div>
          <div className="text-right">
            <small className="block text-micro font-600 text-ink-4">
              {t(viewType === 'yearly' ? 'planning.table.netYearly' : 'planning.table.netMonthly')}
            </small>
            <b
              className={cn(
                'text-[16px] font-650 tracking-[-0.03em] num',
                section.netCashflow >= 0 ? 'text-gain' : 'text-loss'
              )}
            >
              {formatCurrencySigned(section.netCashflow)}
            </b>
          </div>
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[36%]">{t('planning.table.item')}</TableHead>
              <TableHead>{t('planning.table.category')}</TableHead>
              <TableHead>{t('planning.table.frequency')}</TableHead>
              <TableHead className="text-right">
                {t(viewType === 'yearly' ? 'planning.table.yearly' : 'planning.table.monthly')}
              </TableHead>
              <TableHead className="w-28">
                <span className="sr-only">{tc('labels.actions')}</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {groupRow(t('planning.table.incomeGroup', { amount: money(section.totalIncome) }))}
            {incomeItems.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={5} className="text-center text-table text-ink-4">
                  {t('planning.table.noIncome')}
                </TableCell>
              </TableRow>
            )}
            {incomeItems.map((e) => row(e, true))}
            {groupRow(t('planning.table.expenseGroup', { amount: money(section.totalExpenses) }))}
            {expenseItems.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={5} className="text-center text-table text-ink-4">
                  {t('planning.table.noExpenses')}
                </TableCell>
              </TableRow>
            )}
            {expenseItems.map((e) => row(e, false))}
          </TableBody>
        </Table>
        <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
          <span>
            {t('planning.table.incomes', { count: incomeItems.length })} ·{' '}
            {t('planning.table.expenses', { count: expenseItems.length })}
          </span>
          <button
            type="button"
            onClick={() => openAdd(id, 'expense')}
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
          >
            {t('planning.table.addToSection')} →
          </button>
        </div>
      </Card>
    );
  };

  const head = (
    <PageHead
      eyebrow={t('planning.eyebrow')}
      title={t('planning.title')}
      description={
        <>
          {t('planning.subtitle')}{' '}
          <Link
            href="/reports/cashflow"
            className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('planning.link')} →
          </Link>
        </>
      }
      actions={
        <>
          <Segmented
            size="lg"
            value={viewType}
            onValueChange={setViewType}
            options={[
              { value: 'monthly', label: t('viewType.monthly') },
              { value: 'yearly', label: t('viewType.yearly') },
            ]}
          />
          <Button onClick={() => openAdd('personal', 'expense')}>
            <Plus />
            {t('planning.addItem')}
          </Button>
        </>
      }
    />
  );

  if (isLoading || !report) {
    return (
      <>
        {head}
        <Stats columns={3}>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  return (
    <>
      {head}

      <Stats columns={3}>
        <Stat
          label={t('planning.stats.income')}
          value={money(report.totalIncome)}
          note={t('planning.stats.incomeNote', {
            personal: money(report.personal.totalIncome),
            investments: money(report.investments.totalIncome),
          })}
        />
        <Stat
          label={t('planning.stats.expenses')}
          value={money(report.totalExpenses)}
          note={t('planning.stats.expensesNote', { amount: money(autoExpenses) })}
        />
        <Stat
          label={t('planning.stats.net')}
          value={formatCurrencySigned(report.netCashflow)}
          tone={report.netCashflow > 0 ? 'gain' : report.netCashflow < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            report.netCashflow >= 0
              ? t('planning.stats.netNoteFree', { percent: fmt.percent(freeShare, 0) })
              : t('planning.stats.netNoteOver', { amount: money(-report.netCashflow) })
          }
        />
      </Stats>

      <Card className="relative mb-7 overflow-hidden px-[21px] pb-[18px] pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-['']">
        <div className="relative z-[1] flex items-center justify-between gap-6">
          <div>
            <h3 className="m-0 text-h3 text-ink">{t('planning.flow.title')}</h3>
            <p className="mt-[5px] text-micro font-500 text-ink-4">{t('planning.flow.sub')}</p>
          </div>
          {report.totalIncome > 0 && (
            <Badge>
              {t('planning.flow.badge', { percent: fmt.percent(Math.max(0, freeShare), 0) })}
            </Badge>
          )}
        </div>
        {flow.length > 0 ? (
          <div className="relative z-[1]">
            <FlowBar
              className="mb-[14px] mt-[18px]"
              segments={flow}
              formatValue={money}
              formatShare={(s) => t('planning.flow.share', { percent: fmt.percent(s, 0) })}
            />
            <div className="grid grid-cols-3 gap-x-6 gap-y-2">
              {flow.map((s, i) => (
                <div
                  key={s.key}
                  className="grid grid-cols-[8px_1fr_auto] items-center gap-[10px] border-b border-line-soft py-1.5 text-caption text-ink-2"
                >
                  <i
                    aria-hidden
                    className="size-2 rounded-[2px] border border-line-strong"
                    style={{ background: flowColor(i, s.kind) }}
                  />
                  <span>{s.label}</span>
                  <b className="font-650 text-ink num">
                    {money(s.value)} · {fmt.percent(s.share, 0)}
                  </b>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <p className="relative z-[1] mt-4 text-table text-ink-3">{t('planning.flow.none')}</p>
        )}
      </Card>

      <div className="grid gap-[18px]">
        {sectionCard('personal', t('sections.personal'), t('sections.personalDescription'))}
        {sectionCard(
          'investments',
          t('sections.investments'),
          t('sections.investmentsDescription')
        )}
      </div>

      <AddCashflowItemDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSubmit={(data) => createMutation.mutate(data)}
        itemType={addDialogType}
        categories={dialogCategories(addDialogSection, addDialogType)}
        isLoading={createMutation.isPending}
      />
      <AddCashflowItemDialog
        open={!!editItem}
        onOpenChange={(open) => {
          if (!open) {
            setEditItem(null);
            setEditCategory('');
          }
        }}
        onSubmit={(data) => editItem && updateMutation.mutate({ id: editItem.id, data })}
        editItem={editItem}
        editCategory={editCategory}
        itemType={editType()}
        categories={dialogCategories(editSection, editType())}
        isLoading={updateMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={!!deleteItem}
        onOpenChange={(open) => !open && setDeleteItem(null)}
        title={t('deleteConfirm.title')}
        description={t('deleteConfirm.descriptionNamed', { name: deleteItem?.name ?? '' })}
        onConfirm={() => deleteItem && deleteMutation.mutate(deleteItem.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('planning.deleteItem')}
      />
    </>
  );
}
