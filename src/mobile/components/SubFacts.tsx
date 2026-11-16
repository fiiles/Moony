import type { ReactNode } from 'react';

interface SubFactsProps {
  /** The first fact: text that may end with an ellipsis (a bank, a provider, a kind of event). */
  lead?: ReactNode;
  /**
   * The second fact: an amount, a date, a distance, a code — never cut (README §13, "Sub-lines and
   * ellipses"). Joined with " · ".
   */
  tail?: ReactNode;
  /**
   * `keep` (default): the tail always shows and the lead gives way (an agenda row's distance, an
   * account's currency). `drop`: the lead keeps its width and the tail shows only when it fits
   * whole, else it is left to the detail (a loan's fixation, a list of tickers).
   */
  mode?: 'keep' | 'drop';
  /**
   * Both facts run on as ordinary text and wrap onto a second line, the tail kept whole. For a row of
   * words (an agenda row's "what happens", an upkeep row's explanation), where a cut sentence hides
   * the meaning; with it `mode` does not apply.
   */
  wrap?: boolean;
}

/**
 * A row's sub-line of at most two facts (audit 2026-11-15 C1): no money figure and no date ends in
 * an ellipsis. `keep` cuts the text before the tail; `drop` wraps a tail that does not fit onto a
 * second line that is clipped away, so it is either whole or gone.
 */
export function SubFacts({ lead, tail, mode = 'keep', wrap = false }: SubFactsProps) {
  const hasLead = lead !== undefined && lead !== null && lead !== false && lead !== '';
  const hasTail = tail !== undefined && tail !== null && tail !== false && tail !== '';
  if (wrap) {
    if (!hasLead && !hasTail) return null;
    return (
      <span className="block min-w-0 break-words">
        {hasLead && lead}
        {hasLead && hasTail && ' · '}
        {hasTail && <span className="whitespace-nowrap">{tail}</span>}
      </span>
    );
  }
  if (!hasTail) return hasLead ? <span className="block min-w-0 truncate">{lead}</span> : null;
  if (!hasLead) return <span className="whitespace-nowrap">{tail}</span>;
  if (mode === 'drop') {
    return (
      <span className="flex max-h-[1lh] min-w-0 flex-wrap overflow-hidden" data-sub-facts="drop">
        <span className="min-w-0 max-w-full truncate">{lead}</span>
        <span className="flex-none whitespace-pre">
          {' · '}
          {tail}
        </span>
      </span>
    );
  }
  return (
    <span className="flex min-w-0">
      <span className="min-w-0 truncate">{lead}</span>
      <span className="flex-none whitespace-pre">
        {' · '}
        {tail}
      </span>
    </span>
  );
}
