import type { ReactNode } from 'react';
import { Receipt } from 'lucide-react';
import { cn } from '@/lib/utils';
import { AssetLogo } from '@/components/common/AssetLogo';
import { EmptyState } from '@/components/common/EmptyState';

/** The head row of both tables: 40 px on `well`, 10 px uppercase labels. */
export const HEAD_ROW =
  'grid h-10 items-center border-y border-line bg-well px-[21px] text-thead uppercase text-ink-3';

/** A body row: 56 px, divided by hairlines. */
export const BODY_ROW =
  'grid min-h-row items-center border-b border-line-soft px-[21px] py-2 text-table text-ink-2';

/** A right-aligned numeric cell; its sub-line may wrap between words, never inside an amount. */
export const NUM_CELL = 'min-w-0 text-right num';

/** The muted line under a cell's value. */
export function SubLine({
  children,
  className,
  title,
}: {
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <small
      title={title}
      className={cn('mt-[3px] block text-micro font-500 leading-[1.3] text-ink-4', className)}
    >
      {children}
    </small>
  );
}

/** The instrument of a row: logo, name and a sub-line, both truncated with the full text on hover. */
export function EntityCell({
  ticker,
  name,
  sub,
  note,
}: {
  ticker: string;
  name: string;
  sub: string;
  /** A second muted line (the uncovered note of a sale). */
  note?: string;
}) {
  const title = name || ticker;
  return (
    <div className="flex min-w-0 items-center gap-2.5">
      <AssetLogo ticker={ticker} type="stock" className="shrink-0" />
      <div className="min-w-0">
        <b className="block truncate text-table font-650 text-ink" title={title}>
          {title}
        </b>
        <SubLine className="truncate" title={sub}>
          {sub}
        </SubLine>
        {note && (
          <SubLine className="truncate" title={note}>
            {note}
          </SubLine>
        )}
      </div>
    </div>
  );
}

/** The empty state inside the table card. */
export function TableEmpty({ title, body }: { title: string; body: string }) {
  return <EmptyState bare className="py-10" icon={<Receipt />} title={title} description={body} />;
}
