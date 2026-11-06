import { useTranslation } from 'react-i18next';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { plansPath } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Placeholder until the plan detail page (Task 4) replaces it. */
export default function PlanDetail() {
  const { t } = useTranslation('spending_plans');
  const back = useBack(plansPath());
  return (
    <Screen
      title={t('eyebrow')}
      leading={<NavTextButton chevron label={t('title')} onClick={back} />}
    >
      {null}
    </Screen>
  );
}
