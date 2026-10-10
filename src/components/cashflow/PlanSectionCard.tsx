import { useTranslation } from 'react-i18next';
import type {
  CashflowCategory,
  CashflowReport,
  CashflowReportItem,
  CashflowSection,
} from '@shared/schema';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanGroupRow } from '@/components/cashflow/PlanGroupRow';
import { PlanItemRow } from '@/components/cashflow/PlanItemRow';
import { planModule } from '@/components/cashflow/plan-modules';
import { allExpanded } from '@/utils/plan-expansion';

export type SectionId = 'personal' | 'investments';
export type ItemType = 'income' | 'expense';

interface PlanSectionCardProps {
  id: SectionId;
  title: string;
  sub: string;
  section: CashflowSection;
  report: CashflowReport;
  viewType: 'monthly' | 'yearly';
  expanded: Set<string>;
  onToggleExpanded: (categoryKey: string) => void;
  onSetExpanded: (categoryKeys: string[], expanded: boolean) => void;
  onToggleInclusion: (key: string, kind: 'item' | 'category', included: boolean) => void;
  onAdd: (section: SectionId, type: ItemType, category?: string) => void;
  onReplace: (section: SectionId, type: ItemType, item: CashflowReportItem) => void;
  onEdit: (section: SectionId, item: CashflowReportItem, category: string) => void;
  onDelete: (item: CashflowReportItem) => void;
}

const FOOT_LINK =
  'text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus';

/** One planning section (personal or investments): income and expense groups, footer. */
export function PlanSectionCard({
  id,
  title,
  sub,
  section,
  report,
  viewType,
  expanded,
  onToggleExpanded,
  onSetExpanded,
  onToggleInclusion,
  onAdd,
  onReplace,
  onEdit,
  onDelete,
}: PlanSectionCardProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { money, signedMoney } = useReportMoney(report.reportCurrency);
  const nonEmpty = (cats: CashflowCategory[]) => cats.filter((c) => c.items.length > 0);
  const incomeCats = nonEmpty(section.income);
  const expenseCats = nonEmpty(section.expenses);
  const allKeys = [...incomeCats, ...expenseCats].map((c) => c.key);
  const everyOpen = allExpanded(expanded, allKeys);
  const incomeCount = section.income.reduce((n, c) => n + c.items.length, 0);
  const expenseCount = section.expenses.reduce((n, c) => n + c.items.length, 0);
  const offCount = [...section.income, ...section.expenses]
    .flatMap((c) => c.items)
    .filter((i) => !i.included).length;
  const sourceName = (item: CashflowReportItem) =>
    item.replacementSourceId
      ? (report.automaticSources.find((s) => s.sourceId === item.replacementSourceId)?.name ??
        t('readiness.retainedSource'))
      : undefined;

  const block = (label: string, cats: CashflowCategory[], isIncome: boolean, type: ItemType) => (
    <>
      <div className="border-b border-line-soft bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4">
        {label}
      </div>
      {cats.length === 0 && (
        <p className="m-0 border-b border-line-soft px-[14px] py-3.5 text-center text-table text-ink-4">
          {t(isIncome ? 'planning.table.noIncome' : 'planning.table.noExpenses')}
        </p>
      )}
      {cats.map((category) => {
        const mod = planModule(category.key);
        const module = mod ? { label: tc(mod.labelKey), url: mod.url } : undefined;
        const modules =
          module && category.items.some((i) => !i.isUserDefined) ? [module.label] : [];
        const open = expanded.has(category.key);
        return (
          <div key={category.key}>
            <PlanGroupRow
              category={category}
              isIncome={isIncome}
              expanded={open}
              reportCurrency={report.reportCurrency}
              modules={modules}
              onToggleExpanded={() => onToggleExpanded(category.key)}
              onToggleInclusion={() =>
                onToggleInclusion(`category:${category.key}`, 'category', !category.included)
              }
              onAdd={() => onAdd(id, type, category.key)}
            />
            {open &&
              category.items.map((item) => (
                <PlanItemRow
                  key={item.id}
                  item={item}
                  isIncome={isIncome}
                  categoryIncluded={category.included}
                  reportCurrency={report.reportCurrency}
                  viewType={viewType}
                  replacedSourceName={sourceName(item)}
                  module={module}
                  onToggleInclusion={() =>
                    onToggleInclusion(
                      item.isUserDefined ? `item:${item.id}` : (item.sourceId ?? `item:${item.id}`),
                      'item',
                      !item.included
                    )
                  }
                  onReplace={() => onReplace(id, type, item)}
                  onEdit={() => onEdit(id, item, category.key)}
                  onDelete={() => onDelete(item)}
                />
              ))}
          </div>
        );
      })}
    </>
  );

  const net = Number(section.netCashflow);
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
              net >= 0 ? 'text-gain' : 'text-loss'
            )}
          >
            {signedMoney(net)}
          </b>
        </div>
      </CardHeader>
      <div className="border-t border-line">
        {block(
          t('planning.table.incomeGroup', { amount: money(Number(section.totalIncome)) }),
          incomeCats,
          true,
          'income'
        )}
        {block(
          t('planning.table.expenseGroup', { amount: money(Number(section.totalExpenses)) }),
          expenseCats,
          false,
          'expense'
        )}
      </div>
      <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span>
          {t('planning.table.incomes', { count: incomeCount })} ·{' '}
          {t('planning.table.expenses', { count: expenseCount })}
          {offCount > 0 && <> · {t('planning.table.outOfPlanCount', { count: offCount })}</>}
        </span>
        <span className="flex gap-[18px]">
          {allKeys.length > 0 && (
            <button
              type="button"
              onClick={() => onSetExpanded(allKeys, !everyOpen)}
              className={FOOT_LINK}
            >
              {t(everyOpen ? 'planning.table.collapseAll' : 'planning.table.expandAll')}
            </button>
          )}
          <button type="button" onClick={() => onAdd(id, 'expense')} className={FOOT_LINK}>
            {t('planning.table.addToSection')} →
          </button>
        </span>
      </div>
    </Card>
  );
}
