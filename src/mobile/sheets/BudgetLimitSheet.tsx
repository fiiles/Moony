import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { TransactionCategory } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useCategoryName } from '@/hooks/use-categories';
import { useCurrency } from '@/lib/currency';
import { budgetingApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useSaveBudgetGoal } from '@/hooks/use-budget-goal-mutations';
import { AmountField } from '@/mobile/components/AmountField';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { MonthField } from '@/mobile/components/cashflow/MonthField';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { Sheet } from '@/mobile/components/Sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  limitDefaults,
  limitFormState,
  limitSaveLabel,
  type LimitAverage,
  type LimitFormInput,
  type LimitMode,
} from '@/mobile/lib/budget-limit';
import {
  budgetEditSource,
  currentMonthStart,
  intervalLastMonth,
  isUnchangedGoalEdit,
  overwrittenPeriods,
  scheduledVersions,
  versionInEffect,
} from '@/utils/budget-goal-versions';
import { preserveMoneyText } from '@/utils/money-flows';
import { formatReportMoney } from '@/utils/report-currencies';

export type { LimitMode };

interface BudgetLimitSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Set together with `open`; the sheet keeps the last one while it slides away. */
  mode: LimitMode | null;
  /** Every category, to name the one being edited. */
  categories: readonly TransactionCategory[];
  /** The expense categories that have no limit yet (the first step of an addition). */
  availableCategories: readonly TransactionCategory[];
  /** Average monthly spending by category id (`averagesByCategory`); empty: no hint. */
  averages: ReadonlyMap<string, LimitAverage>;
  /** The currency the averages are in. */
  averageCurrency: string;
}

/** The category the sheet starts on: the edited one, the preselected one, or none (the pick step). */
function initialCategoryId(mode: LimitMode): string | null {
  return mode.kind === 'edit' ? mode.goal.categoryId : mode.categoryId;
}

/**
 * "Přidat rozpočet" / "Upravit limit" (spec §8.7, the phone counterpart of the desktop's
 * `BudgetGoalDialog`): the monthly limit of a category from one month to another. One sheet with
 * two steps inside it, so that Android back has a single entry to take off: an addition without
 * a category first lists the categories that have no limit, and a tap moves on to the form. The
 * form's logic lives in `budget-limit.ts` and `budget-goal-versions.ts`; this renders it.
 *
 * It keeps the mode it was last opened with while it slides away, and every opening starts over
 * from the defaults of that mode.
 */
export function BudgetLimitSheet({ mode, ...props }: BudgetLimitSheetProps) {
  const [shown, setShown] = useState(mode);
  if (mode !== null && mode !== shown) setShown(mode);
  if (shown === null) return null;
  return <LimitSheet mode={shown} {...props} />;
}

