import type { BudgetGoal, BudgetGoalInterval, TransactionCategory } from '@shared/schema';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { formatReportMoney } from '@/utils/report-currencies';
import { budgetVersionPeriods } from '@/utils/budget-goal-versions';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';

export function BudgetSettingsTable({
  goals,
  categories,
  onEdit,
  onDelete,
}: {
  goals: BudgetGoal[];
  categories: TransactionCategory[];
  onEdit: (goal: BudgetGoal, effectiveMonth: number) => void;
  onDelete: (intervals: BudgetGoalInterval[]) => void;
}) {
  const { t } = useTranslation('budgeting');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const monthLabel = (value: number) => fmt.monthInSentence(new Date(value * 1000));
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const intervalKey = (interval: BudgetGoalInterval) =>
    JSON.stringify([interval.categoryId, interval.timeframe, interval.effectiveMonth]);
  const selectable = goals.flatMap((goal) =>
    goal.versions
      .filter((v) => v.amount != null)
      .map((version) => ({
        categoryId: goal.categoryId,
        timeframe: goal.timeframe,
        effectiveMonth: version.effectiveMonth,
      }))
  );
  const selected = selectable.filter((interval) => selectedKeys.has(intervalKey(interval)));
  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{t('settings.title')}</CardTitle>
          <CardDescription>{t('settings.description')}</CardDescription>
        </div>
        <Button
          variant="danger"
          size="sm"
          disabled={selected.length === 0}
          onClick={() => onDelete(selected)}
        >
          {t('settings.deleteSelected', { count: selected.length })}
        </Button>
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-10">
              <Checkbox
                aria-label={t('settings.selectAll')}
                disabled={selectable.length === 0}
                checked={
                  selected.length === 0
                    ? false
                    : selected.length === selectable.length
                      ? true
                      : 'indeterminate'
                }
                onCheckedChange={(checked) =>
                  setSelectedKeys(checked ? new Set(selectable.map(intervalKey)) : new Set())
                }
              />
            </TableHead>
            <TableHead>{t('table.category')}</TableHead>
            <TableHead>{t('settings.recurrence')}</TableHead>
            <TableHead>{t('settings.validity')}</TableHead>
            <TableHead className="text-right">{t('budgetLimit')}</TableHead>
            <TableHead />
          </TableRow>
        </TableHeader>
        <TableBody>
          {goals.flatMap((goal) => {
            const category = categories.find((c) => c.id === goal.categoryId);
            return budgetVersionPeriods(goal.versions).map(({ version, end }) => (
              <TableRow key={`${goal.id}-${version.effectiveMonth}`}>
                <TableCell>
                  {version.amount != null && (
                    <Checkbox
                      aria-label={t('settings.selectInterval', {
                        category: category ? categoryName(category) : goal.categoryId,
                        month: monthLabel(version.effectiveMonth),
                      })}
                      checked={selectedKeys.has(
                        intervalKey({
                          categoryId: goal.categoryId,
                          timeframe: goal.timeframe,
                          effectiveMonth: version.effectiveMonth,
                        })
                      )}
                      onCheckedChange={(checked) =>
                        setSelectedKeys((previous) => {
                          const next = new Set(previous);
                          const key = intervalKey({
                            categoryId: goal.categoryId,
                            timeframe: goal.timeframe,
                            effectiveMonth: version.effectiveMonth,
                          });
                          if (checked) next.add(key);
                          else next.delete(key);
                          return next;
                        })
                      }
                    />
                  )}
                </TableCell>
                <TableCell className="font-650">
                  {category ? categoryName(category) : goal.categoryId}
                </TableCell>
                <TableCell>{t(goal.timeframe)}</TableCell>
                <TableCell>
                  {end == null
                    ? t('settings.from', { month: monthLabel(version.effectiveMonth) })
                    : t('settings.between', {
                        start: monthLabel(version.effectiveMonth),
                        end: monthLabel(end),
                      })}
                </TableCell>
                <TableCell className="text-right num">
                  {version.amount == null
                    ? t('noBudget')
                    : formatReportMoney(fmt, Number(version.amount), version.currency)}
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex justify-end gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onEdit(goal, version.effectiveMonth)}
                    >
                      {t(version.amount == null ? 'actions.setLimit' : 'actions.editLimit')}
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ));
          })}
          {goals.length === 0 && (
            <TableRow>
              <TableCell colSpan={6} className="py-8 text-center text-ink-4">
                {t('noBudgets.description')}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </Card>
  );
}
