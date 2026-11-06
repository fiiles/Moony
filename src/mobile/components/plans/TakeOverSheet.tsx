import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';

interface TakeOverSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** How many plans the take-over creates. */
  count: number;
  fromYear: number;
  isPending: boolean;
  onConfirm: () => void;
}

/**
 * "Převzít n plánů z roku N−1?" (spec M7): asked once, then every open plan of the year before
 * that has no same-named plan is created. A plain confirm, not the destructive one: nothing is
 * lost. The caller closes the sheet when the plans were created.
 */
export function TakeOverSheet({
  open,
  onOpenChange,
  count,
  fromYear,
  isPending,
  onConfirm,
}: TakeOverSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('plans.takeOver.title', { count, year: fromYear })}
      description={t('plans.takeOver.description')}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal rounded-[10px] px-[18px] py-2 text-center text-m-row"
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            className="h-auto min-h-12 flex-1 whitespace-normal rounded-[10px] py-2 text-center text-m-row"
            loading={isPending}
            onClick={onConfirm}
          >
            {tp('takeOver.submit', { count })}
          </Button>
        </>
      }
    >
      {null}
    </Sheet>
  );
}
