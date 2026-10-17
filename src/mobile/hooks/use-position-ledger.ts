import { useCallback, useEffect, useMemo, useState } from 'react';
import type { InvestmentTransaction } from '@shared/schema';
import { foldRows } from '@/mobile/lib/position-rows';
import { filterTransactions, revealRow, type TxFilter } from '@/mobile/lib/position-series';

/** How long a row stays tinted before it fades back (the fade itself is the row's 1.2 s transition). */
const HOT_MS = 150;

/**
 * The position's ledger as the detail lists it: the filter, the fold ("Zobrazit všech N") and the
 * flash a tap on a chart mark sends to the rows it stands for. The flash reveals the first row (the
 * filter that holds it, the unfolded list), scrolls it to the middle of the screen, tints it and
 * lets the tint go 150 ms later, so the row's own transition fades it back.
 */
export function usePositionLedger(transactions: readonly InvestmentTransaction[]) {
  const [filter, setFilter] = useState<TxFilter>('all');
  const [expanded, setExpanded] = useState(false);
  const [hot, setHot] = useState<readonly string[]>([]);

  const rows = useMemo(() => filterTransactions(transactions, filter), [transactions, filter]);
  const { shown, hidden } = foldRows(rows, expanded);

  const flash = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      const reveal = revealRow(transactions, filter, ids[0]);
      setFilter(reveal.filter);
      if (reveal.expand) setExpanded(true);
      setHot(ids);
    },
    [transactions, filter]
  );

  // After the commit, so a row the flash has just revealed is in the page to be scrolled to
  useEffect(() => {
    if (hot.length === 0) return;
    document
      .getElementById(`tx-${hot[0]}`)
      ?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    const timer = setTimeout(() => setHot([]), HOT_MS);
    return () => clearTimeout(timer);
  }, [hot]);

  return {
    filter,
    setFilter,
    rows,
    shown,
    hidden,
    unfold: () => setExpanded(true),
    hot,
    flash,
  };
}

export type PositionLedgerState = ReturnType<typeof usePositionLedger>;
