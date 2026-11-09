import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Copy, FileText, Download } from 'lucide-react';
import type { RecoveryKit } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { groupAccountNumber, recoveryKitHtml, recoveryKitText } from '@/utils/recovery-kit';
import type { RecoveryKitCopy } from '@/utils/recovery-kit';
import { Button } from '@/components/ui/button';

/** The kit's printed lines, in the language the app shows. */
function useRecoveryKitCopy(): RecoveryKitCopy {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  return useMemo(
    () => ({
      title: t('setup.kit.file.title'),
      intro: t('setup.kit.file.intro'),
      accountLabel: t('setup.kit.file.accountLabel'),
      accountNote: t('setup.kit.file.accountNote'),
      codeLabel: t('setup.kit.file.codeLabel'),
      codeNote: t('setup.kit.file.codeNote'),
      qrLabel: t('setup.kit.file.qrLabel'),
      differsFromRecoveryKey: t('setup.kit.differs'),
      created: t('setup.kit.file.created', {
        date: date(new Date(), { dateStyle: 'long' }),
      }),
      relayLabel: t('setup.kit.file.relayLabel'),
      relayNote: t('setup.kit.file.relayNote'),
    }),
    [t, date]
  );
}

interface RecoveryKitPanelProps {
  kit: RecoveryKit;
  /** Called once the kit was saved to a file or its code copied. */
  onKept: () => void;
}

/** The Sync Recovery Kit as shown during setup: QR, code, account, and the ways to keep it. */
export function RecoveryKitPanel({ kit, onKept }: RecoveryKitPanelProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { copyRecoveryCode } = useDeviceSyncMutations();
  const kitCopy = useRecoveryKitCopy();
  const [copied, setCopied] = useState(false);
  const [clipboardHint, setClipboardHint] = useState(false);

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  const saveFile = async (kind: 'html' | 'txt') => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { writeTextFile } = await import('@tauri-apps/plugin-fs');
      const path = await save({
        defaultPath: `moony-sync-recovery-kit.${kind === 'html' ? 'html' : 'txt'}`,
        filters:
          kind === 'html'
            ? [{ name: 'HTML', extensions: ['html'] }]
            : [{ name: 'Text', extensions: ['txt'] }],
      });
      if (!path) return;
      await writeTextFile(
        path,
        kind === 'html' ? recoveryKitHtml(kit, kitCopy) : recoveryKitText(kit, kitCopy)
      );
      toast(t('setup.kit.saved'));
      onKept();
    } catch (error) {
      fail(error);
    }
  };

  const copyCode = async () => {
    try {
      const native = await copyRecoveryCode.mutateAsync();
      if (!native) {
        await navigator.clipboard.writeText(kit.recoveryCode);
        setClipboardHint(true);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
      onKept();
    } catch (error) {
      fail(error);
    }
  };

  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-4">
        <img
          src={`data:image/svg+xml;base64,${btoa(kit.qrSvg)}`}
          alt={t('setup.kit.file.qrLabel')}
          className="size-[120px] shrink-0 rounded-r2 border border-line-strong bg-white p-1.5"
          data-testid="recovery-kit-qr"
        />
        <div className="min-w-0 grid gap-1">
          <span className="text-caption font-700 text-ink-2">
            {t('setup.kit.file.accountLabel')}
          </span>
          <span className="num text-body font-600 text-ink">
            {groupAccountNumber(kit.accountNumber)}
          </span>
          {kit.relayUrl && (
            <span className="break-all text-micro font-500 text-ink-3">
              {t('setup.kit.file.relayLabel')}: {kit.relayUrl}
            </span>
          )}
        </div>
      </div>
      <div
        className="select-all break-all rounded-r3 bg-dark px-4 py-[14px] text-center text-[14px] font-650 leading-relaxed tracking-[0.04em] text-ink-inverse num"
        data-testid="recovery-kit-code"
      >
        {kit.recoveryCode}
      </div>
      <div className="grid grid-cols-3 gap-2">
        <Button type="button" variant="outline" onClick={copyCode}>
          {copied ? <Check /> : <Copy />}
          {copied ? t('setup.kit.copied') : t('setup.kit.copyCode')}
        </Button>
        <Button type="button" variant="outline" onClick={() => saveFile('html')}>
          <Download />
          {t('setup.kit.savePage')}
        </Button>
        <Button type="button" variant="outline" onClick={() => saveFile('txt')}>
          <FileText />
          {t('setup.kit.saveText')}
        </Button>
      </div>
      {clipboardHint && (
        <p className="m-0 text-micro font-500 text-ink-3" data-testid="clipboard-hint">
          {t('setup.kit.clipboardHint')}
        </p>
      )}
      <p className="m-0 text-micro font-500 text-ink-3">{t('setup.kit.differs')}</p>
    </div>
  );
}
