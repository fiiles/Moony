import { describe, expect, it } from 'vitest';
import type { StockDividend } from '@shared/schema';
import { parseAmountInput } from './amount-input';
import { dividendErrors, dividendForm, dividendInput } from './dividend-form';
import { isoToUtcDay } from './property-detail';

const blank = dividendForm(null, '2026-11-07');

const stored: StockDividend = {
  id: 'd1',
  investmentId: 'i1',
  source: 'manual',
  exDate: isoToUtcDay('2026-03-14'),
  amountPerShare: null,
  shares: null,
  grossAmount: '12.50',
  currency: 'USD',
  note: 'Q1',
  superseded: false,
};

describe('dividendForm', () => {
  it('starts a new one today with empty fields', () => {
    expect(blank).toEqual({ dateIso: '2026-11-07', amountText: '', note: '' });
  });

  it('takes the stored values of an edit', () => {
    expect(dividendForm(stored, '2026-11-07')).toEqual({
      dateIso: '2026-03-14',
      amountText: '12.5',
      note: 'Q1',
    });
    expect(dividendForm({ ...stored, note: null }, '2026-11-07').note).toBe('');
  });
});

describe('dividendErrors', () => {
  it('names an empty, unreadable and zero amount', () => {
    expect(dividendErrors(blank)).toEqual({ amount: 'required' });
    expect(dividendErrors({ ...blank, amountText: 'abc' })).toEqual({ amount: 'invalid' });
    expect(dividendErrors({ ...blank, amountText: '0' })).toEqual({ amount: 'notPositive' });
  });

  it('requires a date', () => {
    expect(dividendErrors({ ...blank, amountText: '5', dateIso: '' })).toEqual({
      date: 'required',
    });
  });

  it('allows 500 characters of note, after trimming, and not 501', () => {
    const ok = { ...blank, amountText: '5' };
    expect(dividendErrors({ ...ok, note: 'x'.repeat(500) })).toEqual({});
    expect(dividendErrors({ ...ok, note: ` ${'x'.repeat(500)} ` })).toEqual({});
    expect(dividendErrors({ ...ok, note: 'x'.repeat(501) })).toEqual({ note: 'tooLong' });
  });
});

describe('dividendInput', () => {
  it('writes the parsed amount, the UTC day and the trimmed note', () => {
    expect(
      dividendInput({ dateIso: '2026-03-14', amountText: '1 234,5', note: ' Q1 ' }, 'USD')
    ).toEqual({
      exDate: isoToUtcDay('2026-03-14'),
      grossAmount: parseAmountInput('1 234,5'),
      currency: 'USD',
      note: 'Q1',
    });
  });

  it('turns a blank note into null and an invalid form into null', () => {
    expect(
      dividendInput({ dateIso: '2026-03-14', amountText: '5', note: '  ' }, 'USD')?.note
    ).toBeNull();
    expect(dividendInput(blank, 'USD')).toBeNull();
  });
});
