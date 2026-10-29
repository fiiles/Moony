import type { ReactNode } from 'react';
import { ArrowDownLeft, Receipt } from 'lucide-react';

/**
 * The counterparty of a ledger row: a round icon (incoming arrow or receipt), the name in bold and
 * a sub-line under it, then any badges (`children`). Shared by the account ledger and the
 * spending plan's payment list, so both read the same.
 */
export function LedgerCounterpartyCell({
  credit,
  title,
  subtitle,
  children,
}: {
  credit: boolean;
  /** "—" when null. */
  title: string | null;
  subtitle?: string | null;
  children?: ReactNode;
}) {
  return (
    <div className="flex min-w-0 items-center gap-[10px] text-ink">
      <span className="grid size-[30px] shrink-0 place-items-center rounded-full bg-well-2 text-ink-2">
        {credit ? (
          <ArrowDownLeft className="size-3.5" strokeWidth={1.75} aria-hidden />
        ) : (
          <Receipt className="size-3.5" strokeWidth={1.75} aria-hidden />
        )}
      </span>
      <div className="min-w-0">
        {title ? (
          <b className="block truncate text-table font-650" title={title}>
            {title}
          </b>
        ) : (
          <span className="text-ink-4">—</span>
        )}
        {subtitle && (
          <small className="block truncate text-micro font-500 text-ink-4 num" title={subtitle}>
            {subtitle}
          </small>
        )}
      </div>
      {children}
    </div>
  );
}
