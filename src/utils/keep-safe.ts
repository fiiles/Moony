export type KeepSafeItem = 'password' | 'recoveryKey' | 'kit';

/**
 * What "Co si uschovat" lists: nothing without the sync flag (the card is not shown, so Security
 * reads as today); the device password and recovery key; the Sync Recovery Kit while sync is on.
 * The connection code is never listed (spec §7.5: the server's admin shows it again).
 */
export function keepSafeItems(featureOn: boolean, syncOn: boolean): KeepSafeItem[] {
  if (!featureOn) return [];
  return syncOn ? ['password', 'recoveryKey', 'kit'] : ['password', 'recoveryKey'];
}
