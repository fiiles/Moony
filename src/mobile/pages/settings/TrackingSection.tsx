import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import {
  areasFromPreferences,
  preferencesWithAreas,
  type TrackedAreas,
} from '@/mobile/lib/tracked-areas';

/** The card the section's controls sit in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

/**
 * Nastavení → Co sledovat: the three areas of the first run's step, switched here later, and the
 * desktop's "Vyloučit osobní nemovitosti z přehledů". Hiding an area only hides it (its data
 * stays), and the Majetek and Finance tabs follow the stored menu preferences.
 */
export default function TrackingSection() {
  const { t } = useTranslation('mobile');
  const { t: ta } = useTranslation('auth');
  const { t: ts } = useTranslation('settings');
  const { user } = useAuth();
  const { updateMenuPreferences, updateProfile } = useSettingsMutations();

  const areas = areasFromPreferences(user?.menuPreferences);
  const setArea = (key: keyof TrackedAreas, on: boolean) => {
    if (updateMenuPreferences.isPending) return;
    updateMenuPreferences.mutate(
      preferencesWithAreas(user?.menuPreferences, { ...areas, [key]: on })
    );
  };
  const busy = !user || updateMenuPreferences.isPending;

  return (
    <SectionPage title={t('settings.rows.tracking')} lead={t('settings.tracking.lead')}>
      <div className={CARD}>
        <SwitchRow
          id="area-accounts"
          label={ta('onboarding.areas.accounts.title')}
          sub={ta('onboarding.areas.alwaysOn')}
          checked
          disabled
          onCheckedChange={() => undefined}
        />
        <SwitchRow
          id="area-investments"
          label={ta('onboarding.areas.investments.title')}
          sub={ta('onboarding.areas.investments.description')}
          checked={areas.investments}
          disabled={busy}
          onCheckedChange={(on) => setArea('investments', on)}
        />
        <SwitchRow
          id="area-real-estate"
          label={ta('onboarding.areas.realEstate.title')}
          sub={ta('onboarding.areas.realEstate.description')}
          checked={areas.realEstate}
          disabled={busy}
          onCheckedChange={(on) => setArea('realEstate', on)}
        />
        <SwitchRow
          id="area-liabilities"
          label={ta('onboarding.areas.liabilities.title')}
          sub={ta('onboarding.areas.liabilities.description')}
          checked={areas.liabilities}
          disabled={busy}
          onCheckedChange={(on) => setArea('liabilities', on)}
        />
      </div>
      <p className="mx-1 mb-4 mt-0 text-m-sub text-ink-3">{t('settings.tracking.hideOnly')}</p>

      <div className={CARD}>
        <SwitchRow
          id="exclude-personal-real-estate"
          label={ts('dashboardPreferences.excludeRealEstate')}
          sub={ts('dashboardPreferences.excludeRealEstateHint')}
          checked={user?.excludePersonalRealEstate ?? false}
          disabled={!user || updateProfile.isPending}
          onCheckedChange={(on) => {
            if (!updateProfile.isPending) updateProfile.mutate({ excludePersonalRealEstate: on });
          }}
        />
      </div>
    </SectionPage>
  );
}
