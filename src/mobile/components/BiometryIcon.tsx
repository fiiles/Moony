import { FingerprintPattern, ScanFace, ShieldCheck } from 'lucide-react';
import type { BiometryType } from '@shared/schema';
import { biometryKind } from '@/lib/biometrics';

/** The biometry's icon (decorative): the face, the fingerprint, or a shield for anything else. */
export function BiometryIcon({ type, className }: { type: BiometryType; className?: string }) {
  const kind = biometryKind(type);
  const Icon = kind === 'face' ? ScanFace : kind === 'touch' ? FingerprintPattern : ShieldCheck;
  return <Icon className={className} strokeWidth={1.5} aria-hidden />;
}
