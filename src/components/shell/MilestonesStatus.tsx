import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { StatusText } from '@/components/shell/DataStatus';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { groupMilestones } from '@/utils/milestones';

/**
 * "2 k vyřízení" in the top bar (spec 2026-10-05 §7): the "Teď jednat" items on
 * every page, opened in a popover. Hidden when there is nothing to do.
 */
export function MilestonesStatus() {
  const { t } = useTranslation('milestones');
  const [open, setOpen] = useState(false);
  const { data: milestones = [] } = useMilestones();
  const { markDone, snooze } = useMilestoneActions();
  const { now } = groupMilestones(milestones);

  if (now.length === 0) return null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <StatusText className="text-ink-2">{t('status.count', { count: now.length })}</StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[380px] p-0">
        <div className="px-4 pt-3 text-eyebrow uppercase text-ink-4">{t('status.title')}</div>
        <div className="max-h-[420px] overflow-y-auto px-4">
          <MilestoneList
            milestones={now}
            onDone={markDone}
            onSnooze={snooze}
            onNavigate={() => setOpen(false)}
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
