import { useTranslation } from 'react-i18next';
import { Clock, EyeOff } from 'lucide-react';
import type { Milestone } from '@shared/schema';
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
import { ROW_MENU_ITEM } from './RowMenu';

/**
 * Agenda row of "Co vás čeká" (spec §8.3 item 4, prototype note 5): the entity row with the date
 * chip (day and month, the weekday under it; dark material for a deadline at most 7 days away), the
 * title and one sub-line that ends with the distance ("Zinsbindung endet · in 2 Monaten"). The
 * distance is in the sub-line, not in the chip: it does not fit 54 pt in most languages, a weekday
 * always does, and a long sub-line is the part that gives way. A crossed target and a split notice
 * are today's news instead: the chip reads "Today" over the kind and there is no distance. Swipe
 * right reminds again in a week (only when that ends before the date), swipe left hides (a
 * reversible removal with its "Vrátit" toast, README §13 Gestures); a long press or the visible
 * `···` opens the full menu. A tap opens the source page (`milestonePath`), whose back button names
 * the screen the row is on (`from`: Přehled, or "Co vás čeká" in full); a kind without one on the
 * phone leaves the row inert.
 */
export function AgendaRow({ m, from }: { m: Milestone; from: string }) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  // The sub-line names a payment, a coupon or a maturity amount
  const subLine = useMilestoneSubLine(useMoneyText());
  // One instance for the swipes and the menu items (it keeps the muted kinds observed).
  const actions = useMilestoneActions();
  const { hide, remindLater } = actions;

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
      href={milestonePath(m) ?? undefined}
      from={from}
      menu={{
        label: t('card.rowMenu', { title: m.title }),
        // Skrýt, Připomenout and Nepřipomínat are not on the page the row opens
        shown: true,
        children: <MilestoneMenuItems m={m} actions={actions} itemClassName={ROW_MENU_ITEM} />,
      }}
    />
  );
}
