import type { Milestone } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';

/** An amount of a milestone in its own currency. */
export interface AgendaAmount {
  value: number;
  currency: string;
}

/**
 * What an agenda row says, split so that no amount or date ends in an ellipsis (audit 2026-11-15
 * C1, README §13 "Sub-lines and ellipses"): `what` is one short fact for the sub-line
 * (`mobile:agenda.what.<what>`, the distance follows it), an amount moves to the row's value. A
 * crossed target's price is the value and its target the sub; a split names its ratio and counts
 * the trades before it. `what: null` is a kind the phone has no word for: the row falls back to the
 * shared sub-line.
 */
export type AgendaRowParts =
  | { what: AgendaWhat | null; amount: AgendaAmount | null }
  | {
      what: 'watch_target_below' | 'watch_target_above';
      price: AgendaAmount | null;
      target: AgendaAmount | null;
      amount: null;
    }
  | { what: 'stock_split'; ratio: string; count: number; amount: null };

export type AgendaWhat =
  | 'insurance_anniversary'
  | 'insurance_anniversary_passed'
  | 'insurance_end'
  | 'insurance_payment'
  | 'loan_fixation_end'
  | 'loan_payoff'
  | 'bond_maturity'
  | 'bond_coupon'
  | 'account_termination'
  | 'savings_rate_end';

function amountOf(text: string | null, currency: string | null): AgendaAmount | null {
  if (text === null) return null;
  const value = Number(text);
  if (!Number.isFinite(value)) return null;
  return { value, currency: recordCurrency(currency) };
}

export function agendaRowParts(m: Milestone): AgendaRowParts {
  switch (m.kind) {
    case 'insurance_payment':
    case 'bond_coupon':
    case 'bond_maturity':
      return { what: m.kind, amount: amountOf(m.amount, m.currency) };
    case 'insurance_anniversary':
      return {
        what: m.tone === 'info' ? 'insurance_anniversary_passed' : 'insurance_anniversary',
        amount: null,
      };
    case 'insurance_end':
    case 'loan_fixation_end':
    case 'loan_payoff':
    case 'account_termination':
    case 'savings_rate_end':
      return { what: m.kind, amount: null };
    case 'watch_target':
      return {
        what: m.direction === 'below' ? 'watch_target_below' : 'watch_target_above',
        price: amountOf(m.referenceAmount, m.currency),
        target: amountOf(m.amount, m.currency),
        amount: null,
      };
    case 'stock_split':
      return { what: 'stock_split', ratio: m.direction ?? '', count: m.count ?? 0, amount: null };
    default:
      return { what: null, amount: null };
  }
}
