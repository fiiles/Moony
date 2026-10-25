import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { BookOpen, FileSpreadsheet } from 'lucide-react';
import type { LearnedCsvFormat } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useFormat } from '@/lib/use-format';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { useImportSheet } from '@/mobile/hooks/use-import-sheet';
import { useLearnedFormats } from '@/mobile/hooks/use-learned-formats';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { learnedFormatSub } from '@/mobile/lib/learned-format';
import { ExportGuidesSheet } from '@/mobile/sheets/ExportGuidesSheet';
import { LearnedFormatSheet } from '@/mobile/sheets/LearnedFormatSheet';

/** The 48 pt primary under the lead, as the other section pages' buttons. */
const BUTTON =
  'h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row';

/**
 * Nastavení → Import (spec §8.6 rung 4, §8.9): "Importovat výpis" (the import sheet every entry
 * point shares, asked here without an account), the layouts the person confirmed once — "Naučené
 * formáty", each a row that opens its sheet to rename or delete it — and the export guides of the
 * banks in a sheet. A format's row says which account it was learned for (left out when that
 * account is gone), when, when it was last used, and that the bank has a built-in format now when
 * a shipped preset matches its layout (the preset then reads the file; the learned format waits).
 * No format yet: one sentence on how one comes to be.
 */
export default function ImportSection() {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const { openImport } = useImportSheet();
  const { accounts } = useBankAccounts();
  const { formats, isPending } = useLearnedFormats();
  const [openId, setOpenId] = useState<string | null>(null);
  const [guidesOpen, setGuidesOpen] = useState(false);

  // A format that is gone (deleted, or the list reloaded without it) closes its sheet
  const opened = formats.find((format) => format.id === openId) ?? null;

  // Created and last-used are real instants (the store's clock), written short like a sub-line's day
  const day = (unixSeconds: number) =>
    fmt.date(new Date(unixSeconds * 1000), shortDayOptions(unixSeconds));
  const subOf = (format: LearnedCsvFormat): string => {
    const { account, created, lastUsed } = learnedFormatSub(format, accounts);
    const when = { created: day(created), lastUsed: day(lastUsed) };
    const text =
      account === null
        ? t('settings.import.subNoAccount', when)
        : t('settings.import.sub', { account, ...when });
    return format.shadowedByPresetId ? `${text} · ${t('settings.import.shadowed')}` : text;
  };

  return (
    <SectionPage title={t('settings.rows.import')} lead={t('settings.import.lead')}>
      <Button className={BUTTON} onClick={() => openImport({ source: 'settings' })}>
        {t('account.import')}
      </Button>

      <SectionHead title={t('settings.import.formats')} />
      {formats.length > 0 ? (
        <EntityRows>
          {formats.map((format) => (
            <SettingsRow
              key={format.id}
              icon={FileSpreadsheet}
              title={format.name}
              sub={subOf(format)}
              onClick={() => setOpenId(format.id)}
            />
          ))}
        </EntityRows>
      ) : (
        // Nothing is said until the list has been read: an empty store and a loading one differ
        !isPending && (
          <p className="mx-0.5 mb-3.5 mt-0 text-m-caption text-ink-4">
            {t('settings.import.empty')}
          </p>
        )
      )}

      <SectionHead title={t('settings.import.guides')} />
      <EntityRows>
        <SettingsRow
          icon={BookOpen}
          title={t('settings.import.guidesRow')}
          onClick={() => setGuidesOpen(true)}
        />
      </EntityRows>

      <LearnedFormatSheet
        format={opened}
        accounts={accounts}
        onOpenChange={(open) => {
          if (!open) setOpenId(null);
        }}
      />
      <ExportGuidesSheet open={guidesOpen} onOpenChange={setGuidesOpen} />
    </SectionPage>
  );
}
