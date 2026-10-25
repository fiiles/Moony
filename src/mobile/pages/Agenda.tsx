import { useTranslation } from 'react-i18next';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { splitMilestones } from '@/utils/milestones';
import { AgendaRow } from '@/mobile/components/AgendaRow';
import { EntityRows } from '@/mobile/components/EntityRow';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** "Co vás čeká" in full (spec §8.3 item 4: "Zobrazit vše (N)"), pushed on Přehled. */
export default function Agenda() {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('milestones');
  const back = useBack(routes.overview);
  const { data: milestones = [], isLoading } = useMilestones();
  const { agenda } = splitMilestones(milestones, utcDayFloor(Date.now() / 1000));

  return (
    <Screen
      title={tm('card.title')}
      leading={<NavTextButton label={t('tabs.overview')} onClick={back} chevron />}
    >
      {agenda.length > 0 ? (
        <EntityRows className="mt-2">
          {agenda.map((m) => (
            <AgendaRow key={m.key} m={m} from={tm('card.title')} />
          ))}
        </EntityRows>
      ) : (
        !isLoading && <p className="mx-0.5 mt-2 text-m-body text-ink-3">{tm('card.nothingDue')}</p>
      )}
    </Screen>
  );
}
