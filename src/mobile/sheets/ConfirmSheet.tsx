import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';

interface ConfirmSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The question ("Smazat transakci?"). */
  title: string;
  /** What will be gone, named ("Lidl Praha 4 (14. 10., −1 249 Kč) bude trvale smazána."). */
  description: string;
  /** The verb of the solid button ("Smazat transakci"). */
  confirmLabel: string;
  onConfirm: () => void;
  isPending?: boolean;
}

/**
 * Confirm sheet of a destructive action (spec §10.5, README §13): the question, what goes, Zrušit
 * and the solid destructive button — the only place that button appears. The caller closes the
 * sheet when the action has succeeded.
 */
export function ConfirmSheet({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  onConfirm,
  isPending = false,
}: ConfirmSheetProps) {
  const { t: tc } = useTranslation('common');
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-12 rounded-[10px] px-[18px] text-m-row"
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            variant="destructive"
            className="h-12 flex-1 rounded-[10px] text-m-row"
            loading={isPending}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {null}
    </Sheet>
  );
}
