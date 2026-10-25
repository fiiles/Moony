import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from 'lucide-react';
import { FX_SUPPORTED_CODES, currencyName } from '@shared/currencies';
import type { BackupInspection } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { useRestoreBackup } from '@/mobile/hooks/use-restore-backup';
import { CountryPickerSheet } from '@/mobile/sheets/CountryPickerSheet';
import { CurrencyPickerSheet } from '@/mobile/sheets/CurrencyPickerSheet';
import { RestoreBackupSheet } from '@/mobile/sheets/RestoreBackupSheet';
import { StepHead } from './StepHead';
import { caption, primaryButton, textLink } from './styles';

const PRIVACY_POINTS = ['encryption', 'localOnly', 'marketData'] as const;

/**
 * Soukromí: what the app does with the data (the phone's own three points, `firstRun.points.*`:
 * the desktop's speak of ordinary files beside the database and of a data folder to back up), and
 * the two choices that shape the rest: the main currency (only one with a rate source) and the
 * banking country (rule packs, the import's bank order). Each opens a picker sheet; the hints under
 * the rows are the desktop's.
 */
export function PrivacyStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const [currencyOpen, setCurrencyOpen] = useState(false);
  const [countryOpen, setCountryOpen] = useState(false);
  const { pickRestore, restore, restoring } = useRestoreBackup();
  const [inspection, setInspection] = useState<BackupInspection | null>(null);
  const chooseRestore = async () => {
    const picked = await pickRestore();
    if (picked) setInspection(picked);
  };
  // `restore` has told the person what went wrong: the sheet stays open to try again or cancel;
  // on success the page reloads to the restored profile's lock screen
  const confirmRestore = () => {
    if (inspection) void restore(inspection).catch(() => undefined);
  };

  return (
    <div data-testid="first-run-privacy">
      <StepHead title={t('onboarding.welcome.title')} lead={tm('firstRun.privacyLead')} />

      <div className="mb-4 grid gap-2 rounded-r3 bg-well px-4 py-3.5 text-m-sub text-ink-2">
        {PRIVACY_POINTS.map((key) => (
          <div key={key} className="flex items-start gap-2">
            <Check className="mt-px size-3.5 shrink-0 text-gain" aria-hidden />
            <span>
              <b className="font-650 text-ink">{tm(`firstRun.points.${key}.title`)}</b>{' '}
              {tm(`firstRun.points.${key}.text`)}
            </span>
          </div>
        ))}
      </div>

      <div className="mb-5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1">
        <div className="pb-2.5">
          <KeyValueRow
            label={t('onboarding.welcome.currency')}
            value={`${flow.currency} · ${currencyName(flow.currency, fmt.locale)}`}
            onClick={() => setCurrencyOpen(true)}
          />
          <p className={caption}>{t('onboarding.welcome.currencyHint')}</p>
        </div>
        <div className="pb-2.5">
          <KeyValueRow
            label={t('onboarding.welcome.country')}
            value={flow.countryName ?? t('onboarding.welcome.countryNone')}
            onClick={() => setCountryOpen(true)}
          />
          <p className={caption}>{t('onboarding.welcome.countryHint')}</p>
        </div>
      </div>

      <Button className={primaryButton} onClick={flow.goNext}>
        {tc('buttons.continue')}
      </Button>

      {/* The desktop wizard's way in for someone moving to this phone or reinstalling: the
          restored profile replaces the first run, and its own password unlocks it */}
      <p className={cn(caption, 'mt-5 text-center')}>{t('onboarding.welcome.restoreLead')}</p>
      <button
        type="button"
        className={cn(textLink, 'mt-0')}
        disabled={restoring}
        onClick={() => void chooseRestore()}
        data-testid="first-run-restore-backup"
      >
        {t('onboarding.welcome.restoreBackup')}
      </button>

      <RestoreBackupSheet
        fresh
        inspection={inspection}
        onOpenChange={(open) => {
          if (!open) setInspection(null);
        }}
        onConfirm={confirmRestore}
        isPending={restoring}
      />
      <CurrencyPickerSheet
        open={currencyOpen}
        onOpenChange={setCurrencyOpen}
        value={flow.currency}
        onPick={flow.setCurrency}
        options={FX_SUPPORTED_CODES}
      />
      <CountryPickerSheet
        open={countryOpen}
        onOpenChange={setCountryOpen}
        value={flow.country}
        onPick={flow.setCountry}
      />
    </div>
  );
}
