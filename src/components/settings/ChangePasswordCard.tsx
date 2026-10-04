import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle, Check, Copy } from 'lucide-react';
import { changePasswordSchema } from '@shared/schema';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

type PasswordData = z.infer<typeof changePasswordSchema>;

// Pending change password data (stored between prepare and confirm phases)
interface PendingPasswordChange {
  currentPassword: string;
  newPassword: string;
  recoveryKey: string;
}

function ChangePasswordForm() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const [step, setStep] = useState<'form' | 'confirm'>('form');
  const [pendingData, setPendingData] = useState<PendingPasswordChange | null>(null);
  const [copied, setCopied] = useState(false);

  const form = useForm<PasswordData>({
    resolver: zodResolver(changePasswordSchema),
    defaultValues: {
      currentPassword: '',
      newPassword: '',
      confirmPassword: '',
    },
  });

  // Phase 1: Prepare - verify current password and get new recovery key
  const prepareMutation = useMutation({
    mutationFn: async (data: PasswordData) => {
      const result = await authApi.prepareChangePassword({
        currentPassword: data.currentPassword,
      });
      return { ...data, recoveryKey: result.recoveryKey };
    },
    onSuccess: (result) => {
      setPendingData({
        currentPassword: result.currentPassword,
        newPassword: result.newPassword,
        recoveryKey: result.recoveryKey,
      });
      setStep('confirm');
    },
    onError: (error: Error) => {
      toast.error(t('toast.verificationFailed'), { description: translateApiError(error, tc) });
    },
  });

  // Phase 2: Confirm - actually change the password
  const confirmMutation = useMutation({
    mutationFn: async () => {
      if (!pendingData) throw new Error('No pending password change');
      await authApi.confirmChangePassword({
        currentPassword: pendingData.currentPassword,
        newPassword: pendingData.newPassword,
        recoveryKey: pendingData.recoveryKey,
      });
    },
    onSuccess: () => {
      toast(t('toast.passwordChanged'));
      setStep('form');
      setPendingData(null);
      form.reset();
    },
    onError: (error: Error) => {
      toast.error(t('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  const handleCancel = () => {
    setStep('form');
    setPendingData(null);
    setCopied(false);
  };

  const copyRecoveryKey = async () => {
    if (pendingData?.recoveryKey) {
      await navigator.clipboard.writeText(pendingData.recoveryKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  return (
    <>
      <Form {...form}>
        <form onSubmit={form.handleSubmit((data) => prepareMutation.mutate(data))}>
          <FormField
            control={form.control}
            name="currentPassword"
            render={({ field }) => (
              <FormItem>
                <FormLabel>{t('password.current')}</FormLabel>
                <FormControl>
                  <PasswordInput autoComplete="current-password" {...field} />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <div className="mt-3 grid grid-cols-2 gap-3">
            <FormField
              control={form.control}
              name="newPassword"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('password.new')}</FormLabel>
                  <FormControl>
                    <PasswordInput autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirmPassword"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('password.confirm')}</FormLabel>
                  <FormControl>
                    <PasswordInput autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
          </div>
          <div className="mt-4">
            <Button type="submit" size="sm" loading={prepareMutation.isPending}>
              {t('password.update')}
            </Button>
          </div>
        </form>
      </Form>

      {/* Recovery key confirmation */}
      <Dialog open={step === 'confirm'} onOpenChange={(open) => !open && handleCancel()}>
        <DialogContent className="max-w-[460px]">
          <DialogHeader>
            <DialogTitle>{t('password.recoveryKeyTitle')}</DialogTitle>
            <DialogDescription>{t('password.recoveryKeyDescription')}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Alert variant="destructive">
              <AlertTriangle />
              <AlertDescription>{t('password.oldKeyInvalid')}</AlertDescription>
            </Alert>
            <div className="break-all rounded-r3 bg-well px-4 py-[14px] text-center font-mono text-[15px] tracking-wider text-ink">
              {pendingData?.recoveryKey}
            </div>
            <Button onClick={copyRecoveryKey} className="w-full" variant="outline">
              {copied ? <Check /> : <Copy />}
              {copied ? t('auth:recoveryKey.copied') : t('auth:recoveryKey.copy')}
            </Button>
            <p className="text-center text-micro font-500 text-ink-4">
              {t('password.confirmHint')}
            </p>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={handleCancel} disabled={confirmMutation.isPending}>
              {tc('buttons.cancel')}
            </Button>
            <Button onClick={() => confirmMutation.mutate()} loading={confirmMutation.isPending}>
              {t('auth:recoveryKey.savedElsewhere')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** Settings → Security → Změna hesla: the two-phase change with the new recovery key. */
export function ChangePasswordCard() {
  const { t } = useTranslation('settings');

  return (
    <SettingsCard title={t('password.title')} description={t('password.description')}>
      <ChangePasswordForm />
    </SettingsCard>
  );
}
