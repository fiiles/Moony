import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Clock, Ellipsis, EyeOff } from 'lucide-react';
import type { Milestone } from '@shared/schema';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { MilestoneMenuItems } from '@/components/milestones/MilestoneRows';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { useMilestoneSubLine } from '@/hooks/use-milestone-sub-line';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import {
  agendaDay,
  canRemindLater,
  daysBetween,
  isUpkeep,
  isUrgent,
  relativeLabel,
} from '@/utils/milestones';
import { milestonePath } from '@/mobile/lib/milestone-path';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { DateChip, DATE_CHIP_WIDTH } from './DateChip';
import { EntityRow } from './EntityRow';

/**
 * Agenda row of "Co vás čeká" (spec §8.3 item 4, prototype note 5): the entity row with the date
 * chip (day and month, the weekday under it; dark material for a deadline at most 7 days away), the
 * title and one sub-line that ends with the distance ("Zinsbindung endet · in 2 Monaten"). The
 * distance is in the sub-line, not in the chip: it does not fit 54 pt in most languages, a weekday
 * always does, and a long sub-line is the part that gives way. A crossed target and a split notice
 * are today's news instead: the chip reads "Today" over the kind and there is no distance. Swipe
 * right reminds again in a week (only when that ends before the date), swipe left hides; a long
 * press opens the full menu. A tap opens the source page (`milestonePath`), whose back button names
 * the screen the row is on (`from`: Přehled, or "Co vás čeká" in full); a kind without one on the
 * phone leaves the row inert.
 */
export function AgendaRow({ m, from }: { m: Milestone; from: string }) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  // The sub-line names a payment, a coupon or a maturity amount: masked with the rest
  const subLine = useMilestoneSubLine(useMoneyText());
  // One instance for the swipes and the menu items (it keeps the muted kinds observed).
  const actions = useMilestoneActions();
  const { hide, remindLater } = actions;
  const [menuOpen, setMenuOpen] = useState(false);

  const today = utcDayFloor(Date.now() / 1000);
  const day = agendaDay(m, today);
  const rel = relativeLabel(daysBetween(today, day));
  const urgent = isUrgent(m, today);
  // A crossed target and a split notice are today's news: the chip reads "Today" over the kind.
  const todayChip = m.kind === 'watch_target' || m.kind === 'stock_split';
  const chipWord = m.kind === 'stock_split' ? t('card.split') : t('card.target');
  const remindKey = isUpkeep(m) ? 'actions.remindMonth' : 'actions.remindWeek';

  // The chip is not hidden from screen readers: the sub-line leaves the date out, the chip says it.
  const chip = todayChip ? (
    <DateChip day={day} urgent={urgent} label={t('card.today')} caption={chipWord} />
  ) : (
    <DateChip day={day} urgent={urgent} caption={fmt.day(day, { weekday: 'short' })} />
  );

  // What happens, then how far away: the first part is cut with an ellipsis, the distance stays.
  const what = subLine(m);
  const distance = t(`relative.${rel.key}`, { count: rel.count });
  const sub = todayChip ? (
    what
  ) : (
    <span className="flex min-w-0">
      {what && (
        <>
          <span className="min-w-0 truncate">{what}</span>
          <span className="flex-none whitespace-pre"> · </span>
        </>
      )}
      <span className="flex-none">{distance}</span>
    </span>
  );

  return (
    <EntityRow
      chip={chip}
      chipWidth={DATE_CHIP_WIDTH}
      title={m.title}
      sub={sub}
      onSwipeRight={canRemindLater(m, today) ? () => remindLater(m) : undefined}
      swipeRightLabel={t(remindKey)}
      swipeRightIcon={Clock}
      onSwipeLeft={() => hide(m)}
      swipeLeftLabel={t('actions.hide')}
      swipeLeftIcon={EyeOff}
      onLongPress={() => setMenuOpen(true)}
      href={milestonePath(m) ?? undefined}
      from={from}
      accessory={
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={t('card.rowMenu', { title: m.title })}
              className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
            >
              <Ellipsis aria-hidden className="size-5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[220px]">
            <MilestoneMenuItems
              m={m}
              actions={actions}
              itemClassName="min-h-11 text-m-body [&_svg]:size-[18px]"
            />
          </DropdownMenuContent>
        </DropdownMenu>
      }
    />
  );
}
