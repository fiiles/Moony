import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useInsurance } from '@/hooks/use-insurance';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/**
 * What a row of the Finance tab opens until its own screen exists: the policy detail comes in
 * Phase 4 (the loan's is `LoanDetail`). It names the record (a deep link to an id that is gone
 * keeps the tab's name) and says when it is coming.
 */
export function PolicyPlaceholder() {
  const { t } = useTranslation('mobile');
  const back = useBack(routes.finance);
  const [, policyParams] = useRoute(routes.policyDetail);
  const { policies } = useInsurance();

  const title = policies.find((policy) => policy.id === policyParams?.id)?.policyName;

  return (
    <Screen
      title={title ?? t('tabs.finance')}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('placeholder.detail')}</p>
    </Screen>
  );
}
