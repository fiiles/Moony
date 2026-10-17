import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useInsurance } from '@/hooks/use-insurance';
import { useLoans } from '@/hooks/use-loans';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

type Kind = 'loan' | 'policy' | 'queue';

/**
 * What a row of the Finance tab opens until its own screen exists: the loan and the policy detail
 * come in Phase 4, the categorize queue in Phase 2b. It names the record (a deep link to an id
 * that is gone keeps the tab's name) and says when it is coming.
 */
export default function FinancePlaceholder({ kind }: { kind: Kind }) {
  const { t } = useTranslation('mobile');
  const back = useBack(routes.finance);
  const [, loanParams] = useRoute(routes.loanDetail);
  const [, policyParams] = useRoute(routes.policyDetail);
  const { loans } = useLoans();
  const { policies } = useInsurance();

  const title =
    kind === 'queue'
      ? t('finance.uncategorized')
      : kind === 'loan'
        ? loans.find((loan) => loan.id === loanParams?.id)?.name
        : policies.find((policy) => policy.id === policyParams?.id)?.policyName;

  return (
    <Screen
      title={title ?? t('tabs.finance')}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">
        {t(kind === 'queue' ? 'placeholder.queue' : 'placeholder.detail')}
      </p>
    </Screen>
  );
}

export const LoanPlaceholder = () => <FinancePlaceholder kind="loan" />;
export const PolicyPlaceholder = () => <FinancePlaceholder kind="policy" />;
export const QueuePlaceholder = () => <FinancePlaceholder kind="queue" />;
