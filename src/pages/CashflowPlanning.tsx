import { useReportMoney } from '@/lib/use-report-money';
import { useCurrency } from '@/lib/currency';
import { ReportQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import type {
  CashflowReport,
  CashflowReportItem,
  CashflowCategory as CashflowCategoryType,
  CashflowSection,
} from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Segmented } from '@/components/ui/segmented';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { FlowBar, type FlowSegment } from '@/components/cashflow/FlowBar';
import { flowColor } from '@/components/cashflow/flow-colors';
import AddCashflowItemDialog from '@/components/cashflow/AddCashflowItemDialog';
import {
  PlanSectionCard,
  type ItemType,
  type SectionId,
} from '@/components/cashflow/PlanSectionCard';
import { useCashflowPlanningMutations } from '@/hooks/use-cashflow-planning-mutations';
import { readExpanded, setKeys, toggleKey, writeExpanded } from '@/utils/plan-expansion';

type ViewType = 'monthly' | 'yearly';

/** `localStorage` can throw on access (blocked storage); the page then simply forgets. */
function planStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Planned cashflow (design system §7 Plan, §11): recurring items in the
 * personal and investment sections, every non-empty category as a collapsible
 * group with its included total, rows automatic from Úvěry / Pojištění /
 * Nemovitosti / Akcie / Dluhopisy / Bankovní účty, any row or category can be
 * left out of the plan from its "···" menu, a monthly/yearly switch and a 100 %
 * flow bar instead of the Sankey.
 */
