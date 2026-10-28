import { useQuery } from '@tanstack/react-query';
import { biometricApi } from '@/lib/tauri-api';
import { BIOMETRIC_STATUS_KEY, desktopBiometryLabel } from '@/lib/biometrics';

/**
 * Biometric unlock on this desktop (Touch ID, Windows Hello): whether it can be used and is
 * switched on, and which name the copy uses (`label`). Readable while the database is still
 * locked; the lock's `queryClient.clear()` drops it, so every lock screen asks again.
 */
export function useBiometricStatus({ enabled = true }: { enabled?: boolean } = {}) {
  const query = useQuery({
    queryKey: BIOMETRIC_STATUS_KEY,
    queryFn: biometricApi.status,
    enabled,
  });
  const label = desktopBiometryLabel(
    query.data?.biometryType ?? 'none',
    import.meta.env.TAURI_ENV_PLATFORM ?? ''
  );
  return { query, status: query.data, label };
}
