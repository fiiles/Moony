/**
 * The logic of the budget limit sheet (spec §8.7, the phone counterpart of the desktop's
 * `BudgetGoalDialog`): what the form's text means, which errors it has, what the primary button
 * says, the average spending the hint quotes and the defaults a sheet opens with. The version
 * arithmetic (what a save replaces, what is unchanged) stays in `@/utils/budget-goal-versions`.
 */
import type { BudgetGoal, BudgetingReport } from '@shared/schema';
import {
  budgetEditSource,
  intervalLastMonth,
  monthInputValue,
  monthStartFromInput,
} from '@/utils/budget-goal-versions';
import { getUtcPeriodRange } from '@/utils/period';
import { parseAmountInput } from './amount-input';

/** What the limit sheet is opened for. */
export type LimitMode =
  /** A new limit; with a category the sheet opens on the form, without one it asks first. */
  | { kind: 'add'; categoryId: string | null; effectiveMonth?: number }
  /** The limit in effect in `effectiveMonth` (else the current one) of an existing budget. */
  | { kind: 'edit'; goal: BudgetGoal; effectiveMonth?: number };

/** What the sheet's three fields hold. `from` and `until` are `YYYY-MM`; `until` may be `''` (no end). */
export interface LimitFormInput {
  amountText: string;
  from: string;
  until: string;
}

export interface LimitFormState {
  /** The limit as a number once it is a usable one (a number above zero), else `null`. */
  amount: number | null;
  /** First day (UTC epoch) of the first month the limit applies to. */
  from: number | null;
  /** First day (UTC epoch) of the last month it applies to; `null`: it runs on. */
  until: number | null;
  errors: {
    amount?: 'required' | 'invalid' | 'positive';
    from?: 'required';
    until?: 'invalid' | 'beforeStart';
  };
  valid: boolean;
}

/**
 * Reads the form: the amount as the keypad types it (`parseAmountInput`), the months as the platform's month picker returns them. An end
 * before the start is an error; the end is only compared with a start that is itself valid.
 */
export function limitFormState({ amountText, from, until }: LimitFormInput): LimitFormState {
  const errors: LimitFormState['errors'] = {};

  let amount: number | null = null;
  if (amountText.trim() === '') {
    errors.amount = 'required';
  } else {
    const parsed = parseAmountInput(amountText);
    const value = parsed === null ? NaN : Number(parsed);
    if (!Number.isFinite(value)) errors.amount = 'invalid';
    else if (value <= 0) errors.amount = 'positive';
    else amount = value;
  }

  const fromMonth = monthStartFromInput(from);
  if (fromMonth === null) errors.from = 'required';

  const untilMonth = until === '' ? null : monthStartFromInput(until);
  if (until !== '' && untilMonth === null) errors.until = 'invalid';
  else if (untilMonth !== null && fromMonth !== null && untilMonth < fromMonth) {
    errors.until = 'beforeStart';
  }

  return {
    amount,
    from: fromMonth,
    until: untilMonth,
    errors,
    valid: Object.keys(errors).length === 0,
  };
}

/**
 * The primary button's wording (the `BudgetGoalDialog` footer rule): "Přepsat a uložit" on the
 * second tap of a save that replaces limits, else "Uložit limit" while editing and "Přidat
 * rozpočet" while adding.
 */
export function limitSaveLabel(
  collisions: number,
  confirming: boolean,
  editing: boolean
): 'overwrite' | 'save' | 'add' {
  if (confirming && collisions > 0) return 'overwrite';
  return editing ? 'save' : 'add';
}

/** A category's average monthly spending over the months observed, and how many those were. */
export interface LimitAverage {
  average: number;
  months: number;
}

/**
 * The hint's averages by category id (the dialog's `averages`): each expense category's total
 * over the completed months of the report. Empty — so that no hint claims anything — when the
 * report is missing or its numbers cannot be trusted (a missing rate, an invalid amount) or when
 * no completed month was observed.
 */
export function averagesByCategory(
  report: BudgetingReport | undefined
): ReadonlyMap<string, LimitAverage> {
  const averages = new Map<string, LimitAverage>();
  if (!report) return averages;
  const { missingFxCount, invalidAmountCount, observedCompletedMonths } = report.quality;
  if (missingFxCount > 0 || invalidAmountCount > 0 || observedCompletedMonths.length === 0) {
    return averages;
  }
  const months = observedCompletedMonths.length;
  for (const category of report.expenseCategories) {
    averages.set(category.categoryId, {
      average: (parseFloat(category.totalAmount) || 0) / months,
      months,
    });
  }
  return averages;
}

/** The six completed months before this one: from the first second of the first to the last of the sixth. */
export function averageWindow(now: Date = new Date()): { start: number; end: number } {
  return {
    start: getUtcPeriodRange('monthly', -6, now).start,
    end: getUtcPeriodRange('monthly', -1, now).end,
  };
}

/**
 * A stored amount ("8000.00", "1250.50") as the plain decimal text a person would type into the
 * field ("8000", "1250.5"): no trailing zeros, no exponent, whatever the locale.
 */
export function limitAmountText(stored: string): string {
  if (!/^-?\d+(\.\d+)?$/.test(stored)) return stored;
  return stored.includes('.') ? stored.replace(/0+$/, '').replace(/\.$/, '') : stored;
}

/**
 * What a sheet opens with. Editing: the limit in effect in the edited month (`budgetEditSource`,
 * in its own currency), starting that month and ending where the edited interval ends (it keeps
 * its end unless the user changes it). Adding: an empty amount from the month the report shows, or this one, with no end.
 */
export function limitDefaults(
  goal: BudgetGoal | null,
  effectiveMonth: number | undefined,
  nowMonth: number
): LimitFormInput {
  if (goal) {
    const source = budgetEditSource(goal, effectiveMonth, nowMonth);
    const last = intervalLastMonth(goal.versions, source.month);
    const stored = limitAmountText(source.amount);
    return {
      amountText: stored,
      from: monthInputValue(source.month),
      until: last === null ? '' : monthInputValue(last),
    };
  }
  return { amountText: '', from: monthInputValue(effectiveMonth ?? nowMonth), until: '' };
}
