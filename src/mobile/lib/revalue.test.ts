import { describe, expect, it } from 'vitest';
import { revaluedTotal, revalueData, startRevalueForm } from './revalue';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

describe('revalueData', () => {
  const form = { valueText: '5 400 000', dateIso: '2026-10-20', note: '  odhad makléře ' };
  const args = { assetId: 'p', currency: 'CZK', today: '2026-10-24' };

  it('makes the valuation: the amount as decimal text, the day as UTC seconds, the trimmed note', () => {
    expect(revalueData(form, args)).toEqual({
      ok: true,
      data: {
        assetId: 'p',
        value: '5400000',
        currency: 'CZK',
        valuedAt: day(2026, 10, 20),
        note: 'odhad makléře',
      },
    });
  });

  it('reads the amount the keypad of the system language types', () => {
    const result = revalueData({ ...form, valueText: '5400000,50' }, args);
    expect(result).toMatchObject({ ok: true, data: { value: '5400000.50' } });
  });

  it('leaves a blank note out (null), as the desktop dialog does', () => {
    expect(revalueData({ ...form, note: '   ' }, args)).toMatchObject({
      ok: true,
      data: { note: null },
    });
  });

  it('accepts today and refuses a day in the future', () => {
    expect(revalueData({ ...form, dateIso: '2026-10-24' }, args).ok).toBe(true);
    expect(revalueData({ ...form, dateIso: '2026-10-25' }, args)).toEqual({
      ok: false,
      errors: { date: 'future' },
    });
  });

  it('refuses a missing date, a missing amount, a text that is no number and a negative one', () => {
    expect(revalueData({ ...form, dateIso: '' }, args)).toEqual({
      ok: false,
      errors: { date: 'required' },
    });
    expect(revalueData({ ...form, valueText: '' }, args)).toEqual({
      ok: false,
      errors: { value: 'required' },
    });
    expect(revalueData({ ...form, valueText: 'abc' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid' },
    });
    expect(revalueData({ ...form, valueText: '-5' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid' },
    });
  });

  it('reports every problem at once', () => {
    expect(revalueData({ ...form, valueText: 'x', dateIso: '' }, args)).toEqual({
      ok: false,
      errors: { value: 'invalid', date: 'required' },
    });
  });
});

describe('startRevalueForm', () => {
  it("starts from today's estimate, today and no note", () => {
    expect(startRevalueForm(5_400_000, '2026-10-25')).toEqual({
      valueText: '5400000',
      dateIso: '2026-10-25',
      note: '',
    });
  });

  it('keeps the decimals of a per-unit price', () => {
    expect(startRevalueForm(1950.5, '2026-10-25').valueText).toBe('1950.5');
  });

  it('writes an estimate of nothing as 0', () => {
    expect(startRevalueForm(0, '2026-10-25').valueText).toBe('0');
  });
});

describe('revaluedTotal', () => {
  it('is the typed price times the units held', () => {
    expect(revaluedTotal('1 950,5', 2)).toBe(3901);
    expect(revaluedTotal('1950.5', 0.5)).toBe(975.25);
  });

  it('is zero for a price of nothing', () => {
    expect(revaluedTotal('0', 3)).toBe(0);
  });

  it('is null for an empty field, a text that is no number and a negative amount', () => {
    expect(revaluedTotal('', 2)).toBeNull();
    expect(revaluedTotal('abc', 2)).toBeNull();
    expect(revaluedTotal('-1', 2)).toBeNull();
  });
});
