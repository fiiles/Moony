import { useTranslation } from 'react-i18next';
import { FingerprintPattern, KeyRound, RefreshCw, ShieldCheck } from 'lucide-react';
import type { DesktopBiometryLabel } from '@/lib/biometrics';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * The desktop method's icon (decorative): the fingerprint for Touch ID, a key for Windows Hello
 * (face, finger or PIN, whichever the person set up), a shield for anything else.
 */
export function DesktopBiometryIcon({
  label,
  className,
}: {
  label: DesktopBiometryLabel;
  className?: string;
}) {
  const Icon =
    label === 'touchId' ? FingerprintPattern : label === 'windowsHello' ? KeyRound : ShieldCheck;
  return <Icon className={className} aria-hidden />;
}

/**
 * "Odemknout otiskem" / "Odemknout přes Windows Hello" on the lock screen, above the password
 * form: opens the system prompt again at any time (the automatic one fires once per lock).
 */
export function BiometricUnlockButton({
  label,
  pending,
  disabled,
  onClick,
  className,
}: {
  label: DesktopBiometryLabel;
  pending: boolean;
  disabled?: boolean;
  onClick: () => void;
  className?: string;
}) {
  const { t } = useTranslation('auth');
  return (
    <Button
      type="button"
      className={cn('h-10 w-full text-body [&_svg]:size-4', className)}
      onClick={onClick}
      disabled={disabled}
      loading={pending}
    >
      {/* While the system prompt is up the leading icon spins; a spinning fingerprint would not read. */}
      {pending ? <RefreshCw aria-hidden /> : <DesktopBiometryIcon label={label} />}
      {t(`biometric.unlock.${label}`)}
    </Button>
  );
}
