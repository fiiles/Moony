import { describe, expect, it } from 'vitest';
import {
  RECOVERY_KEY_FILE,
  backupFileName,
  dialogPathToFsPath,
  exportsOnThisDevice,
  restoreBackupPlan,
} from './export-file';

describe('backupFileName', () => {
  it('names the archive by its day, the way the desktop does', () => {
    expect(backupFileName('2026-10-17')).toBe('moony-backup-2026-10-17.zip');
  });
});

describe('RECOVERY_KEY_FILE', () => {
  it('is the one file name the capability scope allows', () => {
    expect(RECOVERY_KEY_FILE).toBe('moony-recovery-key.txt');
  });
});

describe('dialogPathToFsPath', () => {
  it('turns a file URL into the path it names, undoing the percent-encoding', () => {
    expect(dialogPathToFsPath('file:///private/var/a%20b/c.zip')).toBe('/private/var/a b/c.zip');
  });

  it('leaves a plain path alone, a percent sign in it included', () => {
    expect(dialogPathToFsPath('/private/var/mobile/c.zip')).toBe('/private/var/mobile/c.zip');
    expect(dialogPathToFsPath('/private/var/100%/c.zip')).toBe('/private/var/100%/c.zip');
  });
});

describe('restoreBackupPlan', () => {
  it('switches biometric unlock off first when it is on: its key opens the profile that is replaced', () => {
    expect(restoreBackupPlan(true)).toEqual([
      'disable-biometrics',
      'clear-asked',
      'restore',
      'reload',
    ]);
  });

  it('skips that step when it is not on', () => {
    expect(restoreBackupPlan(false)).toEqual(['clear-asked', 'restore', 'reload']);
  });
});

describe('exportsOnThisDevice', () => {
  it('is true on iOS only: Android creates an empty document, it never copies the staged file', () => {
    expect(exportsOnThisDevice('ios')).toBe(true);
    for (const other of ['android', 'androideabi', 'darwin', 'windows', 'linux', '']) {
      expect(exportsOnThisDevice(other), other).toBe(false);
    }
  });
});
