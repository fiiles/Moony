import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { MoonyBarChart, type BarGroup } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  flowsWindow,
  monthLabelDate,
  monthlyFlows,
  type FlowTransaction,
} from '@/utils/bank-activity';

interface FlowsCardProps {
  /** Booked rows in the account currency (converted by the caller). */
  transactions: readonly FlowTransaction[];
  currency: string;
  currentMonth: number;
  /** Month index the 12-month window ends with. */
  windowEnd: number;
  onWindowEndChange: (end: number) => void;
  /** A month the user clicked; null = the newest month with rows in the window. */
  selectedOverride: number | null;
  onSelect: (month: number | null) => void;
  /** True when this is the first card on the page: 42 px value and the hero material. */
  hero: boolean;
  /** Readiness notes (converted / missing currencies) rendered under the chart. */
  notes?: ReactNode;
}

/** Monthly credits and debits of the account in a 12-month window shifted by arrows. */
export function FlowsCard({
  transactions,
  currency,
  currentMonth,
  windowEnd,
  onWindowEndChange,
  selectedOverride,
  onSelect,
  hero,
  notes,
}: FlowsCardProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const win = flowsWindow(transactions, windowEnd, currentMonth, selectedOverride);
  const flows = monthlyFlows(transactions, windowEnd);
  const selected =
    win.selected !== null ? (flows.find((f) => f.month === win.selected) ?? null) : null;
  const money = (v: number, signed = false) => fmt.money(v, currency, { signed, decimals: 0 });
  const monthName = (month: number) =>
    fmt.month(monthLabelDate(month), { month: 'long', year: 'numeric' });
  const bars: BarGroup[] = flows.map((f) => ({
    label: fmt.month(monthLabelDate(f.month), { month: 'short', year: '2-digit' }),
    a: f.income,
    b: f.expense,
    muted: win.selected !== null && f.month !== win.selected,
  }));
  const hasRows = !!selected && selected.count > 0;
  const net = selected && hasRows ? selected.income - selected.expense : 0;

  const arrows = (
    <div className="flex items-center gap-1">
      <Button
        variant="outline"
        size="icon-sm"
        aria-label={t('flows.prev')}
        disabled={!win.canPrev}
        onClick={() => {
          onSelect(null);
          onWindowEndChange(windowEnd - 1);
        }}
      >
        <ChevronLeft />
      </Button>
      <Button
        variant="outline"
        size="icon-sm"
        aria-label={t('flows.next')}
        disabled={!win.canNext}
        onClick={() => {
          onSelect(null);
          onWindowEndChange(windowEnd + 1);
        }}
      >
        <ChevronRight />
      </Button>
    </div>
  );

  const value = (
    <HeroValue
      compact={!hero}
      label={
        win.selected !== null
          ? t('flows.netIn', { month: monthName(win.selected) })
          : t('flows.net')
      }
      value={
        <span className={net > 0 ? 'text-gain' : net < 0 ? 'text-loss' : undefined}>
          {hasRows ? money(net, true) : '—'}
        </span>
      }
      tone={net > 0 ? 'gain' : net < 0 ? 'loss' : 'neutral'}
      deltaNote={
        selected && hasRows
          ? t('flows.line', {
              credits: money(selected.income, true),
              debits: money(-selected.expense, true),
              count: selected.count,
            })
          : t('flows.noMoves')
      }
    />
  );

  const chart = (
    <>
      <MoonyBarChart
        className="-mx-1 mt-[18px]"
        bars={bars}
        names={[t('flows.income'), t('flows.expense')]}
        height={180}
        formatValue={(v) => money(v)}
        renderExtra={(bar) =>
          bar.a === 0 && bar.b === 0
            ? t('flows.noImport')
            : `${t('flows.balance')} ${money(bar.a - bar.b, true)}`
        }
        onBarClick={(i) => onSelect(flows[i]?.month ?? null)}
      />
      <ChartLegend
        items={[
          { label: t('flows.income'), swatch: { kind: 'block', color: 'var(--s1)' } },
          { label: t('flows.expense'), swatch: { kind: 'block', color: 'var(--s3)' } },
        ]}
      />
      {notes}
    </>
  );

  if (hero) {
    return (
      <HeroCard aside={arrows} chart={chart}>
        {value}
      </HeroCard>
    );
  }
  return (
    <Card className={cn('px-6 pb-4 pt-5')}>
      <div className="flex items-start justify-between gap-6">
        {value}
        {arrows}
      </div>
      {chart}
    </Card>
  );
}
