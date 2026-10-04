import type { ReactNode } from 'react';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

/** Chart height of every trend card on the list pages, so their charts read the same. */
export const TREND_CHART_HEIGHT = 170;

interface TrendCardProps {
  title: ReactNode;
  subtitle?: ReactNode;
  /** Right of the head: the period segment, a horizon switch or a badge. */
  aside?: ReactNode;
  /** The chart, drawn at `TREND_CHART_HEIGHT` by the caller. */
  children: ReactNode;
  /** Legend row (with its note) under the chart. */
  legend?: ReactNode;
  /** Dims the card while its data refreshes. */
  dimmed?: boolean;
  className?: string;
}

/**
 * Trend card of a list page (design system §7 List, "optional trend card"):
 * title, one sentence, a control on the right, the chart and its legend on
 * the hero material (paper with the orb top right). Every list page uses it,
 * so the charts share their chrome and height.
 */
export function TrendCard({
  title,
  subtitle,
  aside,
  children,
  legend,
  dimmed = false,
  className,
}: TrendCardProps) {
  return (
    <Card
      className={cn(
        'relative mb-7 overflow-hidden px-[21px] pb-[14px] pt-5 transition-opacity duration-base after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-[""]',
        dimmed && 'opacity-50',
        className
      )}
    >
      <div className="relative z-[1] flex items-center justify-between gap-6">
        <div className="min-w-0">
          <h3 className="m-0 text-h3 text-ink">{title}</h3>
          {subtitle && <p className="mt-[5px] text-micro font-500 text-ink-4">{subtitle}</p>}
        </div>
        {aside}
      </div>
      <div className="relative z-[1] -mx-1 mt-[14px]">{children}</div>
      {legend && <div className="relative z-[1]">{legend}</div>}
    </Card>
  );
}
