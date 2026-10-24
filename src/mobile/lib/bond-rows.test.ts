import { describe, expect, it } from 'vitest';
import type { Bond } from '@shared/schema';
import { bondRows } from './bond-rows';

const DAY = 86_400;
const day = (year: number, month: number, date: number) => Date.UTC(year, month - 1, date) / 1000;
const TODAY = day(2026, 10, 24);

const bond = (overrides: Partial<Bond> = {}): Bond => ({
  id: 'b1',
  name: 'Státní dluhopis',
  isin: 'CZ0001005037',
  couponValue: '1000',
  quantity: '10',
  currency: 'CZK',
  interestRate: '5.25',
  maturityDate: day(2028, 3, 15),
  purchaseDate: null,
  createdAt: 0,
  updatedAt: 0,
  ...overrides,
});

describe('bondRows', () => {
  it('reads the numbers of a bond in its own currency', () => {
    const [row] = bondRows(
      [bond({ currency: 'EUR', couponValue: '99.5', quantity: '3' })],
      TODAY,
      false
    );
    expect(row).toMatchObject({
      face: 298.5,
      currency: 'EUR',
      rate: 5.25,
      quantity: 3,
      unitFace: 99.5,
      maturity: day(2028, 3, 15),
      badge: null,
    });
    expect(row.bond.isin).toBe('CZ0001005037');
  });

  it('badges a maturity within 365 days "withinYear", the 365th day included', () => {
    const badge = (days: number) =>
      bondRows([bond({ maturityDate: TODAY + days * DAY })], TODAY, false)[0].badge;
    expect(badge(0)).toBe('withinYear');
    expect(badge(365)).toBe('withinYear');
    expect(badge(366)).toBeNull();
  });

  it('badges a repaid bond "matured" and a bond without a maturity with nothing', () => {
    const rows = bondRows(
      [bond({ id: 'open', maturityDate: null }), bond({ id: 'repaid', maturityDate: TODAY - DAY })],
      TODAY,
      true
    );
    expect(Object.fromEntries(rows.map((r) => [r.bond.id, r.badge]))).toEqual({
      open: null,
      repaid: 'matured',
    });
    expect(rows.find((r) => r.bond.id === 'open')?.maturity).toBeNull();
  });

  it('sorts by maturity, the nearest first and the open-ended last', () => {
    const rows = bondRows(
      [
        bond({ id: 'open', maturityDate: null }),
        bond({ id: 'far', maturityDate: day(2031, 1, 1) }),
        bond({ id: 'near', maturityDate: day(2027, 1, 1) }),
        bond({ id: 'open2', maturityDate: null }),
      ],
      TODAY,
      false
    );
    // Two open-ended bonds keep the order they came in
    expect(rows.map((r) => r.bond.id)).toEqual(['near', 'far', 'open', 'open2']);
  });

  it('leaves repaid bonds out unless asked, and then puts them first by their day', () => {
    const bonds = [
      bond({ id: 'live', maturityDate: day(2027, 1, 1) }),
      bond({ id: 'repaid', maturityDate: day(2026, 1, 1) }),
    ];
    expect(bondRows(bonds, TODAY, false).map((r) => r.bond.id)).toEqual(['live']);
    expect(bondRows(bonds, TODAY, true).map((r) => r.bond.id)).toEqual(['repaid', 'live']);
  });

  it('keeps a bond that matures today among the live ones', () => {
    const rows = bondRows([bond({ maturityDate: TODAY })], TODAY, false);
    expect(rows).toHaveLength(1);
    expect(rows[0].badge).toBe('withinYear');
  });
});
