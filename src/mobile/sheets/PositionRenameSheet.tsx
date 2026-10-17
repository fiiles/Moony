import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';

interface PositionRenameSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The name the field starts with each time the sheet opens. */
  name: string;
  /** Saves the trimmed name; the caller closes the sheet once it has succeeded. */
  onSave: (name: string) => void;
  isPending: boolean;
}

/**
 * "Upravit název" (spec §8.8): a half sheet with the one sentence of the desktop's dialog, one text
 * field and Zrušit · Uložit název. The name shows in the list, the detail and the overview; saving
 * an empty name is not possible.
 */
export function PositionRenameSheet({
  open,
  onOpenChange,
  name,
  onSave,
  isPending,
}: PositionRenameSheetProps) {
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const [value, setValue] = useState(name);

  // Every opening starts from the name the position has now, not from the last attempt
  const [openedFor, setOpenedFor] = useState(false);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) setValue(name);
  }

  const trimmed = value.trim();
  const canSave = trimmed !== '' && !isPending;

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={ts('detail.editName.title')}
      description={ts('detail.editName.description')}
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
            type="submit"
            form={`${fieldId}-form`}
            className="h-12 flex-1 rounded-[10px] text-m-row"
            disabled={!canSave}
            loading={isPending}
          >
            {ts('detail.editName.save')}
          </Button>
        </>
      }
    >
      <form
        id={`${fieldId}-form`}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          if (canSave) onSave(trimmed);
        }}
      >
        <label
          htmlFor={fieldId}
          className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2"
        >
          {ts('detail.editName.label')}
        </label>
        <input
          id={fieldId}
          type="text"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          autoComplete="off"
          autoCorrect="off"
          className="h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-body text-ink transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none"
        />
      </form>
    </Sheet>
  );
}
