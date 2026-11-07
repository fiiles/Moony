import { loanPath } from '@/mobile/shell/routes';

/** The loan events a link may open the loan's event dialog on. */
export type LinkedLoanEvent = 'balance_check' | 'rate_change';

/** The loan's detail with its event dialog open (`?event=…`): Údržba's Zadat zůstatek / Doplnit sazbu. */
export function loanEventPath(loanId: string, event: LinkedLoanEvent): string {
  return `${loanPath(loanId)}?event=${event}`;
}

/** The event a loan detail's query asks to open, or `null` for none or an unknown one. */
export function loanEventFromSearch(search: string): LinkedLoanEvent | null {
  const event = new URLSearchParams(search).get('event');
  return event === 'balance_check' || event === 'rate_change' ? event : null;
}
