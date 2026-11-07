import { describe, expect, it } from 'vitest';
import { loanEventFromSearch, loanEventPath } from './loan-event-param';

describe('loan event links', () => {
  it('puts the event in the loan path and reads it back', () => {
    const path = loanEventPath('l1', 'balance_check');
    expect(path).toBe('/finance/loans/l1?event=balance_check');
    expect(loanEventFromSearch(path.slice(path.indexOf('?')))).toBe('balance_check');
    expect(loanEventFromSearch('?event=rate_change')).toBe('rate_change');
  });

  it('opens nothing for no event or one a link may not open', () => {
    expect(loanEventFromSearch('')).toBeNull();
    expect(loanEventFromSearch('?event=extra_payment')).toBeNull();
  });
});
