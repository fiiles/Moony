import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { UPKEEP_DAYS } from '@/mobile/lib/notification-prefs';

interface DayPickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The chosen day of the month. */
  value: number;
  onPick: (day: number) => void;
}

/**
 * Opens the list at the chosen row. Only the sheet's body is scrolled: `scrollIntoView` would also
 * move the boxes above it, and the sheet is still rising when this runs.
 */
function showChosen(list: HTMLElement | null) {
  const row = list?.querySelector<HTMLElement>('[aria-checked="true"]');
  let box = row?.parentElement ?? null;
  while (box && getComputedStyle(box).overflowY !== 'auto') box = box.parentElement;
  if (!row || !box) return;
  const offset = row.getBoundingClientRect().top - box.getBoundingClientRect().top;
  box.scrollTop += offset - (box.clientHeight - row.offsetHeight) / 2;
}

/**
 * "Den v měsíci" of the monthly upkeep reminder (spec §8.9): the days 1 to 28, so every month has
 * the day, the current one checked and in view. A tap picks the day and closes the sheet.
 */
export function DayPickerSheet({ open, onOpenChange, value, onPick }: DayPickerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('notifications.upkeep.label')}
      full
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <div ref={showChosen}>
        <PickGroup title={t('notifications.upkeep.day')}>
          {UPKEEP_DAYS.map((day) => (
            <PickRow
              key={day}
              label={t('notifications.upkeep.dayLabel', { day: fmt.number(day) })}
              checked={day === value}
              onSelect={() => {
                onPick(day);
                onOpenChange(false);
              }}
            />
          ))}
        </PickGroup>
      </div>
    </Sheet>
  );
}
