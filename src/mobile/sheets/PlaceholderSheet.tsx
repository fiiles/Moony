import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';

interface PlaceholderSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  /** Says what is coming and when ("Import výpisů přijde ve fázi 2b."). */
  text: string;
}

/** A sheet that is not built yet: its title, one sentence and Zavřít. */
export function PlaceholderSheet({ open, onOpenChange, title, text }: PlaceholderSheetProps) {
  const { t: tc } = useTranslation('common');
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      footer={
        <Button
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      <p className="mx-1 my-0 text-m-body text-ink-3">{text}</p>
    </Sheet>
  );
}
