import { Fragment, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Copy, Download, FileText } from 'lucide-react';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRecoveryKitCopy } from '@/components/settings/sync/use-recovery-kit-copy';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import {
  groupAccountNumber,
  recoveryCodeGroups,
  recoveryKitHtml,
  recoveryKitText,
  toBase64,
} from '@/utils/recovery-kit';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';
import { errorKind } from '@/mobile/lib/error-kind';
import {
  SYNC_KIT_PAGE_FILE,
  SYNC_KIT_TEXT_FILE,
  exportsOnThisDevice,
} from '@/mobile/lib/export-file';

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';
const WIDE_BUTTON = `${BUTTON} w-full [&_svg]:size-[18px]`;

/**
 * The Sync Recovery Kit as the phone shows it, in Show Recovery Kit and in setup: QR, account,
 * grouped code, and the ways to keep it (copy the code; on iOS save it as text or as a page through
 * the Files picker). `onKept` is called once the code was copied or a file was saved. With sync on,
 * copying works only shortly after the kit was shown; `onPasswordRequired` is then called instead
 * of a failure toast.
 */
export function KitBody({
  kit,
  onKept,
  onPasswordRequired,
}: {
  kit: RecoveryKit;
  onKept?: () => void;
  onPasswordRequired?: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { copyRecoveryCode } = useDeviceSyncMutations();
  const { exportText } = useExportTextFile();
  const kitCopy = useRecoveryKitCopy();
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
    },
    []
  );
  const [clipboardHint, setClipboardHint] = useState(false);
  const [saving, setSaving] = useState<'text' | 'page' | null>(null);

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), {
      description: translateApiError(error instanceof Error ? error : String(error), tc),
    });

  const copyCode = async () => {
    try {
      const native = await copyRecoveryCode.mutateAsync();
      if (!native) {
        await navigator.clipboard.writeText(kit.recoveryCode);
        setClipboardHint(true);
      }
      setCopied(true);
      onKept?.();
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      if (
        onPasswordRequired &&
        error instanceof ApiError &&
        error.key === 'validation.syncPasswordRequired'
      ) {
        onPasswordRequired();
        return;
      }
      fail(error);
    }
  };

  const save = async (kind: 'text' | 'page') => {
    if (saving) return;
    setSaving(kind);
    try {
      const outcome =
        kind === 'text'
          ? await exportText(SYNC_KIT_TEXT_FILE, recoveryKitText(kit, kitCopy))
          : await exportText(SYNC_KIT_PAGE_FILE, recoveryKitHtml(kit, kitCopy));
      if (outcome === 'saved') {
        toast(t('setup.kit.saved'));
        onKept?.();
      }
    } catch (error) {
      console.error('Saving the Sync Recovery Kit failed:', errorKind(error));
      fail(error);
    } finally {
      setSaving(null);
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
          <span className="text-m-sub font-semibold text-ink-3">
            {t('setup.kit.file.accountLabel')}
          </span>
          <span className="text-m-row text-ink num">{groupAccountNumber(kit.accountNumber)}</span>
          {kit.relayUrl && (
            <span className="break-all text-m-caption text-ink-3">
              {t('setup.kit.file.relayLabel')}: {kit.relayUrl}
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
          <>
            <Button
              variant="secondary"
              className={WIDE_BUTTON}
              loading={saving === 'text'}
              disabled={saving !== null}
              onClick={() => void save('text')}
            >
              <FileText aria-hidden />
              {t('phone.saveText')}
            </Button>
            <Button
              variant="secondary"
              className={WIDE_BUTTON}
              loading={saving === 'page'}
              disabled={saving !== null}
              onClick={() => void save('page')}
            >
              <Download aria-hidden />
              {t('phone.savePage')}
            </Button>
          </>
        )}
      </div>
      {clipboardHint && (
        <p className="m-0 text-m-sub text-ink-3" data-testid="clipboard-hint">
          {t('setup.kit.clipboardHint')}
        </p>
      )}
      <p className="m-0 text-m-sub text-ink-3">{t('setup.kit.differs')}</p>
    </div>
  );
}
