import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, TriangleAlert } from 'lucide-react';
import type { KitCheck } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { kitCodeProblem } from '@/utils/sync-setup';
import { kitCheckIsGood, kitCheckKey } from '@/utils/sync-kit';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/**
 * "Ověřit sadu": reads a typed code (or the text of a scanned QR) and says whether it is this
 * sync's current recovery kit. No password and nothing leaves the device (`sync_check_recovery_kit`
 * wipes the code before it answers). The code is a mutation variable (`gcTime: 0`) and a field
 * value: both go when the dialog closes. Mounted only while open.
 */
export function CheckKitDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { checkRecoveryKit } = useDeviceSyncMutations();
  const [code, setCode] = useState('');
  const [answer, setAnswer] = useState<KitCheck | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The typed code leaves the mutation's state with the dialog.
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

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[520px]">
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t('check.title')}</DialogTitle>
            <DialogDescription>{t('check.description')}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="check-kit-code">{t('check.label')}</Label>
            <Input
              id="check-kit-code"
              value={code}
              onChange={(event) => {
                setCode(event.target.value);
                setAnswer(null);
                setError(null);
              }}
              onKeyDown={submitOnEnter}
              autoComplete="off"
              autoCapitalize="characters"
              autoCorrect="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={error ? true : undefined}
            />
            {error && (
              <p className="m-0 text-micro font-500 text-loss" role="alert">
                {error}
              </p>
            )}
          </div>
          {answer && (
            <Alert variant={kitCheckIsGood(answer.result) ? 'default' : 'destructive'}>
              {kitCheckIsGood(answer.result) ? <Check /> : <TriangleAlert />}
              <AlertDescription data-testid="kit-check-result">{message(answer)}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              {tc('buttons.close')}
            </Button>
            <Button type="submit" disabled={trimmed === ''} loading={checkRecoveryKit.isPending}>
              {t('check.submit')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
