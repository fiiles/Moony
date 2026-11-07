import type { RecentMove } from '@/hooks/use-recent-moves';
import { accountPath, cryptoPath, positionPath } from '@/mobile/shell/routes';

/** The query that opens one payment's sheet on the account detail (`?tx=<id>`). */
export const TX_PARAM = 'tx';

/** The account detail with one payment's sheet open: Přehled's "Poslední pohyby" row. */
export function transactionPath(accountId: string, transactionId: string): string {
  return `${accountPath(accountId)}?${TX_PARAM}=${encodeURIComponent(transactionId)}`;
}

/** The payment a detail's query asks to open, or `null`. */
export function transactionFromSearch(search: string): string | null {
  return new URLSearchParams(search).get(TX_PARAM) || null;
}

/**
 * The phone page a "Poslední pohyby" row opens (C11): a bank payment opens its account with the
 * payment's sheet, a trade its position or coin. The move carries the desktop's `href` and a
 * prefixed id (`useRecentMoves`); a shape this does not know opens nothing (`null`).
 */
export function movePath(move: Pick<RecentMove, 'id' | 'kind' | 'href'>): string | null {
  const after = (prefix: string, text: string) =>
    text.startsWith(prefix) && text.length > prefix.length ? text.slice(prefix.length) : null;
  switch (move.kind) {
    case 'bankIn':
    case 'bankOut': {
      const accountId = after('/bank-accounts/', move.href);
      const txId = after('bank-', move.id);
      return accountId && txId ? transactionPath(accountId, txId) : null;
    }
    case 'buy':
    case 'sell': {
      const id = after('/stocks/', move.href);
      return id && positionPath(id);
    }
    case 'cryptoBuy':
    case 'cryptoSell': {
      const id = after('/crypto/', move.href);
      return id && cryptoPath(id);
    }
  }
}