export default function CashflowPlanning() {
  const { t } = useTranslation('reports');
  const { currencyCode, fxRevision } = useCurrency();
  const fmt = useFormat();

  const [viewType, setViewType] = useState<ViewType>('monthly');
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [addDialogType, setAddDialogType] = useState<ItemType>('expense');
  const [addDialogSection, setAddDialogSection] = useState<SectionId>('personal');
  const [addPreset, setAddPreset] = useState<{
    category?: string;
    replacementSourceId?: string | null;
  }>({});
  const [editItem, setEditItem] = useState<CashflowReportItem | null>(null);
  const [editCategory, setEditCategory] = useState<string>('');
  const [editSection, setEditSection] = useState<SectionId>('personal');
  const [deleteItem, setDeleteItem] = useState<CashflowReportItem | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => readExpanded(planStorage()));
  const persistExpanded = (next: Set<string>) => {
    setExpanded(next);
    writeExpanded(planStorage(), next);
  };

  useShellPage({ status: { text: t('planning.status'), tone: 'neutral' } });

  const {
    data: report,
    isLoading,
    isError,
    refetch,
  } = useQuery<CashflowReport>({
    queryKey: ['cashflow-report', viewType, currencyCode, fxRevision],
    queryFn: () => cashflowApi.getReport(viewType, currencyCode),
    refetchOnMount: 'always',
  });
  const { money, signedMoney } = useReportMoney(report?.reportCurrency);
  const { createMutation, updateMutation, deleteMutation, inclusionMutation } =
    useCashflowPlanningMutations();

  const openAdd = (
    section: SectionId,
    type: ItemType,
    category?: string,
    replacementSourceId: string | null = null
  ) => {
    setAddDialogSection(section);
    setAddDialogType(type);
    setAddPreset({ category, replacementSourceId });
    setAddDialogOpen(true);
  };

  const sectionOf = (id: SectionId): CashflowSection | undefined =>
    id === 'personal' ? report?.personal : report?.investments;

  const dialogCategories = (section: SectionId, type: ItemType): CashflowCategoryType[] => {
    const s = sectionOf(section);
    if (!s) return [];
    return type === 'income' ? s.income : s.expenses;
  };
  const editType = (): ItemType => {
    const s = sectionOf(editSection);
    return s && s.income.some((c) => c.key === editCategory) ? 'income' : 'expense';
  };

  // Automatic expenses still in the plan, and how many expense rows are out of it
  const { autoExpenses, offExpenseCount } = useMemo(() => {
    if (!report) return { autoExpenses: 0, offExpenseCount: 0 };
    const items = [report.personal, report.investments]
      .flatMap((s) => s.expenses)
      .flatMap((c) => c.items);
    return {
      autoExpenses: items
        .filter((i) => !i.isUserDefined && i.included)
        .reduce((sum, i) => sum + Number(i.amount), 0),
      offExpenseCount: items.filter((i) => !i.included).length,
    };
  }, [report]);

  // Category totals already count included rows only
  const flow = useMemo((): FlowSegment[] => {
    if (!report || Number(report.totalIncome) <= 0) return [];
    const byCategory = [report.personal, report.investments]
      .flatMap((s) => s.expenses)
      .filter((c) => Number(c.total) > 0)
      .map((c) => ({
        key: c.key,
        label: t(`categories.${c.key}`, { defaultValue: c.name }),
        value: Number(c.total),
      }))
      .sort((a, b) => b.value - a.value);
    const head = byCategory.slice(0, 4);
    const rest = byCategory.slice(4);
    const segments: FlowSegment[] = head.map((c) => ({
      key: c.key,
      label: c.label,
      value: c.value,
      share: c.value / Number(report.totalIncome),
    }));
    if (rest.length > 0) {
      const value = rest.reduce((s, c) => s + c.value, 0);
      segments.push({
        key: 'other',
        label: t('planning.flow.other'),
        value,
        share: value / Number(report.totalIncome),
      });
    }
    if (Number(report.netCashflow) > 0) {
      segments.push({
        key: 'rest',
        label: t('planning.flow.rest'),
        value: Number(report.netCashflow),
        share: Number(report.netCashflow) / Number(report.totalIncome),
        kind: 'rest',
      });
    }
    return segments;
  }, [report, t]);

  const freeShare =
    report && Number(report.totalIncome) > 0
      ? Number(report.netCashflow) / Number(report.totalIncome)
      : 0;

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

  if (isError)
    return (
      <>
        {head}
        <ReportError onRetry={() => void refetch()} />
      </>
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

  const sectionProps = {
    report,
    viewType,
    expanded,
    onToggleExpanded: (key: string) => persistExpanded(toggleKey(expanded, key)),
    onSetExpanded: (keys: string[], open: boolean) =>
      persistExpanded(setKeys(expanded, keys, open)),
    onToggleInclusion: (key: string, kind: 'item' | 'category', included: boolean) =>
      inclusionMutation.mutate({ key, kind, included }),
    onAdd: (section: SectionId, type: ItemType, category?: string) =>
      openAdd(section, type, category),
    onReplace: (section: SectionId, type: ItemType, item: CashflowReportItem) =>
      openAdd(section, type, item.category, item.sourceId),
    onEdit: (section: SectionId, item: CashflowReportItem, category: string) => {
      setEditItem(item);
      setEditCategory(category);
      setEditSection(section);
    },
    onDelete: setDeleteItem,
  };

  return (
    <>
      {head}
      <ReportQualityNote quality={report.quality} planning />

      <Stats columns={3}>
        <Stat
          label={t('planning.stats.income')}
          value={money(Number(report.totalIncome))}
          note={t('planning.stats.incomeNote', {
            personal: money(Number(report.personal.totalIncome)),
            investments: money(Number(report.investments.totalIncome)),
          })}
        />
        <Stat
          label={t('planning.stats.expenses')}
          value={money(Number(report.totalExpenses))}
          note={
            offExpenseCount > 0
              ? `${t('planning.stats.expensesNote', { amount: money(autoExpenses) })} · ${t('planning.table.outOfPlanCount', { count: offExpenseCount })}`
              : t('planning.stats.expensesNote', { amount: money(autoExpenses) })
          }
        />
        <Stat
          label={t('planning.stats.net')}
          value={signedMoney(Number(report.netCashflow))}
          tone={
            Number(report.netCashflow) > 0
              ? 'gain'
              : Number(report.netCashflow) < 0
                ? 'loss'
                : 'neutral'
          }
          noteTone="neutral"
          note={
            Number(report.netCashflow) >= 0
              ? t('planning.stats.netNoteFree', { percent: fmt.percent(freeShare, 0) })
              : t('planning.stats.netNoteOver', { amount: money(-Number(report.netCashflow)) })
          }
        />
      </Stats>

      <Card className="relative mb-7 overflow-hidden px-[21px] pb-[18px] pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-['']">
        <div className="relative z-[1] flex items-center justify-between gap-6">
          <div>
            <h3 className="m-0 text-h3 text-ink">{t('planning.flow.title')}</h3>
            <p className="mt-[5px] text-micro font-500 text-ink-4">{t('planning.flow.sub')}</p>
          </div>
          {Number(report.totalIncome) > 0 && (
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
        <PlanSectionCard
          id="personal"
          title={t('sections.personal')}
          sub={t('sections.personalDescription')}
          section={report.personal}
          {...sectionProps}
        />
        <PlanSectionCard
          id="investments"
          title={t('sections.investments')}
          sub={t('sections.investmentsDescription')}
          section={report.investments}
          {...sectionProps}
        />
      </div>

      <AddCashflowItemDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSubmit={(data) =>
          createMutation.mutate(data, { onSuccess: () => setAddDialogOpen(false) })
        }
        itemType={addDialogType}
        automaticSources={report.automaticSources}
        categories={dialogCategories(addDialogSection, addDialogType)}
        presetCategory={addPreset.category}
        presetReplacementSourceId={addPreset.replacementSourceId}
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
        onSubmit={(data) =>
          editItem &&
          updateMutation.mutate(
            { id: editItem.id, data },
            {
              onSuccess: () => {
                setEditItem(null);
                setEditCategory('');
              },
            }
          )
        }
        editItem={editItem}
        editCategory={editCategory}
        itemType={editType()}
        automaticSources={report.automaticSources}
        categories={dialogCategories(editSection, editType())}
        isLoading={updateMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={!!deleteItem}
        onOpenChange={(open) => !open && setDeleteItem(null)}
        title={t('deleteConfirm.title')}
        description={t('deleteConfirm.descriptionNamed', { name: deleteItem?.name ?? '' })}
        onConfirm={() =>
          deleteItem &&
          deleteMutation.mutate(deleteItem.id, { onSuccess: () => setDeleteItem(null) })
        }
        isPending={deleteMutation.isPending}
        confirmLabel={t('planning.deleteItem')}
      />
    </>
  );
}
