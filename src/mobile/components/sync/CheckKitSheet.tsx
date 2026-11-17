import { useEffect, useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, TriangleAlert } from 'lucide-react';
import type { KitCheck } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { kitCheckIsGood, kitCheckKey } from '@/utils/sync-kit';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { Sheet } from '@/mobile/components/Sheet';
import { kitCodeProblem } from '@/mobile/lib/sync-scan';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * "Ověřit sadu" on the phone: reads a typed or pasted code (or the text of the kit's QR) and says
 * whether it is this sync's current recovery kit. No password and nothing leaves the device
 * (`sync_check_recovery_kit` wipes the code before it answers). The code is a mutation variable
 * (`gcTime: 0`) and a field value: both go when the sheet closes. Mounted only while open.
 */
export function CheckKitSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const fieldId = useId();
  const { checkRecoveryKit } = useDeviceSyncMutations();
  const [code, setCode] = useState('');
  const [answer, setAnswer] = useState<KitCheck | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The typed code leaves the mutation's state with the sheet.
  const reset = checkRecoveryKit.reset;
  useEffect(() => reset, [reset]);

  const trimmed = code.trim();
  const problem = trimmed === '' ? null : kitCodeProblem(trimmed);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (trimmed === '' || checkRecoveryKit.isPending) return;
    setError(null);
    setAnswer(null);
    if (problem) {
      setError(t(problem));
      return;
    }
    try {
      setAnswer(await checkRecoveryKit.mutateAsync(trimmed));
    } catch (failure) {
      setError(translateApiError(failure as Error, tc));
    } finally {
      // The answer does not need the code, and the field does not keep it.
      checkRecoveryKit.reset();
      setCode('');
    }
  };

  const message = (result: KitCheck) =>
    t(kitCheckKey(result.result, result.currentCreatedAt), {
      date:
        result.currentCreatedAt === null
          ? ''
          : date(new Date(result.currentCreatedAt * 1000), { dateStyle: 'long' }),
    });

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('check.title')}
      description={t('check.description')}
      footer={
        <>
          <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={onClose}>
            {tc('buttons.close')}
          </Button>
          <Button
            type="submit"
            form={formId}
            className={`${BUTTON} flex-1`}
            disabled={trimmed === ''}
            loading={checkRecoveryKit.isPending}
          >
            {t('check.submit')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={(event) => void submit(event)} noValidate className="space-y-3.5">
        <div className="space-y-1.5">
          <Label htmlFor={fieldId}>{t('check.label')}</Label>
          <Input
            id={fieldId}
            value={code}
            onChange={(event) => {
              setCode(event.target.value);
              setAnswer(null);
              setError(null);
            }}
            autoComplete="off"
            autoCapitalize="characters"
            autoCorrect="off"
            spellCheck={false}
            enterKeyHint="go"
            className={`${field48} font-mono`}
            aria-invalid={error ? true : undefined}
          />
          {error && (
            <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
              {error}
            </p>
          )}
        </div>
        {answer && (
          <InlineAlert icon={kitCheckIsGood(answer.result) ? Check : TriangleAlert}>
            <span data-testid="kit-check-result">{message(answer)}</span>
          </InlineAlert>
        )}
      </form>
    </Sheet>
  );
}
