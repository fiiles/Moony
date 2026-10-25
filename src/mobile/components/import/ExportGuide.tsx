import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ChevronLeft, ExternalLink } from 'lucide-react';
import { toast } from 'sonner';
import { regionName } from '@/components/bank-accounts/csv-import/import-config';
import { Button } from '@/components/ui/button';
import { useLanguage } from '@/i18n/I18nProvider';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { guideBankName, guideSteps, type GuidedPreset } from '@/mobile/lib/export-guide';
import { initials } from '@/mobile/lib/initials';

/**
 * How to get a statement file out of one bank on a phone (spec §8.6, rung 2; README §13 "Export
 * guide"): the section head, the bank's three steps as a numbered list in the person's language
 * (13 / 450 `ink-2`), the format to pick as a caption and — for a bank that is reached through its
 * web banking — a secondary button that opens it in the system browser. The steps are the preset's
 * own data; nothing here goes to the network until that button is tapped.
 */
export function ExportGuide({ preset }: { preset: GuidedPreset }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { language } = useLanguage();
  const headId = useId();
  const guide = preset.mobileExport;
  const steps = guideSteps(guide, language);
  const webUrl = guide.route === 'web' ? guide.webUrl : null;
  if (steps.length === 0) return null;

  const openWeb = async () => {
    if (!webUrl) return;
    try {
      await openUrl(webUrl);
    } catch {
      toast.error(tc('status.error'), { description: t('import.guide.openFailed') });
    }
  };

  return (
    <section aria-labelledby={headId} className="mb-2 mt-[18px]">
      <h3 id={headId} className="mx-1 mb-1.5 mt-0 text-m-section text-ink">
        {t('import.guide.title', { bank: guideBankName(preset.bankName) })}
      </h3>
      <ol className="m-0 list-none p-0">
        {steps.map((step, index) => (
          <li
            key={index}
            className="grid grid-cols-[22px_minmax(0,1fr)] items-start gap-2.5 px-1 py-[7px] text-m-lead leading-[1.45] text-ink-2 [overflow-wrap:break-word]"
          >
            <span
              aria-hidden
              className="mt-px grid size-[22px] place-items-center rounded-full bg-well-2 text-m-caption font-bold text-ink-3 num"
            >
              {index + 1}
            </span>
            <span className="min-w-0">{step}</span>
          </li>
        ))}
      </ol>
      <p className="mx-1 mb-0 mt-1.5 text-m-caption text-ink-3">
        {t('import.guide.format', { format: guide.format })}
      </p>
      {webUrl && (
        <Button
          variant="secondary"
          className="mt-2.5 h-auto min-h-11 whitespace-normal py-2 text-center max-w-full rounded-[10px] px-4 text-m-sub [&_svg]:size-4"
          onClick={() => void openWeb()}
        >
          <ExternalLink aria-hidden strokeWidth={1.75} />
          {t('import.guide.openWeb')}
        </Button>
      )}
    </section>
  );
}

/** The 28 pt chip of a bank row: its initials (`ČS`), as the entity rows' chips carry theirs. */
function BankChip({ name }: { name: string }) {
  return (
    <span
      aria-hidden
      className="grid size-7 shrink-0 place-items-center rounded-full bg-well-2 text-m-caption font-bold text-ink-2"
    >
      {initials(name)}
    </span>
  );
}

/**
 * "Návody bank": the banks that have a guide, as a list of plain rows (chip with the initials, the
 * name, its country), and the tapped bank's guide in place with a way back to the list. It is what a
 * file that cannot be read offers — the person may have the wrong file from the right bank.
 */
export function BankGuides({ presets }: { presets: readonly GuidedPreset[] }) {
  const { t } = useTranslation('mobile');
  const { getLocale } = useLanguage();
  const [openId, setOpenId] = useState<string | null>(null);
  const top = useRef<HTMLDivElement>(null);
  const shown = useRef(openId);
  const opened = presets.find((preset) => preset.id === openId);

  // The sheet's body scrolls: a guide opened from the far end of the list (or the list shown again
  // from the end of a guide) starts at its own top, not wherever the shorter content left the scroll
  useEffect(() => {
    if (shown.current === openId) return;
    shown.current = openId;
    top.current?.scrollIntoView({ block: 'start' });
  }, [openId]);

  if (presets.length === 0) return null;

  if (opened) {
    return (
      <div ref={top} className="mt-2">
        <Button
          type="button"
          variant="ghost"
          className="h-auto min-h-11 whitespace-normal py-2 text-left w-full justify-start gap-1 rounded-[10px] px-2 text-m-sub text-ink"
          onClick={() => setOpenId(null)}
        >
          <ChevronLeft aria-hidden strokeWidth={1.75} className="!size-4" />
          {t('import.guide.banks')}
        </Button>
        <ExportGuide preset={opened} />
      </div>
    );
  }

  return (
    <div ref={top}>
      <PickGroup title={t('import.guide.banks')} role="group">
        {presets.map((preset) => {
          const name = guideBankName(preset.bankName);
          return (
            <PickRow
              key={preset.id}
              icon={<BankChip name={name} />}
              label={name}
              sub={regionName(preset.country, getLocale())}
              onSelect={() => setOpenId(preset.id)}
            />
          );
        })}
      </PickGroup>
    </div>
  );
}
