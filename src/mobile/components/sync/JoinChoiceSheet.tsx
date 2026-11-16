import { useTranslation } from 'react-i18next';
import { ClipboardPaste, QrCode, Type } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { Sheet } from '@/mobile/components/Sheet';

export type PhoneJoinMethod = 'scan' | 'kit' | 'paste';

interface JoinChoiceSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (method: PhoneJoinMethod) => void;
}

/**
 * "Connect this device" on the phone: scan a code from the computer (a Recovery Kit QR is taken
 * too), type a Recovery Kit code or enter a pairing code manually. Picking closes
 * the sheet and hands the way to the page, which opens the flow that does the joining.
 */
export function JoinChoiceSheet({ open, onOpenChange, onPick }: JoinChoiceSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const pick = (method: PhoneJoinMethod) => {
    onOpenChange(false);
    onPick(method);
  };
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('intro.joinTitle')}
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 flex-1 whitespace-normal rounded-[10px] py-2 text-center text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <EntityRows>
        <SettingsRow
          icon={QrCode}
          title={t('phone.choice.scan.title')}
          sub={t('phone.choice.scan.lead')}
          onClick={() => pick('scan')}
        />
        <SettingsRow icon={Type} title={t('phone.typeCode')} onClick={() => pick('kit')} />
        <SettingsRow
          icon={ClipboardPaste}
          title={t('phone.enterCodeTitle')}
          sub={t('phone.enterCodeHint')}
          onClick={() => pick('paste')}
        />
      </EntityRows>
    </Sheet>
  );
}
