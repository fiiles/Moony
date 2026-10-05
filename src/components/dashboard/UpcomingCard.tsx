import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SectionHead } from '@/components/shell/PageHead';
import { Card, CardContent } from '@/components/ui/card';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { groupMilestones } from '@/utils/milestones';

/** Rows shown before "Zobrazit vše"; "Teď jednat" fills them first. */
const COLLAPSED_ROWS = 6;

/**
 * "Co vás čeká" (spec 2026-10-05 §7): contract dates, payments, upkeep and
 * crossed targets in two groups. Hidden when nothing is coming up.
 */
export function UpcomingCard() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const { markDone, snooze } = useMilestoneActions();
  const [expanded, setExpanded] = useState(false);

  if (milestones.length === 0) return null;

  const { now, soon } = groupMilestones(milestones);
  const limit = expanded ? Infinity : COLLAPSED_ROWS;
  const shownNow = now.slice(0, limit);
  const shownSoon = soon.slice(0, Math.max(0, limit - shownNow.length));
  const hidden = milestones.length - shownNow.length - shownSoon.length;

  return (
    <section className="mt-9">
      <SectionHead title={t('card.title')} />
      <Card variant="flat">
        <CardContent className="pb-2 pt-4">
          {shownNow.length > 0 && (
            <div>
              <div className="text-eyebrow uppercase text-ink-4">{t('card.now')}</div>
              <MilestoneList milestones={shownNow} onDone={markDone} onSnooze={snooze} />
            </div>
          )}
          {shownSoon.length > 0 && (
            <div className={shownNow.length > 0 ? 'mt-4' : undefined}>
              <div className="text-eyebrow uppercase text-ink-4">{t('card.soon')}</div>
              <MilestoneList milestones={shownSoon} onDone={markDone} onSnooze={snooze} />
            </div>
          )}
          {(hidden > 0 || expanded) && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mb-1 mt-2 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {expanded ? t('card.showLess') : t('card.showAll', { count: milestones.length })}
            </button>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
