import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import {
  BellOff,
  Clock,
  DatabaseBackup,
  Ellipsis,
  EyeOff,
  FileCheck,
  Home,
  Landmark,
  Percent,
  type LucideIcon,
} from 'lucide-react';
import type { Milestone, MilestoneKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useMilestoneActions, type MilestoneActions } from '@/hooks/use-milestone-mutations';
import { useMilestoneSubLine } from '@/hooks/use-milestone-sub-line';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { utcDayFloor } from '@/utils/chart-axis';
import {
  agendaDay,
  canRemindLater,
  daysBetween,
  groupOf,
  isUpkeep,
  isUrgent,
  milestoneHref,
  relativeLabel,
} from '@/utils/milestones';

const UPKEEP_ICONS: Partial<Record<MilestoneKind, LucideIcon>> = {
  backup_stale: DatabaseBackup,
  balances_stale: Landmark,
  valuation_stale: Home,
  loan_balance_check: FileCheck,
  loan_fixation_expired: Percent,
};

/**
 * The items of a milestone's menu: hide, remind later (a week, a month for upkeep; only when
 * the snooze ends before the date), mute the group. Shared by the desktop "···" and the phone's
 * long press; `itemClassName` lets the phone raise the items to its touch size.
 *
 * The actions come from the row, never from a hook in here: the items mount only while the menu
 * is open, and the row's `useMilestoneActions()` is what keeps `['milestone-muted-kinds']`
 * observed — without it the cached list is collected after the menu closes, and the next
 * "Nepřipomínat …" would build on an empty list and unmute every other group.
 */
export function MilestoneMenuItems({
  m,
  actions: { hide, remindLater, muteGroup },
  itemClassName,
}: {
  m: Milestone;
  actions: Pick<MilestoneActions, 'hide' | 'remindLater' | 'muteGroup'>;
  itemClassName?: string;
}) {
  const { t } = useTranslation('milestones');
  const group = groupOf(m.kind);
  const today = utcDayFloor(Date.now() / 1000);
  return (
    <>
      <DropdownMenuItem className={itemClassName} onSelect={() => hide(m)}>
        <EyeOff />
        {t('actions.hide')}
      </DropdownMenuItem>
      {canRemindLater(m, today) && (
        <DropdownMenuItem className={itemClassName} onSelect={() => remindLater(m)}>
          <Clock />
          {t(isUpkeep(m) ? 'actions.remindMonth' : 'actions.remindWeek')}
        </DropdownMenuItem>
      )}
      <DropdownMenuSeparator />
      <DropdownMenuItem className={itemClassName} onSelect={() => muteGroup(group)}>
        <BellOff />
        {t(`groups.${group}.mute`)}
      </DropdownMenuItem>
    </>
  );
}

/** "···": hide, remind later, mute the group. */
export function MilestoneMenu({ m, title }: { m: Milestone; title: string }) {
  const { t } = useTranslation('milestones');
  const actions = useMilestoneActions();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('card.rowMenu', { title })}
          className="shrink-0 text-ink-4 hover:text-ink data-[state=open]:text-ink"
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <MilestoneMenuItems m={m} actions={actions} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Agenda row (spec 2026-10-05-milestones-agenda §2): a date chip (day and month, the
 * distance under it; dark material for a deadline at most 7 days away), title and one
 * sub-line, the "···" menu next to the link, never inside it.
 */
export function AgendaRow({ m, onNavigate }: { m: Milestone; onNavigate?: () => void }) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const subLine = useMilestoneSubLine();
  const today = utcDayFloor(Date.now() / 1000);
  const day = agendaDay(m, today);
  const rel = relativeLabel(daysBetween(today, day));
  const urgent = isUrgent(m, today);
  const target = m.kind === 'watch_target';
  const sub = subLine(m);

  return (
    <li className="flex items-center gap-1 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        onClick={onNavigate}
        className="grid min-w-0 flex-1 grid-cols-[60px_1fr] items-center gap-3 py-2.5 text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <span
          className={cn(
            'grid h-[42px] place-content-center rounded-r2 text-center leading-tight',
            urgent ? 'bg-dark-grad text-ink-inverse shadow-dark' : 'bg-well text-ink'
          )}
        >
          <b className="block whitespace-nowrap text-table font-650 num">
            {target ? t('card.today') : fmt.day(day, { day: 'numeric', month: 'numeric' })}
          </b>
          <small
            className={cn(
              'block whitespace-nowrap text-micro font-500',
              urgent ? 'text-ink-inverse-2' : 'text-ink-4'
            )}
          >
            {target ? t('card.target') : t(`relative.${rel.key}`, { count: rel.count })}
          </small>
        </span>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{m.title}</b>
          <small title={sub} className="mt-[3px] block truncate text-micro font-500 text-ink-4">
            {sub}
          </small>
        </span>
      </Link>
      <MilestoneMenu m={m} title={m.title} />
    </li>
  );
}

/** Upkeep row: an icon in a `well` square instead of a date chip. */
export function UpkeepRow({ m }: { m: Milestone }) {
  const { t } = useTranslation('milestones');
  const subLine = useMilestoneSubLine();
  const Icon = UPKEEP_ICONS[m.kind] ?? Landmark;
  const title = m.title || t(`titles.${m.kind}`);
  const sub = subLine(m);
  return (
    <li className="flex items-center gap-1 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        className="grid min-w-0 flex-1 grid-cols-[30px_1fr] items-center gap-[11px] py-2.5 text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <i className="grid size-[29px] place-items-center rounded-r2 bg-well text-ink-2">
          <Icon className="size-[15px]" strokeWidth={1.75} aria-hidden />
        </i>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{title}</b>
          <small title={sub} className="mt-[3px] block truncate text-micro font-500 text-ink-4">
            {sub}
          </small>
        </span>
      </Link>
      <MilestoneMenu m={m} title={title} />
    </li>
  );
}
