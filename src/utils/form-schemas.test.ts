import { describe, it, expect } from 'vitest';
import { z } from 'zod';
import { requiredNumber, optionalFee, feeForCreate, feeForUpdate } from './form-schemas';

const amountSchema = z.object({
  amount: requiredNumber('validation.amountRequired').positive('validation.amountPositive'),
});

function firstMessage(input: unknown): string | undefined {
  const result = amountSchema.safeParse(input);
  return result.success ? undefined : result.error.issues[0]?.message;
}

describe('requiredNumber', () => {
  it('reports the required key for an untouched field (undefined) instead of the raw zod NaN text', () => {
    expect(firstMessage({ amount: undefined })).toBe('validation.amountRequired');
    expect(firstMessage({})).toBe('validation.amountRequired');
  });

  it('reports the required key for non-numeric text', () => {
    expect(firstMessage({ amount: 'abc' })).toBe('validation.amountRequired');
  });

  it('never produces zod default English messages', () => {
    for (const value of [undefined, 'abc', '', 0, -5, null]) {
      const message = firstMessage({ amount: value });
      expect(message).toMatch(/^validation\./);
    }
  });

  it('reports the positive key for zero, negative numbers and a cleared field', () => {
    expect(firstMessage({ amount: 0 })).toBe('validation.amountPositive');
    expect(firstMessage({ amount: '-3' })).toBe('validation.amountPositive');
    expect(firstMessage({ amount: '' })).toBe('validation.amountPositive');
  });

  it('coerces numeric strings from <input type="number">', () => {
    expect(amountSchema.parse({ amount: '12.5' })).toEqual({ amount: 12.5 });
    expect(amountSchema.parse({ amount: 3 })).toEqual({ amount: 3 });
  });

  it('works with a non-negative constraint too', () => {
    const price = z.object({
      price: requiredNumber('validation.priceRequired').min(0, 'validation.priceNonNegative'),
    });
    const bad = price.safeParse({ price: undefined });
    expect(bad.success ? undefined : bad.error.issues[0]?.message).toBe('validation.priceRequired');
    const neg = price.safeParse({ price: -1 });
    expect(neg.success ? undefined : neg.error.issues[0]?.message).toBe(
      'validation.priceNonNegative'
    );
    expect(price.parse({ price: '0' })).toEqual({ price: 0 });
  });
});

describe('optionalFee', () => {
  const feeSchema = z.object({ fee: optionalFee() });
  const feeMessage = (fee: unknown) => {
    const result = feeSchema.safeParse({ fee });
    return result.success ? undefined : result.error.issues[0]?.message;
  };

  it('accepts an omitted, empty or blank fee', () => {
    expect(feeSchema.safeParse({}).success).toBe(true);
    expect(feeSchema.safeParse({ fee: '' }).success).toBe(true);
    expect(feeSchema.safeParse({ fee: '  ' }).success).toBe(true);
  });

  it('accepts zero and positive decimals', () => {
    expect(feeSchema.safeParse({ fee: '0' }).success).toBe(true);
    expect(feeSchema.safeParse({ fee: '1.25' }).success).toBe(true);
  });

  it('rejects text with the invalid key and negatives with the negative key', () => {
    expect(feeMessage('abc')).toBe('validation.feeInvalid');
    expect(feeMessage('Infinity')).toBe('validation.feeInvalid');
    expect(feeMessage('-1')).toBe('validation.feeNegative');
  });
});

describe('fee wire values', () => {
  it('omits a blank fee on create and trims a filled one', () => {
    expect(feeForCreate(undefined)).toBeUndefined();
    expect(feeForCreate('')).toBeUndefined();
    expect(feeForCreate(' 2.5 ')).toBe('2.5');
  });

  it('sends "" on update when the fee was cleared', () => {
    expect(feeForUpdate(undefined)).toBe('');
    expect(feeForUpdate('')).toBe('');
    expect(feeForUpdate('3')).toBe('3');
  });
});
