import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import {
  CalendarClock,
  Check,
  Clock,
  DatabaseBackup,
  FileCheck,
  Home,
  Landmark,
  Percent,
  ScrollText,
  Shield,
  Target,
  type LucideIcon,
} from 'lucide-react';
import type { Milestone, MilestoneKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { utcDayFloor } from '@/utils/chart-axis';
import { formatNativePrice } from '@/utils/stock-monitor';
import { daysBetween, milestoneHref, referenceDay, relativeLabel } from '@/utils/milestones';

const ICONS: Record<MilestoneKind, LucideIcon> = {
  insurance_anniversary: Shield,
  insurance_end: Shield,
  insurance_payment: Shield,
  loan_fixation_end: Percent,
  loan_fixation_expired: Percent,
  loan_payoff: Landmark,
  loan_balance_check: FileCheck,
  bond_maturity: ScrollText,
  bond_coupon: ScrollText,
  account_termination: CalendarClock,
  savings_rate_end: Percent,
  balances_stale: Landmark,
  backup_stale: DatabaseBackup,
  valuation_stale: Home,
  watch_target: Target,
};

interface MilestoneListProps {
  milestones: readonly Milestone[];
  onDone: (m: Milestone) => void;
  onSnooze: (m: Milestone) => void;
  /** Called when a row's link is followed (closes the top-bar popover). */
  onNavigate?: () => void;
  /** Let the sub-line wrap onto a second line instead of truncating (narrow popover). */
  wrap?: boolean;
}

/**
 * Milestone rows (spec 2026-10-05 §7): icon in a `well` square, title and one
 * sub-line, the relative date right; "Odložit" and "Vyřízeno" appear on hover
 * next to the link, never inside it.
 */
export function MilestoneList({
  milestones,
  onDone,
  onSnooze,
  onNavigate,
  wrap = false,
}: MilestoneListProps) {
  return (
    <ul className="m-0 list-none p-0">
      {milestones.map((m) => (
        <MilestoneRow
          key={m.key}
          m={m}
          onDone={onDone}
          onSnooze={onSnooze}
          onNavigate={onNavigate}
          wrap={wrap}
        />
      ))}
    </ul>
  );
}

function MilestoneRow({
  m,
  onDone,
  onSnooze,
  onNavigate,
  wrap,
}: {
  m: Milestone;
  onDone: (m: Milestone) => void;
  onSnooze: (m: Milestone) => void;
  onNavigate?: () => void;
  wrap: boolean;
}) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const today = utcDayFloor(Date.now() / 1000);
  const thisYear = new Date(today * 1000).getUTCFullYear();

  const date = (d: number | null) =>
    d === null
      ? ''
      : new Date(d * 1000).getUTCFullYear() === thisYear
        ? fmt.day(d, { day: 'numeric', month: 'numeric' })
        : fmt.day(d);
  const money = (amount: string | null) =>
    amount === null ? '' : fmt.money(Number(amount), m.currency ?? 'CZK');
  const price = (amount: string | null) =>
    amount === null ? '' : formatNativePrice(Number(amount), m.currency, fmt.locale);

  const sub = (() => {
    switch (m.kind) {
      case 'insurance_anniversary':
        return m.tone === 'info'
          ? t('sub.insurance_anniversary_passed', { date: date(m.dueDay) })
          : t('sub.insurance_anniversary', { date: date(m.dueDay), deadline: date(m.actionDay) });
      case 'insurance_payment':
      case 'bond_coupon':
        return t(`sub.${m.kind}`, { amount: money(m.amount), date: date(m.dueDay) });
      case 'bond_maturity':
        return t('sub.bond_maturity', { date: date(m.dueDay), amount: money(m.amount) });
      case 'backup_stale':
        return m.sinceDay === null
          ? t('sub.backup_never')
          : t('sub.backup_stale', { date: date(m.sinceDay) });
      case 'balances_stale':
        return t('sub.balances_stale', { count: m.count ?? 0 });
      case 'valuation_stale':
      case 'loan_balance_check':
        return t(`sub.${m.kind}`, { date: date(m.sinceDay) });
      case 'watch_target':
        return t(m.direction === 'below' ? 'sub.watch_target_below' : 'sub.watch_target_above', {
          price: price(m.referenceAmount),
          target: price(m.amount),
        });
      default:
        return t(`sub.${m.kind}`, { date: date(m.dueDay) });
    }
  })();

  const right = (() => {
    if (m.kind === 'watch_target') {
      const target = Number(m.amount);
      const current = Number(m.referenceAmount);
      return target > 0 && current > 0
        ? fmt.percent(current / target - 1, 1, { signed: true })
        : '';
    }
    const ref = referenceDay(m);
    if (ref === null) return '';
    const label = relativeLabel(daysBetween(today, ref));
    return t(`relative.${label.key}`, { count: label.count });
  })();

  const Icon = ICONS[m.kind];
  const title = m.title || t(`titles.${m.kind}`);

  return (
    <li className="group grid grid-cols-[1fr_auto] items-center gap-2 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        onClick={onNavigate}
        className="grid min-w-0 grid-cols-[30px_1fr_auto] items-center gap-[11px] py-[11px] text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <i className="grid size-[29px] place-items-center rounded-r2 bg-well text-ink-2">
          <Icon className="size-[15px]" strokeWidth={1.75} aria-hidden />
        </i>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{title}</b>
          <small
            title={sub}
            className={cn(
              'mt-[3px] block text-micro font-500 text-ink-4',
              wrap ? 'line-clamp-2 whitespace-normal' : 'truncate'
            )}
          >
            {sub}
          </small>
        </span>
        <span className="whitespace-nowrap text-caption font-600 text-ink-3 num">{right}</span>
      </Link>
      <div className="flex w-[66px] shrink-0 justify-end gap-0.5 opacity-0 transition-opacity duration-fast focus-within:opacity-100 group-hover:opacity-100">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('actions.snooze')}
          title={t('actions.snooze')}
          onClick={() => onSnooze(m)}
        >
          <Clock />
        </Button>
        {m.canDismiss && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('actions.done')}
            title={t('actions.done')}
            onClick={() => onDone(m)}
          >
            <Check />
          </Button>
        )}
      </div>
    </li>
  );
}