function LimitSheet({
  open,
  onOpenChange,
  mode,
  categories,
  availableCategories,
  averages,
  averageCurrency,
}: BudgetLimitSheetProps & { mode: LimitMode }) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const money = useMoneyText();
  const categoryName = useCategoryName();
  const { currencyCode } = useCurrency();

  const goal = mode.kind === 'edit' ? mode.goal : null;
  const editing = goal !== null;
  const timeframe = goal?.timeframe ?? 'monthly';
  const nowMonth = currentMonthStart();
  // Moving the effective month moves the selected amount; the source keeps the edited interval's own
  const editSource = goal ? budgetEditSource(goal, mode.effectiveMonth, nowMonth) : null;
  const editorCurrency = editSource?.currency ?? currencyCode;

  const [categoryId, setCategoryId] = useState(() => initialCategoryId(mode));
  const [input, setInput] = useState<LimitFormInput>(() =>
    limitDefaults(goal, mode.effectiveMonth, nowMonth)
  );
  // The first tap on save lists what the save replaces; the second saves
  const [confirming, setConfirming] = useState(false);
  // An empty amount is only called missing once the field has been left
  const [amountTouched, setAmountTouched] = useState(false);

  // Every opening starts over
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setCategoryId(initialCategoryId(mode));
      setInput(limitDefaults(goal, mode.effectiveMonth, nowMonth));
      setConfirming(false);
      setAmountTouched(false);
    }
  }

  const edit = (patch: Partial<LimitFormInput>) => {
    setInput((current) => ({ ...current, ...patch }));
    setConfirming(false);
  };

  // Every stored limit of the category, so a save can say what it replaces (a new budget may still
  // collide with limits that have run out)
  const { data: history = [] } = useQuery({
    queryKey: ['budget-goal-history'],
    queryFn: () => budgetingApi.getBudgetGoalHistory(),
    enabled: open,
  });
  const versions =
    history.find((g) => g.categoryId === categoryId && g.timeframe === timeframe)?.versions ??
    goal?.versions ??
    [];

  const form = limitFormState(input);
  // The save replaces everything in its range; without an end nothing later stays
  const scheduled = form.until === null ? [] : scheduledVersions(versions, form.until);
  const editedInterval = (() => {
    if (!editSource) return null;
    const inEffect = versionInEffect(versions, editSource.month);
    return inEffect
      ? { start: inEffect.effectiveMonth, end: intervalLastMonth(versions, editSource.month) }
      : null;
  })();
  const collisions =
    form.from !== null && form.amount !== null
      ? overwrittenPeriods(
          versions,
          form.from,
          form.until,
          form.amount,
          editorCurrency,
          editedInterval
        )
      : [];
  const showCollisions = confirming && collisions.length > 0;

  // The list of what is replaced sits under the fields: bring it into view when it appears
  const alertRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (showCollisions) alertRef.current?.scrollIntoView?.({ block: 'nearest' });
  }, [showCollisions]);

  // Moving from the list to the form starts at the top of the sheet's body (the form is shorter).
  // `Sheet` wraps its children in the scrolling body `div` (Sheet.tsx), so that is `bodyRef`'s parent.
  const bodyRef = useRef<HTMLDivElement>(null);
  const picking = categoryId === null;
  useEffect(() => {
    bodyRef.current?.parentElement?.scrollTo?.({ top: 0 });
  }, [picking]);

  const mutation = useSaveBudgetGoal(() => onOpenChange(false), editing);

  const submit = () => {
    // A second Enter or "Done" key while the save runs must not save twice (`loading` only blocks taps)
    if (mutation.isPending) return;
    if (!form.valid || form.amount === null || form.from === null || categoryId === null) return;
    const { amount, from, until } = form;
    // Saving the limit already in effect for that month changes nothing
    if (goal && isUnchangedGoalEdit(versions, from, amount, editorCurrency, until)) {
      toast.info(tb('readiness.noChange'));
      onOpenChange(false);
      return;
    }
    if (collisions.length > 0 && !confirming) {
      setConfirming(true);
      return;
    }
    mutation.mutate({
      categoryId,
      timeframe,
      amount: preserveMoneyText(
        editSource?.amount ?? null,
        editSource?.currency ?? null,
        amount,
        editorCurrency
      ),
      currency: editorCurrency,
      effectiveMonth: from,
      effectiveUntil: until ?? undefined,
    });
  };

  const monthLabel = (month: number) => fmt.monthInSentence(new Date(month * 1000));
  const reportMoney = (amount: number, currency: string) =>
    formatReportMoney({ money }, amount, currency);
  const periodLabel = (start: number, end: number | null) => {
    if (end === null) return tb('settings.from', { month: monthLabel(start) });
    const first = monthLabel(start);
    const last = monthLabel(end);
    return first === last ? first : tb('settings.between', { start: first, end: last });
  };

  const category = categories.find((c) => c.id === categoryId);
  const name = category ? categoryName(category) : (categoryId ?? '');

  // The average is over the months the report observed, which may be fewer than six: say how many
  const observed = categoryId === null ? undefined : averages.get(categoryId);
  const averageHint =
    averages.size === 0
      ? undefined
      : observed && observed.average > 0
        ? tb('readiness.observedAverage', {
            amount: reportMoney(observed.average, averageCurrency),
            count: observed.months,
          })
        : tb('dialog.noAverage');

  const amountError =
    form.errors.amount === undefined || (form.errors.amount === 'required' && !amountTouched)
      ? undefined
      : form.errors.amount === 'required'
        ? tc('validation.amountRequired')
        : form.errors.amount === 'invalid'
          ? tc('validation.invalidAmount')
          : tc('validation.amountPositive');
  const fromError = form.errors.from ? tc('validation.dateRequired') : undefined;
  const untilError =
    form.errors.until === 'invalid'
      ? tc('validation.dateRequired')
      : form.errors.until === 'beforeStart'
        ? tc('validation.effectiveUntilBeforeStart')
        : undefined;

  const saveLabel = {
    overwrite: tb('readiness.overwriteConfirm'),
    save: tb('dialog.saveLimit'),
    add: tb('dialog.addTitle'),
  }[limitSaveLabel(collisions.length, confirming, editing)];

  const cancel = (
    <Button
      type="button"
      variant="ghost"
      className={
        picking
          ? 'h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row'
          : 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row'
      }
      onClick={() => onOpenChange(false)}
    >
      {tc('buttons.cancel')}
    </Button>
  );

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={editing ? tb('dialog.editTitle') : tb('dialog.addTitle')}
      description={
        picking
          ? t('cashflow.limitSheet.pick')
          : editing
            ? tb('dialog.editDescription', { category: name })
            : tb('dialog.addDescription')
      }
      full
      footer={
        picking ? (
          cancel
        ) : (
          <>
            {cancel}
            <Button
              type="submit"
              form="budget-limit"
              className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
              disabled={!form.valid}
              loading={mutation.isPending}
            >
              {saveLabel}
            </Button>
          </>
        )
      }
    >
      <div ref={bodyRef}>
        {picking ? (
          <EntityRows>
            {availableCategories.map((c) => (
              <EntityRow
                key={c.id}
                chip={<CategoryChip category={c} />}
                title={categoryName(c)}
                chevron
                onClick={() => setCategoryId(c.id)}
              />
            ))}
          </EntityRows>
        ) : (
          <form
            id="budget-limit"
            noValidate
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <EntityRows className="mb-3">
              <EntityRow chip={<CategoryChip category={category} />} title={name} />
            </EntityRows>
            <AmountField
              label={tb('dialog.monthlyLimit')}
              unit={editorCurrency}
              value={input.amountText}
              placeholder="0"
              error={amountError}
              hint={averageHint}
              onChange={(event) => edit({ amountText: event.target.value })}
              onBlur={() => setAmountTouched(true)}
            />
            <div className="mb-2 grid grid-cols-2 gap-3">
              <MonthField
                label={tb('readiness.effectiveMonth')}
                value={input.from}
                onChange={(from) => edit({ from })}
                error={fromError}
              />
              <MonthField
                label={tb('readiness.effectiveUntil')}
                value={input.until}
                onChange={(until) => edit({ until })}
                error={untilError}
                action={
                  input.until !== ''
                    ? { label: tb('readiness.openEnded'), onClick: () => edit({ until: '' }) }
                    : undefined
                }
              />
            </div>
            <p className="mx-0.5 mb-3 text-m-caption text-ink-3">
              {tb(form.until === null ? 'readiness.rangeOpenHelp' : 'readiness.rangeClosedHelp')}
            </p>
            {scheduled.length > 0 && (
              <div className="mb-3 rounded-r2 border border-line bg-well px-3 py-2.5 text-m-sub text-ink-3">
                <p className="m-0 mb-1 font-semibold text-ink-2">
                  {tb('readiness.scheduledHeading')}
                </p>
                <ul className="m-0 list-none space-y-0.5 p-0">
                  {scheduled.map((version) => (
                    <li key={version.effectiveMonth}>
                      {version.amount === null
                        ? tb('readiness.scheduledRemoval', {
                            month: monthLabel(version.effectiveMonth),
                          })
                        : tb('readiness.scheduledLimit', {
                            month: monthLabel(version.effectiveMonth),
                            amount: reportMoney(Number(version.amount), version.currency),
                          })}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {showCollisions && (
              <div
                ref={alertRef}
                role="alert"
                className="mb-3 rounded-r2 border border-loss-line bg-loss-soft px-3 py-2.5 text-m-sub text-loss"
              >
                <p className="m-0 mb-1 font-semibold">{tb('readiness.overwriteTitle')}</p>
                <ul className="m-0 list-none space-y-0.5 p-0">
                  {collisions.map(({ version, end }) => (
                    <li key={version.effectiveMonth}>
                      {periodLabel(
                        version.effectiveMonth,
                        end === null ? null : intervalLastMonth(versions, version.effectiveMonth)
                      )}
                      {': '}
                      {reportMoney(Number(version.amount), version.currency)}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </form>
        )}
      </div>
    </Sheet>
  );
}
