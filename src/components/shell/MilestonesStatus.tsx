import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { CalendarDays } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { AgendaRow } from '@/components/milestones/MilestoneRows';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import {
  agendaDay,
  daysBetween,
  isUrgent,
  relativeLabel,
  upcomingDeadlines,
} from '@/utils/milestones';

/**
 * Top-bar deadlines (spec 2026-11-16 top bar, design system §5): only deadlines at most 14 days
 * away, behind a calendar icon so they never read as a status light. One deadline is named
 * ("Spořicí účet · za 10 dní"), more are counted ("4 termíny · první za 4 dny"); the distance is a
 * dark badge at most 7 days away, like the agenda's date chip. Not rendered on Přehled, whose
 * agenda lists the same deadlines.
 */
export function MilestonesStatus() {
  const { t } = useTranslation('milestones');
  const [open, setOpen] = useState(false);
  const { data: milestones = [] } = useMilestones();
  const today = utcDayFloor(Date.now() / 1000);
  const deadlines = upcomingDeadlines(milestones, today);

  // A deadline that goes away (hidden, muted) must not leave the popover open.
  if (deadlines.length === 0 && open) setOpen(false);
  if (deadlines.length === 0) return null;

  const first = deadlines[0];
  const rel = relativeLabel(daysBetween(today, agendaDay(first, today)));
  const when = t(`relative.${rel.key}`, { count: rel.count });
  const single = deadlines.length === 1;

  return (
    <Popover open={open && deadlines.length > 0} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="group min-w-0 rounded-r1 px-1 py-0.5 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <span className="inline-flex min-w-0 items-center gap-[7px] text-caption font-600 text-ink-2 group-hover:text-ink">
            <CalendarDays className="size-3.5 shrink-0 text-ink-3" strokeWidth={1.75} aria-hidden />
            {single ? (
              <span className="max-w-[240px] truncate">{first.title}</span>
            ) : (
              <span className="shrink-0">{t('status.count', { count: deadlines.length })}</span>
            )}
            <span className="shrink-0 text-ink-3">{single ? '·' : `· ${t('status.first')}`}</span>
            {isUrgent(first, today) ? (
              <Badge variant="dark" className="h-[18px] shrink-0 px-1.5">
                {when}
              </Badge>
            ) : (
              <span className="shrink-0 text-ink-3">{when}</span>
            )}
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[400px] p-0">
        <div className="px-4 pt-3 text-eyebrow uppercase text-ink-4">{t('status.title')}</div>
        <ul className="m-0 max-h-[420px] list-none overflow-y-auto px-4 py-0">
          {deadlines.map((m) => (
            <AgendaRow key={m.key} m={m} onNavigate={() => setOpen(false)} />
          ))}
        </ul>
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
