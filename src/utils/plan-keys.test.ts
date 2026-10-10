import { describe, expect, it } from 'vitest';
import { categoryInclusionKey, planInclusionKey } from './plan-keys';

describe('plan-keys', () => {
  it('keys an automatic row by its source id', () => {
    expect(planInclusionKey({ id: 'AAPL', sourceId: 'stock:AAPL' })).toBe('stock:AAPL');
    expect(planInclusionKey({ id: 'l1', sourceId: 'loan:l1' })).toBe('loan:l1');
  });

  it('keys a manual row by item:<id>', () => {
    expect(planInclusionKey({ id: '0f3c', sourceId: null })).toBe('item:0f3c');
  });

  it('keys a category by category:<key>', () => {
    expect(categoryInclusionKey('stockDividends')).toBe('category:stockDividends');
  });
});
