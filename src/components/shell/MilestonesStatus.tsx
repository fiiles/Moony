import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { StatusText } from '@/components/shell/DataStatus';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { utcDayFloor } from '@/utils/chart-axis';
import { agendaDay, daysBetween, relativeLabel, upcomingDeadlines } from '@/utils/milestones';

/**
 * Nearest deadline in the top bar (spec 2026-10-05 §7): deadlines up to 14 days
 * ahead on every page, opened in a popover. Hidden when there are none.
 */
export function MilestonesStatus() {
  const { t } = useTranslation('milestones');
  const [open, setOpen] = useState(false);
  const { data: milestones = [] } = useMilestones();
  const { hide, remindLater } = useMilestoneActions();
  const today = utcDayFloor(Date.now() / 1000);
  const deadlines = upcomingDeadlines(milestones, today);

  // An item that comes back (e.g. "Vrátit") must not reopen the popover by itself.
  if (deadlines.length === 0 && open) setOpen(false);

  if (deadlines.length === 0) return null;

  const first = deadlines[0];
  const label = relativeLabel(daysBetween(today, agendaDay(first, today)));
  const summary = t('status.deadline', {
    title: first.title || t(`titles.${first.kind}`),
    when: t(`relative.${label.key}`, { count: label.count }),
  });

  return (
    <Popover open={open && deadlines.length > 0} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <StatusText className="text-ink-2">
            {summary}
            {deadlines.length > 1 && ` ${t('status.more', { count: deadlines.length - 1 })}`}
          </StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[420px] p-0">
        <div className="px-4 pt-3 text-eyebrow uppercase text-ink-4">{t('status.title')}</div>
        <div className="max-h-[420px] overflow-y-auto px-4">
          <MilestoneList
            milestones={deadlines}
            onHide={hide}
            onRemindLater={remindLater}
            onNavigate={() => setOpen(false)}
            wrap
          />
        </div>
        <div className="border-t border-line-soft px-4 py-2.5">
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('status.openOverview')} →
          </Link>
        </div>
      </PopoverContent>
    </Popover>
  );
}
