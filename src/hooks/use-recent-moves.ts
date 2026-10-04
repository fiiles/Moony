import { useMemo } from 'react';
import { useQueries, useQuery } from '@tanstack/react-query';
import { bankAccountsApi, cryptoApi, investmentsApi } from '@/lib/tauri-api';

export type MoveKind = 'bankIn' | 'bankOut' | 'buy' | 'sell' | 'cryptoBuy' | 'cryptoSell';

export interface RecentMove {
  id: string;
  kind: MoveKind;
  /** Counterparty, description or ticker. */
  title: string;
  /** Account name for bank moves; empty for investments (the domain label comes from the kind). */
  source: string;
  /** Unix seconds of the booking or transaction day. */
  date: number;
  /** Native amount, always positive; the kind carries the direction. */
  amount: number;
  currency: string;
  href: string;
}

/**
 * The latest moves across domains for the overview (design system §7
 * Overview: "Poslední pohyby"): bank credits and debits, stock and crypto
 * buys and sells. Read-only composition of existing commands; refetched on
 * every overview visit so a new transaction shows up without a reload.
 */
export function useRecentMoves(limit = 5) {
  const accounts = useQuery({
    queryKey: ['bank-accounts'],
    queryFn: () => bankAccountsApi.getAll(),
    staleTime: 0,
  });

  const bankQueries = useQueries({
    queries: (accounts.data ?? []).map((account) => ({
      queryKey: ['recent-moves', 'bank', account.id, limit],
      queryFn: () => bankAccountsApi.getTransactions(account.id, { limit }),
      staleTime: 0,
      refetchOnMount: 'always' as const,
    })),
  });

  const stocks = useQuery({
    queryKey: ['recent-moves', 'stocks'],
    queryFn: () => investmentsApi.getAllTransactions(),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const crypto = useQuery({
    queryKey: ['recent-moves', 'crypto'],
    queryFn: () => cryptoApi.getAllTransactions(),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const bankData = bankQueries.map((q) => q.data);
  // A string signature keeps the memo's dependency list a constant size while
  // accounts (and therefore queries) load in.
  const bankSignature = bankQueries.map((q) => q.dataUpdatedAt).join(',');
  const moves = useMemo(() => {
    const out: RecentMove[] = [];
    (accounts.data ?? []).forEach((account, i) => {
      for (const tx of bankData[i]?.transactions ?? []) {
        out.push({
          id: `bank-${tx.id}`,
          kind: tx.type === 'credit' ? 'bankIn' : 'bankOut',
          title: tx.counterpartyName || tx.description || '',
          source: account.name,
          date: tx.bookingDate,
          amount: Math.abs(Number(tx.amount) || 0),
          currency: tx.currency || account.currency,
          href: `/bank-accounts/${account.id}`,
        });
      }
    });
    for (const tx of stocks.data ?? []) {
      out.push({
        id: `stock-${tx.id}`,
        kind: tx.type === 'sell' ? 'sell' : 'buy',
        title: tx.companyName || tx.ticker,
        source: tx.ticker,
        date: tx.transactionDate,
        amount: Math.abs((Number(tx.quantity) || 0) * (Number(tx.pricePerUnit) || 0)),
        currency: tx.currency,
        href: `/stocks/${tx.investmentId}`,
      });
    }
    for (const tx of crypto.data ?? []) {
      out.push({
        id: `crypto-${tx.id}`,
        kind: tx.type === 'sell' ? 'cryptoSell' : 'cryptoBuy',
        title: tx.name || tx.ticker,
        source: tx.ticker,
        date: tx.transactionDate,
        amount: Math.abs((Number(tx.quantity) || 0) * (Number(tx.pricePerUnit) || 0)),
        currency: tx.currency,
        href: `/crypto/${tx.investmentId}`,
      });
    }
    return out.sort((a, b) => b.date - a.date).slice(0, limit);
    // bankData changes exactly when bankSignature does (query data timestamps)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts.data, stocks.data, crypto.data, limit, bankSignature]);

  const isLoading =
    accounts.isLoading ||
    stocks.isLoading ||
    crypto.isLoading ||
    bankQueries.some((q) => q.isLoading);

  return { moves, isLoading };
}
