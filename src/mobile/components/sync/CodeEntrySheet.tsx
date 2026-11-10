import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Sheet } from '@/mobile/components/Sheet';
import { pairingCodeProblem } from '@/mobile/lib/sync-scan';
import { fieldMessage } from '@/mobile/pages/first-run/styles';

interface CodeEntrySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The pasted pairing code, in place of a scanned one. */
  onSubmit: (code: string) => void;
}

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * "Paste a pairing code — developer tool": the simulator has no camera, so with the developer tools
 * on (`status.devTools`, review PF2) a pairing code copied from the other device can be pasted
 * instead of scanned. It takes a pairing code only; release builds never show it.
 */
export function CodeEntrySheet({ open, onOpenChange, onSubmit }: CodeEntrySheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const [code, setCode] = useState('');
  const [problem, setProblem] = useState<string | null>(null);

  if (!open && (code !== '' || problem !== null)) {
    setCode('');
    setProblem(null);
  }

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (code.trim() === '') return;
    const found = pairingCodeProblem(code);
    if (found) {
      setProblem(found);
      return;
    }
    onSubmit(code.trim());
  };

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('phone.devPasteTitle')}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={formId}
            className={`${BUTTON} flex-1`}
            disabled={code.trim() === ''}
          >
            {tc('buttons.continue')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate className="space-y-1.5">
        <Label htmlFor={fieldId}>{t('phone.devPasteLabel')}</Label>
        <textarea
          id={fieldId}
          value={code}
          rows={3}
          onChange={(event) => {
            setCode(event.target.value);
            setProblem(null);
          }}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          aria-invalid={problem ? true : undefined}
          className="m-0 block w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 font-mono text-m-field text-ink transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss"
        />
        {problem && (
          <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
            {t(problem)}
          </p>
        )}
      </form>
    </Sheet>
  );
}
