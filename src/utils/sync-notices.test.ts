import { describe, expect, it } from 'vitest';
import type { SyncDevice } from '@shared/schema';
import {
  afterKitJoinDevices,
  afterKitJoinNames,
  newDeviceKind,
  removalNoticeText,
  removalNotDoneText,
  removedNoticeKeys,
} from './sync-notices';

describe('removalNoticeText', () => {
  it('names the removed devices, with the kit sentence when a kit was made too', () => {
    expect(removalNoticeText({ removed: ['iPhone'], kitReplaced: false })).toEqual({
      key: 'notice.removal.removed',
      kitToo: false,
    });
    expect(removalNoticeText({ removed: ['iPhone'], kitReplaced: true })).toEqual({
      key: 'notice.removal.removed',
      kitToo: true,
    });
  });

  it('is the new-kit sentence when nobody was removed', () => {
    expect(removalNoticeText({ removed: [], kitReplaced: true })).toEqual({
      key: 'notice.removal.newKit',
      kitToo: false,
    });
  });
});

describe('removalNotDoneText', () => {
  it('tells the four causes apart', () => {
    const base = { by: 'Mac', alreadyRemoved: false, kitReplaced: false };
    expect(removalNotDoneText(base)).toBe('notice.notDone.changed');
    expect(removalNotDoneText({ ...base, alreadyRemoved: true })).toBe(
      'notice.notDone.alreadyRemoved'
    );
    expect(removalNotDoneText({ ...base, by: null })).toBe('notice.notDone.refused');
    expect(removalNotDoneText({ ...base, kitReplaced: true })).toBe('notice.notDone.kit');
    expect(removalNotDoneText({ ...base, by: null, kitReplaced: true })).toBe('notice.notDone.kit');
  });
});

describe('newDeviceKind', () => {
  it('puts a Kit join first, then a late vouch', () => {
    expect(newDeviceKind({ viaKit: true, lateVouch: true })).toBe('kit');
    expect(newDeviceKind({ viaKit: false, lateVouch: true })).toBe('lateVouch');
    expect(newDeviceKind({ viaKit: false, lateVouch: false })).toBe('added');
  });
});

describe('afterKitJoinDevices', () => {
  const row = (id: string, name: string, over: Partial<SyncDevice> = {}) =>
    ({ id, name, thisDevice: false, status: 'active', ...over }) as SyncDevice;

  it('keeps the named devices that are still on the list, not this one', () => {
    const devices = [
      row('a', 'Mac'),
      row('b', 'iPhone'),
      row('c', 'iPad', { status: 'former' }),
      row('d', 'Mac', { thisDevice: true }),
      row('e', 'Other'),
    ];
    const held = [
      { id: 'a', name: 'Mac' },
      { id: 'b', name: 'iPhone' },
      { id: 'c', name: 'iPad' },
      { id: 'd', name: 'Mac' },
    ];
    expect(afterKitJoinDevices(held, devices).map((d) => d.id)).toEqual(['a', 'b']);
    expect(afterKitJoinDevices(held, undefined)).toEqual([]);
  });

  it('matches by ID: another device with the same name is not the held one', () => {
    const devices = [row('a', 'Mac'), row('x', 'Mac')];
    expect(afterKitJoinDevices([{ id: 'x', name: 'Mac' }], devices).map((d) => d.id)).toEqual([
      'x',
    ]);
  });
});

describe('removedNoticeKeys', () => {
  it('reads a removal and a left record differently, and counts unsent changes', () => {
    expect(removedNoticeKeys({ left: false, unsent: 0 })).toEqual({
      titleKey: 'removed.title',
      bodyKey: 'removed.body',
      showUnsent: false,
    });
    expect(removedNoticeKeys({ left: true, unsent: 3 })).toEqual({
      titleKey: 'removed.titleLeft',
      bodyKey: 'removed.bodyLeft',
      showUnsent: true,
    });
  });
});

describe('afterKitJoinNames', () => {
  const held = [
    { id: 'a', name: 'Mac' },
    { id: 'b', name: 'iPhone' },
  ];
  const row = (id: string, name: string) =>
    ({ id, name, thisDevice: false, status: 'active' }) as SyncDevice;

  it('names only the held devices still on the list once it has loaded', () => {
    expect(afterKitJoinNames(held, [row('a', 'Mac')])).toBe('Mac');
  });

  it('names every held device while the list loads or when none is left on it', () => {
    expect(afterKitJoinNames(held, null)).toBe('Mac, iPhone');
    expect(afterKitJoinNames(held, [])).toBe('Mac, iPhone');
  });
});
