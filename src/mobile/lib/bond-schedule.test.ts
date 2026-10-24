import { describe, expect, it } from 'vitest';
import type { Bond } from '@shared/schema';
import { upcomingBondPayments } from './bond-schedule';

/** A UTC day as unix seconds (ADR 0008). */
const day = (year: number, month: number, date: number) => Date.UTC(year, month - 1, date) / 1000;
const TODAY = day(2026, 10, 24);

/** 10 × 1 000 CZK at 5 % maturing 2028-03-15, unless the test says otherwise. */
const bond = (overrides: Partial<Bond> = {}): Bond => ({
  id: 'b1',
  name: 'Státní dluhopis',
  isin: null,
  couponValue: '1000',
  quantity: '10',
  currency: 'CZK',
  interestRate: '5',
  maturityDate: day(2028, 3, 15),
  purchaseDate: null,
  createdAt: 0,
  updatedAt: 0,
  ...overrides,
});

describe('upcomingBondPayments', () => {
  it('puts the coupon on the anniversary of the maturity day', () => {
    // 2026-03-15 is behind us and 2028-03-15 is past the horizon: the one coupon is 2027-03-15
    expect(upcomingBondPayments([bond()], TODAY)).toEqual([
      { t: day(2027, 3, 15), bond: bond(), kind: 'coupon', amount: 500, currency: 'CZK' },
    ]);
  });

  it('pays the principal with the last coupon when the maturity is inside the horizon', () => {
    const maturing = bond({ maturityDate: day(2027, 2, 10) });
    expect(upcomingBondPayments([maturing], TODAY)).toEqual([
      { t: day(2027, 2, 10), bond: maturing, kind: 'maturity', amount: 10_500, currency: 'CZK' },
    ]);
  });

  it('moves a 29 February coupon to the last day of February in a year without one', () => {
    const leap = bond({
      couponValue: '1000',
      quantity: '1',
      currency: 'EUR',
      interestRate: '4',
      maturityDate: day(2028, 2, 29),
    });
    expect(upcomingBondPayments([leap], TODAY)).toEqual([
      { t: day(2027, 2, 28), bond: leap, kind: 'coupon', amount: 40, currency: 'EUR' },
    ]);
  });

  it('keeps 29 February in a leap year', () => {
    const leap = bond({ quantity: '1', maturityDate: day(2032, 2, 29) });
    expect(upcomingBondPayments([leap], day(2027, 10, 24)).map((p) => p.t)).toEqual([
      day(2028, 2, 29),
    ]);
  });

  it('leaves out a repaid bond and one without a maturity', () => {
    const repaid = bond({ id: 'repaid', maturityDate: day(2026, 1, 1) });
    const open = bond({ id: 'open', maturityDate: null });
    expect(upcomingBondPayments([repaid, open], TODAY)).toEqual([]);
  });

  it('gives a rate-0 bond its maturity row (the principal is due) and no coupon rows', () => {
    const zero = bond({ quantity: '3', interestRate: '0', maturityDate: day(2027, 6, 1) });
    expect(upcomingBondPayments([zero], TODAY)).toEqual([
      { t: day(2027, 6, 1), bond: zero, kind: 'maturity', amount: 3_000, currency: 'CZK' },
    ]);
    // Out of the horizon, nothing is owed yet: a zero coupon is never a row of its own
    expect(upcomingBondPayments([{ ...zero, maturityDate: day(2029, 6, 1) }], TODAY)).toEqual([]);
  });

  it('lists the payments by day, a tie in the order the bonds came in', () => {
    const late = bond({ id: 'late', maturityDate: day(2028, 5, 1) });
    const early = bond({ id: 'early', maturityDate: day(2028, 1, 10) });
    const tieA = bond({ id: 'tieA', maturityDate: day(2028, 3, 15) });
    const tieB = bond({ id: 'tieB', maturityDate: day(2030, 3, 15), couponValue: '2000' });
    const payments = upcomingBondPayments([late, tieA, early, tieB], TODAY);
    expect(payments.map((p) => [p.bond.id, p.t])).toEqual([
      ['early', day(2027, 1, 10)],
      ['tieA', day(2027, 3, 15)],
      ['tieB', day(2027, 3, 15)],
      ['late', day(2027, 5, 1)],
    ]);
  });

  it('lists a payment due today and ends the window on the day a year ahead', () => {
    const dueToday = bond({ id: 'today', maturityDate: TODAY });
    const dueBeforeEnd = bond({ id: 'before', maturityDate: day(2027, 10, 23) });
    const dueAtEnd = bond({ id: 'end', maturityDate: day(2027, 10, 24) });
    // The coupon of 'end' falls today (an anniversary of its maturity); its maturity is the end
    expect(
      upcomingBondPayments([dueAtEnd, dueBeforeEnd, dueToday], TODAY).map((p) => [
        p.bond.id,
        p.kind,
        p.t,
      ])
    ).toEqual([
      ['end', 'coupon', TODAY],
      ['today', 'maturity', TODAY],
      ['before', 'maturity', day(2027, 10, 23)],
    ]);
  });

  it('shows a yearly coupon once: the day a year ahead is outside the window', () => {
    const yearly = bond({ maturityDate: day(2029, 10, 24) });
    expect(upcomingBondPayments([yearly], TODAY).map((p) => p.t)).toEqual([TODAY]);
  });

  it('counts the months from the day, clamped at the end of a month', () => {
    // 30 November + 3 months is 28 February (not 2 March): the window ends there
    const today = day(2026, 11, 30);
    const inside = bond({ id: 'in', maturityDate: day(2027, 2, 27) });
    const atEnd = bond({ id: 'edge', maturityDate: day(2027, 2, 28) });
    const outside = bond({ id: 'out', maturityDate: day(2027, 3, 1) });
    const ids = (months: number) =>
      upcomingBondPayments([outside, atEnd, inside], today, months).map((p) => p.bond.id);
    expect(ids(3)).toEqual(['in']);
    expect(ids(4)).toEqual(['in', 'edge', 'out']);
  });
});
