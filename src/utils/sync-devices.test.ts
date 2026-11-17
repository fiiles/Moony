import { describe, expect, it } from 'vitest';
import type { SyncDevice } from '@shared/schema';
import {
  canHideDevice,
  canKeepDevice,
  canRemoveDevice,
  checkReasonKey,
  devicePlatformKey,
  deviceLabelKey,
  formerDeviceIds,
  formerLineKey,
  groupDevices,
  inactiveDays,
  joinedViaKey,
  newlyJoinedDevice,
  otherDeviceNames,
} from './sync-devices';

describe('devicePlatformKey', () => {
  it('maps known platforms to their label keys and the rest to other', () => {
    for (const p of ['macos', 'windows', 'linux', 'ios', 'android']) {
      expect(devicePlatformKey(p)).toBe(`devices.platform.${p}`);
    }
    expect(devicePlatformKey('freebsd')).toBe('devices.platform.other');
    expect(devicePlatformKey('')).toBe('devices.platform.other');
  });
});

describe('otherDeviceNames', () => {
  const device = (name: string, thisDevice: boolean): SyncDevice => ({
    id: 'a'.repeat(32),
    name,
    platform: 'macos',
    appVersion: '1.0.0',
    joinedAt: 0,
    lastActiveAt: null,
    thisDevice,
    held: false,
    status: 'active',
    checkReason: null,
    formerReason: null,
    joinedVia: 'unknown',
    addedBy: null,
    stateAt: null,
    stateBy: null,
    removalPending: false,
    inactiveDays: null,
  });

  it('lists the other devices by name', () => {
    expect(
      otherDeviceNames([
        device(' iPhone ', false),
        device('Mac', true),
        device('', false),
        device('Windows PC', false),
      ])
    ).toEqual(['iPhone', 'Windows PC']);
    expect(otherDeviceNames(undefined)).toEqual([]);
  });
});

function row(id: string, over: Partial<SyncDevice> = {}): SyncDevice {
  return {
    id: id.repeat(32).slice(0, 32),
    name: `Device ${id}`,
    platform: 'macos',
    appVersion: '1.0.0',
    joinedAt: 0,
    lastActiveAt: null,
    thisDevice: false,
    held: false,
    status: 'active',
    checkReason: null,
    formerReason: null,
    joinedVia: 'paired',
    addedBy: null,
    stateAt: null,
    stateBy: null,
    removalPending: false,
    inactiveDays: null,
    ...over,
  };
}

describe('groupDevices', () => {
  it('splits by status and puts this device first among the active', () => {
    const groups = groupDevices([
      row('a'),
      row('b', { thisDevice: true }),
      row('c', { status: 'check', checkReason: 'copy' }),
      row('d', { status: 'former', formerReason: 'left', stateAt: 5 }),
      row('e'),
    ]);
    expect(groups.active.map((d) => d.name)).toEqual(['Device b', 'Device a', 'Device e']);
    expect(groups.check.map((d) => d.name)).toEqual(['Device c']);
    expect(groups.former.map((d) => d.name)).toEqual(['Device d']);
  });

  it('does not filter by age (the backend already left older former devices out)', () => {
    const groups = groupDevices([
      row('d', { status: 'former', formerReason: 'removed', stateAt: 1 }),
    ]);
    expect(groups.former).toHaveLength(1);
  });

  it('copes with no list', () => {
    expect(groupDevices(undefined)).toEqual({ active: [], check: [], former: [] });
  });
});

describe('what can be done with a row', () => {
  it('removes another active or check device, never this one, a former or a pending one', () => {
    expect(canRemoveDevice(row('a'))).toBe(true);
    expect(canRemoveDevice(row('a', { status: 'check', checkReason: 'unverified' }))).toBe(true);
    expect(canRemoveDevice(row('a', { thisDevice: true }))).toBe(false);
    expect(canRemoveDevice(row('a', { status: 'former', formerReason: 'left' }))).toBe(false);
    expect(canRemoveDevice(row('a', { removalPending: true }))).toBe(false);
  });

  it('takes only a former row off the list', () => {
    expect(canHideDevice(row('a', { status: 'former', formerReason: 'left' }))).toBe(true);
    expect(canHideDevice(row('a', { status: 'former', formerReason: 'removed' }))).toBe(true);
    expect(canHideDevice(row('a'))).toBe(false);
    expect(canHideDevice(row('a', { thisDevice: true }))).toBe(false);
    expect(canHideDevice(row('a', { status: 'check', checkReason: 'copy' }))).toBe(false);
  });

  it('"Odstranit vše" takes every former device, in list order, and nothing else', () => {
    expect(
      formerDeviceIds([
        row('a'),
        row('b', { status: 'former', formerReason: 'removed' }),
        row('c', { status: 'check', checkReason: 'unverified' }),
        row('d', { status: 'former', formerReason: 'left' }),
      ])
    ).toEqual(['b'.repeat(32), 'd'.repeat(32)]);
    expect(formerDeviceIds([row('a')])).toEqual([]);
    expect(formerDeviceIds(undefined)).toEqual([]);
  });

  it('keeps only a late-vouch row', () => {
    const reasons = ['unverified', 'copy', 'issuerRemoved', 'lateVouch'] as const;
    for (const checkReason of reasons) {
      expect(canKeepDevice(row('a', { status: 'check', checkReason }))).toBe(
        checkReason === 'lateVouch'
      );
    }
    expect(canKeepDevice(row('a'))).toBe(false);
  });

  it('adds "(kopie)" to a copy only', () => {
    expect(deviceLabelKey(row('a', { status: 'check', checkReason: 'copy' }))).toBe(
      'devices.copyName'
    );
    expect(deviceLabelKey(row('a', { status: 'check', checkReason: 'unverified' }))).toBeNull();
    expect(deviceLabelKey(row('a'))).toBeNull();
  });

  it('shows inactivity for an active other device only', () => {
    expect(inactiveDays(row('a', { inactiveDays: 41 }))).toBe(41);
    expect(inactiveDays(row('a', { inactiveDays: 41, thisDevice: true }))).toBeNull();
    expect(
      inactiveDays(row('a', { inactiveDays: 41, status: 'check', checkReason: 'copy' }))
    ).toBeNull();
    expect(inactiveDays(row('a'))).toBeNull();
  });
});

