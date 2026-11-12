import { Fragment, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Copy, FileText, Download } from 'lucide-react';
import type { RecoveryKit } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useRecoveryKitLifetime } from '@/hooks/use-recovery-kit-lifetime';
import { copyToWebviewClipboard } from '@/utils/recovery-kit-lifetime';
import { ApiError } from '@/lib/api-error';
import { deviceSyncApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import {
  groupAccountNumber,
  recoveryCodeGroups,
  toBase64,
  recoveryKitHtml,
  recoveryKitText,
} from '@/utils/recovery-kit';
import type { KitFileKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRecoveryKitCopy } from '@/components/settings/sync/use-recovery-kit-copy';

interface RecoveryKitPanelProps {
  kit: RecoveryKit;
  /** Called once the kit was saved to a file or its code copied. */
  onKept: () => void;
  /**
   * Copying the code or saving a file works only shortly after the kit was shown; afterwards the
   * backend asks for the password again, and this is called instead of a failure toast.
   */
  onPasswordRequired: () => void;
  /**
   * The kit has been on screen for five minutes, or the app was hidden: the caller drops it (state
   * and mutation result) and asks the password again before showing it. Screen capture is off
   * while this panel is mounted.
   */
  onExpired: () => void;
}

/** The Sync Recovery Kit as shown during setup: QR, code, account, and the ways to keep it. */
export function RecoveryKitPanel({
  kit,
  onKept,
  onPasswordRequired,
  onExpired,
}: RecoveryKitPanelProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { copyRecoveryCode } = useDeviceSyncMutations();
  const kitCopy = useRecoveryKitCopy();
  const [copied, setCopied] = useState(false);
  const [clipboardHint, setClipboardHint] = useState(false);
  useRecoveryKitLifetime(onExpired, true);

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  const passwordRequired = (error: unknown) =>
    error instanceof ApiError && error.key === 'validation.syncPasswordRequired';

  const saveFile = async (kind: KitFileKind) => {
    try {
      const outcome = await deviceSyncApi.saveRecoveryKit(
        kind,
        kind === 'page' ? recoveryKitHtml(kit, kitCopy) : recoveryKitText(kit, kitCopy)
      );
      // The save dialog was closed: nothing was written
      if (!outcome.saved) return;
      if (outcome.syncedFolder) toast.warning(t('setup.kit.syncedFolder'));
      else toast(t('setup.kit.saved'));
      onKept();
    } catch (error) {
      if (passwordRequired(error)) {
        onPasswordRequired();
        return;
      }
      // Rust refuses a file that is not this account's current kit (setup restarted meanwhile)
      if (error instanceof ApiError && error.kind === 'internal') {
        toast.error(tc('status.error'), { description: t('setup.kit.saveRefused') });
        return;
      }
      fail(error);
    }
  };

  const copyCode = async () => {
    try {
      const native = await copyRecoveryCode.mutateAsync();
      if (!native) {
        await copyToWebviewClipboard(kit.recoveryCode);
        setClipboardHint(true);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
      onKept();
    } catch (error) {
      // A failed native copy never falls back to the ordinary clipboard (`syncCopyFailed`)
      if (passwordRequired(error)) {
        onPasswordRequired();
        return;
      }
      fail(error);
    }
  };

  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-4">
        <img
          src={`data:image/svg+xml;base64,${toBase64(kit.qrSvg)}`}
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
      {/* Lines break only between groups: each group is unbreakable, a break may follow a hyphen.
          The selected text is the code exactly as copied (`<wbr>` adds no character). */}
      <div
        className="select-all rounded-r3 bg-dark px-4 py-[14px] text-center text-[14px] font-650 leading-relaxed tracking-[0.04em] text-ink-inverse num"
        data-testid="recovery-kit-code"
      >
        {recoveryCodeGroups(kit.recoveryCode).map((group, index) => (
          <Fragment key={index}>
            {index > 0 && (
              <>
                -<wbr />
              </>
            )}
            <span className="whitespace-nowrap">{group}</span>
          </Fragment>
        ))}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" onClick={copyCode}>
          {copied ? <Check /> : <Copy />}
          {copied ? t('setup.kit.copied') : t('setup.kit.copyCode')}
        </Button>
        <Button type="button" variant="outline" onClick={() => saveFile('page')}>
          <Download />
          {t('setup.kit.savePage')}
        </Button>
        <Button type="button" variant="outline" onClick={() => saveFile('text')}>
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
      <p className="m-0 text-micro font-500 text-ink-3">{t('setup.kit.folderWarning')}</p>
    </div>
  );
}
