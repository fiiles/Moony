import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SectionHead } from '@/components/shell/PageHead';
import { Card, CardContent } from '@/components/ui/card';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { utcDayFloor } from '@/utils/chart-axis';
import { splitMilestones } from '@/utils/milestones';

/** Rows shown before "Zobrazit vše". */
const COLLAPSED_ROWS = 6;

/**
 * "Co vás čeká" (spec 2026-10-05 §7): contract dates, payments, upkeep and
 * crossed targets. Hidden when nothing is coming up.
 */
export function UpcomingCard() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const { hide, remindLater } = useMilestoneActions();
  const [expanded, setExpanded] = useState(false);

  if (milestones.length === 0) return null;

  const { agenda, upkeep } = splitMilestones(milestones, utcDayFloor(Date.now() / 1000));
  const rows = [...agenda, ...upkeep];
  const shown = expanded ? rows : rows.slice(0, COLLAPSED_ROWS);

  return (
    <section className="mt-9">
      <SectionHead title={t('card.title')} />
      <Card variant="flat">
        <CardContent className="pb-2 pt-4">
          <MilestoneList milestones={shown} onHide={hide} onRemindLater={remindLater} />
          {rows.length > COLLAPSED_ROWS && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mb-1 mt-2 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {expanded ? t('card.showLess') : t('card.showAll', { count: rows.length })}
            </button>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
