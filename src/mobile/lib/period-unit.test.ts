import { describe, expect, it } from 'vitest';
import { periodUnit } from './period-unit';

describe('periodUnit', () => {
  it('maps the premium and cost frequencies to the unit next to an amount', () => {
    expect(periodUnit('monthly')).toBe('month');
    expect(periodUnit('quarterly')).toBe('quarter');
    expect(periodUnit('semi_annually')).toBe('halfYear');
    expect(periodUnit('annually')).toBe('year');
    expect(periodUnit('yearly')).toBe('year');
  });

  it('has no unit for a one-time payment or an unknown frequency', () => {
    expect(periodUnit('one_time')).toBeNull();
    expect(periodUnit('weekly')).toBeNull();
    expect(periodUnit(null)).toBeNull();
    expect(periodUnit(undefined)).toBeNull();
  });
});
