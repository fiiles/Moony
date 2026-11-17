import { describe, expect, it } from 'vitest';
import deviceSync from '@/i18n/locales/en/deviceSync.json';
import mobile from '@/i18n/locales/en/mobile.json';
import { ANDROID_COPY_KEYS, phoneCopyKey } from './platform-copy';

function lookup(tree: unknown, key: string): unknown {
  return key
    .split('.')
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], tree);
}

describe('phoneCopyKey', () => {
  it('keeps the key on iOS and desktop platforms', () => {
    for (const platform of ['ios', 'darwin', 'windows', 'linux', '']) {
      expect(phoneCopyKey('backup.what', platform)).toBe('backup.what');
    }
  });

  it('swaps listed keys on Android', () => {
    expect(phoneCopyKey('backup.what', 'android')).toBe('backup.whatAndroid');
    expect(phoneCopyKey('firstRun.points.localOnly.textSync', 'android')).toBe(
      'firstRun.points.localOnly.textSyncAndroid'
    );
    expect(phoneCopyKey('replace.saved.saveCopy', 'android')).toBe('replace.saved.saveCopyAndroid');
  });

  it('leaves unlisted keys alone on Android', () => {
    expect(phoneCopyKey('backup.unavailableHere', 'android')).toBe('backup.unavailableHere');
    expect(phoneCopyKey('firstRun.privacyLead', 'android')).toBe('firstRun.privacyLead');
  });

  it('has an English Android variant for every listed key', () => {
    for (const key of ANDROID_COPY_KEYS) {
      const tree = key.startsWith('replace.') ? deviceSync : mobile;
      expect(typeof lookup(tree, key), key).toBe('string');
      expect(typeof lookup(tree, `${key}Android`), `${key}Android`).toBe('string');
    }
  });
});
