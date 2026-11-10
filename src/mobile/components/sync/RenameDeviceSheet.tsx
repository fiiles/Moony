import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncDevice } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { Sheet } from '@/mobile/components/Sheet';
import { field48 } from '@/mobile/pages/first-run/styles';

interface RenameDeviceSheetProps {
  /** The device to rename (this one); `null` while the sheet is closed. */
  device: SyncDevice | null;
  onOpenChange: (open: boolean) => void;
}

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/** "Rename this device": one name field, saved through `renameDevice`. */
export function RenameDeviceSheet({ device, onOpenChange }: RenameDeviceSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { renameDevice } = useDeviceSyncMutations();
  const fieldId = useId();
  const [name, setName] = useState(device?.name ?? '');
  // Each time the sheet opens for a device its name is the field's start
  const [seed, setSeed] = useState(device);
  if (device !== seed) {
    setSeed(device);
    if (device) setName(device.name);
  }
  const trimmed = name.trim();
  const pending = renameDevice.isPending;

  const change = (next: boolean) => {
    if (!next && pending) return;
    onOpenChange(next);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (trimmed === '' || pending) return;
    try {
      await renameDevice.mutateAsync(trimmed);
      onOpenChange(false);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      open={device !== null}
      onOpenChange={change}
      title={t('devices.renameTitle')}
      dirty={device !== null && trimmed !== device.name}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            disabled={pending}
            onClick={() => change(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={formId}
            className={`${BUTTON} flex-1`}
            loading={pending}
            disabled={trimmed === ''}
          >
            {tc('buttons.save')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate className="space-y-1.5">
        <Label htmlFor={fieldId}>{t('setup.device.name')}</Label>
        <Input
          id={fieldId}
          value={name}
          maxLength={60}
          onChange={(event) => setName(event.target.value)}
          autoComplete="off"
          enterKeyHint="done"
          readOnly={pending}
          className={field48}
        />
      </form>
    </Sheet>
  );
}
