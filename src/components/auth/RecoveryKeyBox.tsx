import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Copy, Download } from 'lucide-react';
import { recoveryKeyFileContents } from '@/utils/recovery-key-check';
import { translateApiError } from '@/lib/translate-api-error';
import { Button } from '@/components/ui/button';

interface RecoveryKeyBoxProps {
  recoveryKey: string;
  /** Called once the key was saved to a file (the wizard remembers it). */
  onSavedToFile?: () => void;
}

/**
 * The recovery key as the dark key box with "Zkopírovat" and "Uložit do
 * souboru…" (prototypes onboarding.html and lock.html).
 */
export function RecoveryKeyBox({ recoveryKey, onSavedToFile }: RecoveryKeyBoxProps) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);

  const copyKey = async () => {
    await navigator.clipboard.writeText(recoveryKey);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const saveKeyToFile = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { writeTextFile } = await import('@tauri-apps/plugin-fs');
      const path = await save({
        defaultPath: 'moony-recovery-key.txt',
        filters: [{ name: 'Text', extensions: ['txt'] }],
      });
      if (!path) return;
      await writeTextFile(
        path,
        recoveryKeyFileContents(recoveryKey, {
          title: t('onboarding.recovery.fileTitle'),
          note: t('onboarding.recovery.fileNote'),
        })
      );
      setSaved(true);
      onSavedToFile?.();
      toast(t('onboarding.recovery.saved'));
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <div>
      <div
        className="select-all rounded-r3 bg-dark px-4 py-[14px] text-center text-[17px] font-650 tracking-[0.06em] text-ink-inverse num"
        data-testid="recovery-key"
      >
        {recoveryKey}
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3">
        <Button type="button" variant="outline" onClick={copyKey}>
          {copied ? <Check /> : <Copy />}
          {copied ? t('recoveryKey.copied') : t('recoveryKey.copy')}
        </Button>
        <Button type="button" variant="outline" onClick={saveKeyToFile}>
          {saved ? <Check /> : <Download />}
          {t('recoveryKey.saveToFile')}
        </Button>
      </div>
    </div>
  );
}
