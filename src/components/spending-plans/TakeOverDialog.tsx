import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Copy } from 'lucide-react';
import type { InsertSpendingPlan } from '@shared/schema';
import { takeOverRows, type TakeOverRow } from '@/utils/spending-plans';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useSpendingPlansOverview } from '@/hooks/use-spending-plans';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { PlanMoney } from '@/components/spending-plans/PlanMoney';

interface TakeOverDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  fromYear: number;
  toYear: number;
}

interface Choice {
  selected: boolean;
  /** The amount for the new year as typed. */
  amount: string;
}

/** A typed amount greater than zero, or null. */
function parseAmount(text: string): number | null {
  const value = Number(text);
  return text.trim() !== '' && Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * "Převzít plány z roku N−1" (spec §9, C6): last year's plans with what they planned and spent,
 * an editable amount for the new year, open plans preselected. A name that already exists in the
 * new year cannot be chosen. Copies name, amount, currency, icon, color and note — never payments.
 */
export function TakeOverDialog({ open, onOpenChange, fromYear, toYear }: TakeOverDialogProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { convert, currencyCode, fxReady } = useCurrency();
  const { createPlans } = useSpendingPlanMutations();
  const source = useSpendingPlansOverview(fromYear, open);
  const target = useSpendingPlansOverview(toYear, open);

  // A response of another year (kept while a query switches) is not ready
  const sourceData = source.data?.year === fromYear ? source.data : undefined;
  const targetData = target.data?.year === toYear ? target.data : undefined;
  const rows = useMemo(
    () =>
      sourceData && targetData
        ? takeOverRows(
            sourceData.plans.map((s) => s.plan),
            targetData.plans.map((s) => s.plan.name),
            fmt.locale
          )
        : [],
    [sourceData, targetData, fmt.locale]
  );
  const spentById = useMemo(
    () => new Map((sourceData?.plans ?? []).map((s) => [s.plan.id, s])),
    [sourceData]
  );

  // Only the user's changes; a row without one shows its preselection and last year's amount.
  // Opening the dialog starts over.
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) setChoices({});
  }
  const choiceOf = (r: TakeOverRow): Choice =>
    choices[r.plan.id] ?? {
      selected: r.selected,
      amount: String(parseFloat(r.plan.amount) || ''),
    };

  const chosen = rows.filter((r) => !r.exists && choiceOf(r).selected);
  const invalid = chosen.some((r) => parseAmount(choiceOf(r).amount) === null);
  // The sum of plans in several currencies, in the main currency at today's rates
  let total = 0;
  const missing = new Set<string>();
  for (const r of chosen) {
    const value = convert(parseAmount(choiceOf(r).amount) ?? 0, r.plan.currency, currencyCode);
    if (value === null && fxReady) missing.add(r.plan.currency);
    total += value ?? 0;
  }

  const setChoice = (r: TakeOverRow, patch: Partial<Choice>) =>
    setChoices((prev) => ({
      ...prev,
      [r.plan.id]: { ...choiceOf(r), ...prev[r.plan.id], ...patch },
    }));

  const submit = () => {
    const items: InsertSpendingPlan[] = chosen.map((r) => ({
      name: r.plan.name,
      year: toYear,
      amount: String(parseAmount(choiceOf(r).amount)),
      currency: r.plan.currency,
      icon: r.plan.icon,
      color: r.plan.color,
      note: r.plan.note,
    }));
    createPlans.mutate(items, {
      onSuccess: (created) => {
        toast.success(t('toast.takenOver', { count: created.length, year: fromYear }));
        onOpenChange(false);
      },
    });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] max-w-[860px] flex-col">
        <DialogHeader>
          <DialogTitle>{t('takeOver.title', { year: fromYear })}</DialogTitle>
          <DialogDescription>{t('takeOver.description')}</DialogDescription>
        </DialogHeader>
        {sourceData && targetData && rows.length === 0 ? (
          <p className="py-6 text-center text-table text-ink-3">
            {t('takeOver.empty', { year: fromYear })}
          </p>
        ) : (
          // Only the plans scroll; the header and the footer with the action stay in view
          <Table containerClassName="min-h-0 flex-1">
            <TableHeader>
              <TableRow>
                {/* 14 px padding + 16 px checkbox + 10 px air for the focus ring */}
                <TableHead className="w-10 pr-0">
                  <span className="sr-only">{t('takeOver.plan')}</span>
                </TableHead>
                <TableHead>{t('takeOver.plan')}</TableHead>
                <TableHead className="text-right">
                  {t('takeOver.plannedIn', { year: fromYear })}
                </TableHead>
                <TableHead className="text-right">
                  {t('takeOver.spentIn', { year: fromYear })}
                </TableHead>
                <TableHead className="w-[170px] text-right">
                  {t('takeOver.amountFor', { year: toYear })}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => {
                const choice = choiceOf(r);
                const selected = !r.exists && choice.selected;
                const summaryRow = spentById.get(r.plan.id);
                const spent = summaryRow?.spent ?? null;
                const over = spent !== null && parseFloat(spent) > (parseFloat(r.plan.amount) || 0);
                return (
                  <TableRow key={r.plan.id} className={cn('h-[56px]', r.exists && 'text-ink-4')}>
                    <TableCell>
                      <Checkbox
                        checked={selected}
                        disabled={r.exists}
                        aria-label={t('takeOver.select', { name: r.plan.name })}
                        onCheckedChange={(v) => setChoice(r, { selected: v === true })}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="flex min-w-0 items-center gap-[10px]">
                        <PlanChip plan={r.plan} size="sm" />
                        <div className="min-w-0">
                          <b
                            className={cn(
                              'block truncate text-table font-650',
                              r.exists ? 'text-ink-3' : 'text-ink'
                            )}
                          >
                            {r.plan.name}
                          </b>
                          <small className="block truncate text-micro font-500 text-ink-4">
                            {r.plan.closedAt !== null ? t('takeOver.closed') : t('takeOver.active')}
                            {r.exists && ` · ${t('takeOver.exists', { year: toYear })}`}
                          </small>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="text-right num">
                      {fmt.money(parseFloat(r.plan.amount) || 0, r.plan.currency)}
                    </TableCell>
                    <TableCell className={cn('text-right num', over && 'text-loss')}>
                      <PlanMoney
                        amount={spent}
                        currency={r.plan.currency}
                        quality={summaryRow?.fxQuality}
                      />
                    </TableCell>
                    <TableCell>
                      <InputWrap unit={r.plan.currency}>
                        <Input
                          type="number"
                          step="any"
                          min="0"
                          className="text-right num"
                          aria-label={t('takeOver.amountFor', { year: toYear })}
                          aria-invalid={selected && parseAmount(choice.amount) === null}
                          disabled={!selected}
                          value={choice.amount}
                          onChange={(e) => setChoice(r, { amount: e.target.value })}
                        />
                      </InputWrap>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
        <DialogFooter className="items-center sm:justify-between">
          <span
            className={cn('text-micro font-500 text-ink-4', missing.size > 0 && 'text-loss')}
            title={
              missing.size > 0
                ? tc('fxQuality.missing', {
                    currency: currencyCode,
                    currencies: [...missing].join(', '),
                  })
                : undefined
            }
          >
            {chosen.length > 0
              ? t('takeOver.summary', {
                  count: chosen.length,
                  amount: fmt.money(total, currencyCode),
                })
              : t('takeOver.nothing')}
          </span>
          <div className="flex gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="button"
              disabled={chosen.length === 0 || invalid || createPlans.isPending}
              onClick={submit}
            >
              <Copy />
              {t('takeOver.submit', { count: chosen.length })}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
