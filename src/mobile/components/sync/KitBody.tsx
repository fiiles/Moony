import { Fragment, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Copy, Download } from 'lucide-react';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRecoveryKitCopy } from '@/components/settings/sync/use-recovery-kit-copy';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useRecoveryKitLifetime } from '@/hooks/use-recovery-kit-lifetime';
import { copyToWebviewClipboard } from '@/utils/recovery-kit-lifetime';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { recoveryCodeGroups, recoveryKitHtml, toBase64 } from '@/utils/recovery-kit';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';
import { errorKind } from '@/mobile/lib/error-kind';
import { SYNC_KIT_PAGE_FILE, exportsOnThisDevice } from '@/mobile/lib/export-file';

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';
const WIDE_BUTTON = `${BUTTON} w-full [&_svg]:size-[18px]`;

/**
 * The recovery kit as the phone shows it, in setup and after a key change: QR, grouped code, and the
 * ways to keep it (copy the code; on iOS "Save or share…" hands the kit's page to the Files picker,
 * from where it can be printed). `onKept` is called once the code was copied or a file was saved. Copying works
 * only shortly after the kit was shown; `onPasswordRequired` is then called instead of a failure
 * toast. After five minutes, or as soon as the app is hidden, `onExpired` is called: the caller
 * drops the kit (state and mutation result) and asks the password again.
 */
export function KitBody({
  kit,
  onKept,
  onPasswordRequired,
  onExpired,
}: {
  kit: RecoveryKit;
  onKept?: () => void;
  onPasswordRequired: () => void;
  onExpired: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { copyRecoveryCode, noteRecoveryKitSaved } = useDeviceSyncMutations();
  const { exportText } = useExportTextFile();
  const kitCopy = useRecoveryKitCopy(kit);
  // Screen capture cannot be switched off per window on a phone: the lifetime is all there is
  useRecoveryKitLifetime(onExpired, false);
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
    },
    []
  );
  const [clipboardHint, setClipboardHint] = useState(false);
  const [saving, setSaving] = useState(false);

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), {
      description: translateApiError(error instanceof Error ? error : String(error), tc),
    });

  const copyCode = async () => {
    try {
      const native = await copyRecoveryCode.mutateAsync();
      if (!native) {
        await copyToWebviewClipboard(kit.recoveryCode);
        setClipboardHint(true);
      }
      setCopied(true);
      onKept?.();
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      // A failed native copy never falls back to the ordinary clipboard (`syncCopyFailed`)
      if (error instanceof ApiError && error.key === 'validation.syncPasswordRequired') {
        onPasswordRequired();
        return;
      }
      fail(error);
    }
  };

  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const outcome = await exportText(SYNC_KIT_PAGE_FILE, recoveryKitHtml(kit, kitCopy));
      if (outcome === 'saved') {
        // Where the picker put it is unknown: it counts as a cloud folder, the strongest
        await noteRecoveryKitSaved.mutateAsync();
        toast(t('setup.kit.saved'));
        onKept?.();
      }
    } catch (error) {
      if (error instanceof ApiError && error.key === 'validation.syncPasswordRequired') {
        onPasswordRequired();
        return;
      }
      console.error('Saving the recovery kit failed:', errorKind(error));
      fail(error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-3.5">
      <div className="flex items-center gap-4">
        <img
          src={`data:image/svg+xml;base64,${toBase64(kit.qrSvg)}`}
          alt={t('setup.kit.file.qrLabel')}
          // colour-literal-ok: a QR code is dark on light in both themes, or it does not scan
          className="size-[120px] flex-none rounded-r2 border border-line-strong bg-white p-1.5"
          data-testid="recovery-kit-qr"
        />
        <div className="grid min-w-0 gap-1">
          {kit.relayUrl && (
            <span className="break-all text-m-caption text-ink-3">
              {t('mobile:labelValue', {
                label: t('setup.kit.file.relayLabel'),
                value: kit.relayUrl,
              })}
            </span>
          )}
        </div>
      </div>
      {/* Lines break only between groups: each group is unbreakable, a break may follow a
          hyphen. The selected text is the code as copied (`<wbr>` adds no character). */}
      <div
        className="select-all rounded-r3 bg-dark px-4 py-3.5 text-center text-m-row leading-relaxed tracking-[0.04em] text-ink-inverse num"
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
      <div className="grid gap-2.5">
        <Button variant="secondary" className={WIDE_BUTTON} onClick={() => void copyCode()}>
          {copied ? <Check aria-hidden /> : <Copy aria-hidden />}
          {copied ? t('setup.kit.copied') : t('setup.kit.copyCode')}
        </Button>
        {/* The Files picker exports on iOS only: elsewhere no button that only apologises */}
        {exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '') && (
          <Button
            variant="secondary"
            className={WIDE_BUTTON}
            loading={saving}
            onClick={() => void save()}
          >
            <Download aria-hidden />
            {t('setup.kit.saveShare')}
          </Button>
        )}
      </div>
      {clipboardHint && (
        <p className="m-0 text-m-sub text-ink-3" data-testid="clipboard-hint">
          {t('setup.kit.clipboardHint')}
        </p>
      )}
      <p className="m-0 text-m-sub text-ink-3">{t('setup.kit.differs')}</p>
      <p className="m-0 text-m-sub text-ink-3">{t('setup.kit.folderWarning')}</p>
    </div>
  );
}