describe('checkReasonKey', () => {
  it('names the reason of every check row and none for others', () => {
    const check = (checkReason: SyncDevice['checkReason'], addedBy: string | null = null) =>
      checkReasonKey(row('a', { status: 'check', checkReason, addedBy }));
    expect(check('unverified')).toBe('devices.check.unverified');
    expect(check('copy')).toBe('devices.check.copy');
    expect(check('issuerRemoved')).toBe('devices.check.issuerRemoved');
    expect(check('lateVouch', 'Mac')).toBe('devices.check.lateVouch');
    expect(check('lateVouch')).toBe('devices.check.lateVouchNoName');
    expect(checkReasonKey(row('a'))).toBeNull();
    expect(checkReasonKey(row('a', { status: 'check', checkReason: null }))).toBeNull();
  });
});

describe('joinedViaKey', () => {
  it('maps how a device joined', () => {
    const via = (joinedVia: SyncDevice['joinedVia'], addedBy: string | null = null) =>
      joinedViaKey(row('a', { joinedVia, addedBy }));
    expect(via('created')).toBe('devices.via.created');
    expect(via('paired', 'Mac')).toBe('devices.via.pairedBy');
    expect(via('paired')).toBe('devices.via.paired');
    expect(via('recoveryKit')).toBe('devices.via.recoveryKit');
    expect(via('rejoined')).toBe('devices.via.rejoined');
    expect(via('unknown')).toBeNull();
  });
});

describe('formerLineKey', () => {
  it('says what is known about when and by whom', () => {
    const former = (
      formerReason: SyncDevice['formerReason'],
      stateAt: number | null,
      stateBy: string | null
    ) => formerLineKey(row('a', { status: 'former', formerReason, stateAt, stateBy }));
    expect(former('left', 1, null)).toBe('devices.former.leftOn');
    expect(former('left', null, null)).toBe('devices.former.left');
    expect(former('removed', 1, 'Mac')).toBe('devices.former.removedOnBy');
    expect(former('removed', 1, null)).toBe('devices.former.removedOn');
    expect(former('removed', null, 'Mac')).toBe('devices.former.removedBy');
    expect(former('removed', null, null)).toBe('devices.former.removed');
  });
});

describe('newlyJoinedDevice', () => {
  const row = (id: string, over: Partial<SyncDevice> = {}): SyncDevice => ({
    id,
    name: `Device ${id}`,
    platform: 'macos',
    appVersion: '1.0.0',
    joinedAt: 0,
    lastActiveAt: null,
    thisDevice: false,
    held: false,
    status: 'active',
    checkReason: null,
    formerReason: null,
    joinedVia: 'paired',
    addedBy: null,
    stateAt: null,
    stateBy: null,
    removalPending: false,
    inactiveDays: null,
    ...over,
  });

  it('is the first device that was not on the list before', () => {
    const before = new Set(['a', 'b']);
    expect(newlyJoinedDevice(before, [row('a'), row('b'), row('c')])?.id).toBe('c');
  });

  it('is null while the list is unchanged, unknown, or only grew by this device or a former one', () => {
    const before = new Set(['a']);
    expect(newlyJoinedDevice(before, [row('a')])).toBeNull();
    expect(newlyJoinedDevice(before, undefined)).toBeNull();
    expect(newlyJoinedDevice(before, [row('a'), row('me', { thisDevice: true })])).toBeNull();
    expect(newlyJoinedDevice(before, [row('a'), row('x', { status: 'former' })])).toBeNull();
  });
});
