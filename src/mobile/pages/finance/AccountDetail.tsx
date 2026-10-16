import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** The account detail (spec §8.4): a stub with the account's name until Task 2 builds the screen. */
export default function AccountDetail() {
  const { t } = useTranslation('mobile');
  const back = useBack(routes.finance);
  const [, params] = useRoute(routes.accountDetail);
  const { account } = useBankAccount(params?.id);
  return (
    <Screen
      title={account?.name ?? t('tabs.finance')}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('shell.placeholder')}</p>
    </Screen>
  );
}
