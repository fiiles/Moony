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
import { useCurrency } from '@/lib/currency';
import { formatNativePrice } from '@/utils/stock-monitor';
import { agendaRowParts, type AgendaAmount } from '@/mobile/lib/agenda-row';
import { milestonePath } from '@/mobile/lib/milestone-path';
import { exactMoneyOptions } from '@/mobile/lib/money-rule';
import { useApprox } from '@/mobile/hooks/use-approx';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { DateChip, DATE_CHIP_WIDTH } from './DateChip';
import { EntityRow } from './EntityRow';
import { SubFacts } from './SubFacts';
import { ROW_MENU_ITEM } from './RowMenu';

/**
 * Agenda row of "Co vás čeká" (spec §8.3 item 4, prototype note 5): the entity row with the date
 * chip (day and month, the weekday under it; dark material for a deadline at most 7 days away), the
 * title and one sub-line of two facts: what happens, in a short word, and the distance
 * ("Zinsbindung endet · in 2 Monaten", `agendaRowParts`). The distance is in the sub-line, not in
 * the chip: it does not fit 54 pt in most languages, a weekday always does. An amount (a payment,
 * a coupon, a maturity) is the row's value in its own currency with its "≈" under it, so no figure
 * ends in an ellipsis (audit C1); the word before the distance is what gives way. A crossed target
 * and a split notice are today's news instead: the chip reads "Today" over the kind, the price is
 * the value and the sub-line names the target (or counts the trades of the split). Swipe
 * right reminds again in a week (only when that ends before the date), swipe left hides (a
 * reversible removal with its "Vrátit" toast, README §13 Gestures); a long press or the visible
 * `···` opens the full menu. A tap opens the source page (`milestonePath`), whose back button names
 * the screen the row is on (`from`: Přehled, or "Co vás čeká" in full); a kind without one on the
 * phone leaves the row inert.
 */
export function AgendaRow({ m, from }: { m: Milestone; from: string }) {
  const { t } = useTranslation('milestones');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const moneyText = useMoneyText();
  const approx = useApprox();
  // The shared sub-line, for a kind the phone has no short word for
  const subLine = useMilestoneSubLine(moneyText);
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

  // One fact and how far away (C1): an amount is the row's value, never cut inside the sub-line,
  // and the text before the distance is what gives way
  const parts = agendaRowParts(m);
  const price = (a: AgendaAmount | null) =>
    a === null ? '' : formatNativePrice(a.value, a.currency, fmt.locale, currencyCode);
  let what: string;
  let tail: string | null = null;
  let value: string | undefined;
  let valueSub: string | undefined;
  if (parts.what === 'watch_target_below' || parts.what === 'watch_target_above') {
    what = tm(`agenda.what.${parts.what}`);
    tail = parts.target ? price(parts.target) : null;
    value = parts.price ? price(parts.price) : undefined;
  } else if (parts.what === 'stock_split') {
    what = tm('agenda.what.stock_split', { ratio: parts.ratio });
    tail = parts.count > 0 ? tm('agenda.splitTrades', { count: parts.count }) : null;
  } else {
    what = parts.what === null ? subLine(m) : tm(`agenda.what.${parts.what}`);
    if (parts.amount) {
      const { value: amount, currency } = parts.amount;
      value = moneyText(amount, currency, exactMoneyOptions(amount, currency));
      valueSub = approx(amount, currency);
    }
  }
  const distance = t(`relative.${rel.key}`, { count: rel.count });
  const sub = <SubFacts wrap lead={what} tail={todayChip ? tail : distance} />;

  return (
    <EntityRow
      chip={chip}
      chipWidth={DATE_CHIP_WIDTH}
      wrap
      title={m.title}
      sub={sub}
      value={value}
      valueSub={valueSub}
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
