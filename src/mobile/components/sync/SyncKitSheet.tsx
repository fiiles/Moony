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
import { Sheet } from '@/mobile/components/Sheet';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
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
 * Settings → Sync → Show Recovery Kit: the device password first, then the kit read-only (QR,
 * account, grouped code). The kit holds the vault key, so it is forgotten when the sheet closes,
 * from the state and from the mutation's result alike. Copying the code works only shortly after
 * the kit was shown; afterwards Rust asks for the password again and this goes back to the
 * password step.
 */
export function SyncKitSheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { showRecoveryKit } = useDeviceSyncMutations();
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  const [again, setAgain] = useState(false);

  const resetShownKit = showRecoveryKit.reset;
  useEffect(() => resetShownKit, [resetShownKit]);
  useEffect(() => {
    if (!open) resetShownKit();
  }, [open, resetShownKit]);
  // Closed: the kit and the "again" sentence go with it
  const [wasOpen, setWasOpen] = useState(open);
  if (wasOpen !== open) {
    setWasOpen(open);
    if (!open) {
      setKit(null);
      setAgain(false);
    }
  }

  return (
    <>
      <SyncPasswordSheet
        open={open && kit === null}
        onOpenChange={onOpenChange}
        title={t('showKit.title')}
        description={again ? t('showKit.again') : t('showKit.description')}
        confirmLabel={t('showKit.confirm')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          setKit(await showRecoveryKit.mutateAsync(password));
        }}
      />
      <KitSheet
        kit={open ? kit : null}
        onOpenChange={onOpenChange}
        onPasswordRequired={() => {
          resetShownKit();
          setKit(null);
          setAgain(true);
        }}
      />
    </>
  );
}

function KitSheet({
  kit,
  onOpenChange,
  onPasswordRequired,
}: {
  kit: RecoveryKit | null;
  onOpenChange: (open: boolean) => void;
  onPasswordRequired: () => void;
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
        await navigator.clipboard.writeText(kit?.recoveryCode ?? '');
        setClipboardHint(true);
      }
      setCopied(true);
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      if (error instanceof ApiError && error.key === 'validation.syncPasswordRequired') {
        onPasswordRequired();
        return;
      }
      fail(error);
    }
  };

  const save = async (kind: 'text' | 'page') => {
    if (!kit || saving) return;
    setSaving(kind);
    try {
      const outcome =
        kind === 'text'
          ? await exportText(SYNC_KIT_TEXT_FILE, recoveryKitText(kit, kitCopy))
          : await exportText(SYNC_KIT_PAGE_FILE, recoveryKitHtml(kit, kitCopy));
      if (outcome === 'saved') toast(t('setup.kit.saved'));
    } catch (error) {
      console.error('Saving the Sync Recovery Kit failed:', errorKind(error));
      fail(error);
    } finally {
      setSaving(null);
    }
  };

  return (
    <Sheet
      open={kit !== null}
      onOpenChange={onOpenChange}
      title={t('showKit.title')}
      description={t('setup.kit.description')}
      footer={
        <Button className={`${BUTTON} flex-1`} onClick={() => onOpenChange(false)}>
          {tc('buttons.close')}
        </Button>
      }
    >
      {kit && (
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
              <span className="text-m-row text-ink num">
                {groupAccountNumber(kit.accountNumber)}
              </span>
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
      )}
    </Sheet>
  );
}
