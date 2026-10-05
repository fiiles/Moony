import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Wrench } from 'lucide-react';
import { SectionHead } from '@/components/shell/PageHead';
import { Card, CardContent } from '@/components/ui/card';
import { AgendaRow, UpkeepRow, upkeepShort } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { columns, splitMilestones } from '@/utils/milestones';

/** Agenda rows shown before "Zobrazit vše" (three per column). */
const COLLAPSED_ROWS = 6;

/**
 * "Co vás čeká" (spec 2026-10-05-milestones-agenda): a two-column agenda with date chips,
 * filled top to bottom, and data upkeep folded into one line under it. Hidden when empty.
 */
export function UpcomingCard() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const [expanded, setExpanded] = useState(false);
  const [upkeepOpen, setUpkeepOpen] = useState(false);

  if (milestones.length === 0) return null;

  const today = utcDayFloor(Date.now() / 1000);
  const { agenda, upkeep } = splitMilestones(milestones, today);
  const [left, right] = columns(expanded ? agenda : agenda.slice(0, COLLAPSED_ROWS));

  return (
    <section className="mt-9">
      <SectionHead
        title={t('card.title')}
        link={{ href: '/settings', label: t('card.settingsLink') }}
      />
      <Card variant="flat">
        <CardContent className="pb-3 pt-2">
          {agenda.length === 0 ? (
            <p className="m-0 py-3 text-table text-ink-3">{t('card.nothingDue')}</p>
          ) : (
            <div className="grid grid-cols-2 gap-x-7">
              {[left, right].map((column, i) => (
                <ul key={i} className="m-0 list-none p-0">
                  {column.map((m) => (
                    <AgendaRow key={m.key} m={m} />
                  ))}
                </ul>
              ))}
            </div>
          )}
          {agenda.length > COLLAPSED_ROWS && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mt-2 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {expanded ? t('card.showLess') : t('card.showAll', { count: agenda.length })}
            </button>
          )}
          {upkeep.length > 0 && (
            <div className="mt-2 border-t border-line-soft pt-2.5">
              <button
                type="button"
                aria-expanded={upkeepOpen}
                onClick={() => setUpkeepOpen((v) => !v)}
                className="flex w-full min-w-0 items-center gap-2.5 text-left text-caption font-500 text-ink-3 hover:text-ink-2 focus-visible:outline-none focus-visible:shadow-focus"
              >
                <Wrench
                  className="size-[14px] shrink-0 text-ink-4"
                  strokeWidth={1.75}
                  aria-hidden
                />
                <span className="min-w-0 flex-1 truncate">
                  <b className="font-650 text-ink-2">{t('card.upkeep')}</b>
                  {' · '}
                  {upkeep.map((m) => upkeepShort(m, t)).join(' · ')}
                </span>
                <span className="shrink-0 font-650 text-ink-2">
                  {upkeepOpen
                    ? t('card.upkeepHide')
                    : t('card.upkeepShow', { count: upkeep.length })}
                </span>
              </button>
              {upkeepOpen && (
                <ul className="m-0 mt-1 grid list-none grid-cols-2 gap-x-7 p-0">
                  {upkeep.map((m) => (
                    <UpkeepRow key={m.key} m={m} />
                  ))}
                </ul>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
