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
import { cn } from '@/lib/utils';
import { utcDayFloor } from '@/utils/chart-axis';
import {
  agendaDay,
  canRemindLater,
  daysBetween,
  isUpkeep,
  isUrgent,
  relativeLabel,
} from '@/utils/milestones';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { EntityRow } from './EntityRow';

/** Width of the date chip (prototype `.dchip`): 54 pt at the default text size, wider with the text. */
const DATE_CHIP = 'calc(54px * var(--m-text-k))';

/**
 * Agenda row of "Co vás čeká" (spec §8.3 item 4, prototype note 5): the entity row with a 54 × 42
 * date chip (day and month, the distance under it; dark material for a deadline at most 7 days
 * away), the title and one sub-line. Swipe right reminds again in a week (only when that ends
 * before the date), swipe left hides; a long press opens the full menu. The source pages are not
 * on the phone yet, so a tap does nothing.
 */
export function AgendaRow({ m }: { m: Milestone }) {
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
  const target = m.kind === 'watch_target';
  const remindKey = isUpkeep(m) ? 'actions.remindMonth' : 'actions.remindWeek';

  // Not hidden from screen readers: the sub-line leaves the date out, the chip is where it is said.
  const chip = (
    <span
      className={cn(
        'flex min-h-[42px] flex-none flex-col items-center justify-center rounded-[9px] py-0.5 leading-tight',
        urgent ? 'bg-dark-grad shadow-dark' : 'bg-well'
      )}
      style={{ width: DATE_CHIP }}
    >
      <b
        className={cn(
          'whitespace-nowrap text-m-sub font-650 tracking-[-0.01em] num',
          urgent ? 'text-ink-inverse' : 'text-ink'
        )}
      >
        {target ? t('card.today') : fmt.day(day, { day: 'numeric', month: 'numeric' })}
      </b>
      <small
        className={cn(
          'mt-px whitespace-nowrap text-m-note',
          urgent ? 'text-ink-inverse-2' : 'text-ink-4'
        )}
      >
        {target ? t('card.target') : t(`relative.${rel.key}`, { count: rel.count })}
      </small>
    </span>
  );

  return (
    <EntityRow
      chip={chip}
      chipWidth={DATE_CHIP}
      title={m.title}
      sub={subLine(m)}
      onSwipeRight={canRemindLater(m, today) ? () => remindLater(m) : undefined}
      swipeRightLabel={t(remindKey)}
      swipeRightIcon={Clock}
      onSwipeLeft={() => hide(m)}
      swipeLeftLabel={t('actions.hide')}
      swipeLeftIcon={EyeOff}
      onLongPress={() => setMenuOpen(true)}
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
