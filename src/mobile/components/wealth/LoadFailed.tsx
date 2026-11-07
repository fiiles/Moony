import { LoadError } from '@/mobile/components/LoadError';

/**
 * A class page whose list could not be read (Akcie, Krypto, Dluhopisy, Ostatní aktiva): the class's
 * own sentence ("Akcie se nepodařilo načíst.") with "Zkusit znovu" in place of the rows (`LoadError`).
 */
export function LoadFailed({ children }: { children: string }) {
  return <LoadError message={children} />;
}
