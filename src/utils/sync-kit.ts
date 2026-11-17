import type { KitCheckResult, SyncKitInfo, SyncKitStorage, SyncStatus } from '@shared/schema';

/**
 * Where the recovery kit stands in the Kit row (spec §6.3): `none` (sync has no usable kit: "Vytvořit
 * sadu"), `unsaved` (the person did nothing with it and it cannot be shown again: "Vytvořit novou
 * sadu"), `kept` (copied, printed or saved: check it or make a new one).
 */
export type KitRowState = 'none' | 'unsaved' | 'kept';

export function kitRowState(kit: SyncKitInfo | null | undefined): KitRowState {
  if (!kit) return 'none';
  return kit.storage === null ? 'unsaved' : 'kept';
}

/** The `deviceSync` key naming where the kit is kept ("vytištěná na papír", …). */
export function kitStorageKey(storage: SyncKitStorage): string {
  return `kit.storage.${storage}`;
}

/**
 * The `deviceSync` key of the answer to "Ověřit sadu". A replaced kit names the valid one's date
 * when this device knows it.
 */
export function kitCheckKey(result: KitCheckResult, currentCreatedAt: number | null): string {
  switch (result) {
    case 'valid':
      return 'check.valid';
    case 'replaced':
      return currentCreatedAt === null ? 'check.replacedNoDate' : 'check.replaced';
    case 'otherSync':
      return 'check.otherSync';
    case 'unknown':
      return 'check.unknown';
  }
}

/** Whether the answer says the kit works (so the dialog does not warn). */
export function kitCheckIsGood(result: KitCheckResult): boolean {
  return result === 'valid';
}

/**
 * What follows `finishSetup`: the "Synchronizace je zapnutá" screen, or, when the sync server
 * already held another kit and this device has none it can use (`recoveryKit` null, R-62), the
 * dialog that makes one right away.
 */
export function afterSetup(status: Pick<SyncStatus, 'recoveryKit'>): 'done' | 'newKit' {
  return status.recoveryKit === null ? 'newKit' : 'done';
}
