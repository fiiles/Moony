import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { ChevronRight, ClipboardList } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { ProgressBar } from '@/mobile/components/cashflow/ProgressBar';
import { yearSpentLine } from '@/mobile/components/plans/plan-text';
import type { PlansCardSummary } from '@/mobile/lib/plan-rows';

interface PlansCardProps {
  summary: PlansCardSummary;
  /** The plans list of the card's year (`plansPath`). */
  href: string;
}

/**
 * The spending plans card of the Cashflow root (spec M3, M17): a card like the bars card, with a
 * small square plan glyph, "Výdajové plány 2026", what is left in the active plans, the whole
 * year's spend as a bar and a line, and how many active plans are over or near their amount. It
 * is a card with a heading, never a note's quiet line. A total that could not be
 * converted shows "—" and no bar.
 */
export function PlansCard({ summary, href }: PlansCardProps) {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const fmt = useFormat();
  const { planned, spent, left, over, near } = summary;
  const ratio = planned !== null && spent !== null && planned > 0 ? spent / planned : null;
  const status = [
    over > 0 ? t('plans.card.over', { count: over }) : null,
    near > 0 ? t('plans.card.near', { count: near }) : null,
  ].filter(Boolean);

  return (
    <Link
      href={href}
      className="mb-3.5 block rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1 [-webkit-touch-callout:none] active:bg-well"
    >
      <span className="flex items-center gap-2.5">
        <span
          aria-hidden
          className="flex size-9 flex-none items-center justify-center rounded-[10px] bg-well-2 text-ink-2 [&_svg]:size-4"
        >
          <ClipboardList strokeWidth={1.75} />
        </span>
        <span className="min-w-0 flex-auto truncate text-m-section text-ink">
          {t('plans.card.title', { year: summary.year })}
        </span>
        <ChevronRight aria-hidden className="size-4 flex-none text-ink-5" strokeWidth={1.75} />
      </span>
      <span className="mt-3 flex items-baseline justify-between gap-3">
        <span className="min-w-0 truncate text-m-sub text-ink-3">{tp('stats.remaining')}</span>
        <span
          className={`flex-none whitespace-nowrap text-m-row num ${left !== null && left < 0 ? 'text-loss' : 'text-ink'}`}
        >
          {left === null ? '—' : fmt.money(left, summary.currency, { decimals: 0 })}
        </span>
      </span>
      {ratio !== null && (
        <span className="mt-2 block">
          <ProgressBar value={ratio} tone={ratio > 1 ? 'over' : 'limit'} />
        </span>
      )}
      <span className="mt-2 block text-m-sub text-ink-4">
        {yearSpentLine(t, fmt.money, summary)}
      </span>
      {status.length > 0 && (
        <span className="mt-1 block text-m-sub font-semibold text-ink-2">{status.join(' · ')}</span>
      )}
    </Link>
  );
}
