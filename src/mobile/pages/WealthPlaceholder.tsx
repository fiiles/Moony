import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import type { RealEstate } from '@shared/schema';
import { realEstateApi } from '@/lib/tauri-api';
import { useBack } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

type Kind = 'crypto' | 'bonds' | 'otherAssets' | 'realEstate' | 'property';

/**
 * What a class row of Majetek opens until its own page exists (the rest of Phase 4). It carries the
 * class's name (a property shows its own; a deep link to an id that is gone keeps the class name)
 * and says when the page is coming.
 */
export default function WealthPlaceholder({ kind }: { kind: Kind }) {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const back = useBack(routes.wealth);
  const [, params] = useRoute(routes.propertyDetail);
  // The same key and query as the desktop's Nemovitosti page, so the cache is shared.
  const { data: properties = [] } = useQuery<RealEstate[]>({
    queryKey: ['real-estate'],
    queryFn: () => realEstateApi.getAll(),
    enabled: kind === 'property',
  });

  const title =
    kind === 'property'
      ? (properties.find((property) => property.id === params?.id)?.name ?? td('cards.realEstate'))
      : td(`cards.${kind}`);

  return (
    <Screen
      title={title}
      leading={<NavTextButton label={t('tabs.wealth')} onClick={back} chevron />}
    >
      <p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('wealth.placeholder.classPage')}</p>
    </Screen>
  );
}

export const CryptoPlaceholder = () => <WealthPlaceholder kind="crypto" />;
export const BondsPlaceholder = () => <WealthPlaceholder kind="bonds" />;
export const OtherAssetsPlaceholder = () => <WealthPlaceholder kind="otherAssets" />;
export const RealEstatePlaceholder = () => <WealthPlaceholder kind="realEstate" />;
export const PropertyPlaceholder = () => <WealthPlaceholder kind="property" />;
