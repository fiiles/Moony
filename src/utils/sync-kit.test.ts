import { describe, expect, it } from 'vitest';
import type { SyncKitInfo } from '@shared/schema';
import { afterSetup, kitCheckIsGood, kitCheckKey, kitRowState, kitStorageKey } from './sync-kit';

const kit = (storage: SyncKitInfo['storage']): SyncKitInfo => ({
  createdAt: 1,
  createdBy: 'Mac',
  storage,
  lastReadAt: null,
});

describe('kitRowState', () => {
  it('tells no kit, an unsaved kit and a kept kit apart', () => {
    expect(kitRowState(null)).toBe('none');
    expect(kitRowState(undefined)).toBe('none');
    expect(kitRowState(kit(null))).toBe('unsaved');
    for (const storage of ['paper', 'copied', 'file', 'cloud'] as const) {
      expect(kitRowState(kit(storage))).toBe('kept');
    }
  });
});

describe('kitStorageKey', () => {
  it('names the storage', () => {
    expect(kitStorageKey('cloud')).toBe('kit.storage.cloud');
  });
});

describe('kitCheckKey', () => {
  it('maps each answer to its text', () => {
    expect(kitCheckKey('valid', null)).toBe('check.valid');
    expect(kitCheckKey('otherSync', null)).toBe('check.otherSync');
    expect(kitCheckKey('unknown', null)).toBe('check.unknown');
  });

  it('names the valid kit only when its date is known', () => {
    expect(kitCheckKey('replaced', 5)).toBe('check.replaced');
    expect(kitCheckKey('replaced', null)).toBe('check.replacedNoDate');
  });

  it('is good only for a valid kit', () => {
    expect(kitCheckIsGood('valid')).toBe(true);
    expect(kitCheckIsGood('replaced')).toBe(false);
    expect(kitCheckIsGood('otherSync')).toBe(false);
    expect(kitCheckIsGood('unknown')).toBe(false);
  });
});

describe('afterSetup', () => {
  it('makes a kit at once when the server kit beat this setup', () => {
    expect(afterSetup({ recoveryKit: null })).toBe('newKit');
    expect(afterSetup({ recoveryKit: kit(null) })).toBe('done');
    expect(afterSetup({ recoveryKit: kit('paper') })).toBe('done');
  });
});
