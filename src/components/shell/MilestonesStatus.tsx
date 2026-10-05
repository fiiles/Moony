import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { StatusText } from '@/components/shell/DataStatus';
import { AgendaRow } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { agendaDay, daysBetween, relativeLabel, upcomingDeadlines } from '@/utils/milestones';

/**
 * Top-bar status (spec 2026-10-05-milestones-agenda §2.5): only a deadline at most 14 days
 * away, the nearest one by name ("Spořicí účet · za 10 dní", +N for more). Information,
 * upkeep and targets never show here; most weeks it is absent.
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

  return (
    <Popover open={open && deadlines.length > 0} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="min-w-0 max-w-[320px] rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <StatusText className="min-w-0 text-ink-2">
            <span className="truncate">
              {t('status.deadline', {
                title: first.title,
                when: t(`relative.${rel.key}`, { count: rel.count }),
              })}
            </span>
            {deadlines.length > 1 && (
              <span className="shrink-0 text-ink-4">
                {t('status.more', { count: deadlines.length - 1 })}
              </span>
            )}
          </StatusText>
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
