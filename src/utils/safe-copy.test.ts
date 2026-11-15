import { describe, expect, it } from 'vitest';
import type { SyncDevice, SyncPhase, SyncStatus } from '@shared/schema';
import { protectionLine, syncIsOn } from './safe-copy';

const now = 1_800_000_000;

/** A complete status: every field of the type is present, so a test changes exactly the field it names. */
const base: SyncStatus = {
  phase: 'idle',
  problem: null,
  lastSuccessAt: now - 240,
  pendingChanges: 0,
  heldDevices: 0,
  refusedRows: 0,
  paidUntil: null,
  deletesAt: null,
  clockAheadUntil: null,
  devTools: false,
  ownServer: false,
  serverAddress: null,
  serverInsecure: false,
  enabledAt: null,
  attachments: {
    waitingUpload: 0,
    waitingUploadBytes: 0,
    notHere: 0,
    notOnServer: 0,
    failed: 0,
    notOnDevice: 0,
    problem: null,
  },
  lastCompleteAt: now - 240,
  protectsData: true,
};

const statusIn = (phase: SyncPhase): SyncStatus => ({ ...base, phase });

const device = (id: string): SyncDevice => ({
  id,
  name: id,
  platform: 'macos',
  appVersion: '1.0.0',
  joinedAt: now - 1_000,
  lastActiveAt: null,
  thisDevice: false,
  held: false,
});
const three = [device('a'), device('b'), device('c')];

const PHASES: Record<SyncPhase, boolean> = {
  off: false,
  settingUp: false,
  idle: true,
  syncing: true,
  waiting: true,
  paused: true,
  error: true,
};

describe('syncIsOn', () => {
  it('is on only with the flag and a phase past setup', () => {
    for (const [phase, on] of Object.entries(PHASES) as [SyncPhase, boolean][]) {
      expect(syncIsOn(true, statusIn(phase)), `flag on, ${phase}`).toBe(on);
      expect(syncIsOn(false, statusIn(phase)), `flag off, ${phase}`).toBe(false);
    }
  });

  it('is off while the status is not loaded', () => {
    expect(syncIsOn(true, undefined)).toBe(false);
    expect(syncIsOn(false, undefined)).toBe(false);
  });
});

describe('protectionLine', () => {
  it('shows with the flag, the verdict, a complete sync and the devices in', () => {
    expect(protectionLine(true, base, three, now)).not.toBeNull();
  });

  it('is null without the flag, without the verdict or before the devices are in', () => {
    expect(protectionLine(false, base, three, now)).toBeNull();
    expect(protectionLine(true, { ...base, protectsData: false }, three, now)).toBeNull();
    expect(protectionLine(true, { ...base, lastCompleteAt: null }, three, now)).toBeNull();
    expect(protectionLine(true, base, undefined, now)).toBeNull();
    expect(protectionLine(true, undefined, three, now)).toBeNull();
  });

  it('counts the devices and gives the age', () => {
    expect(protectionLine(true, base, three, now)).toEqual({
      devices: 3,
      age: { key: 'minutes', count: 4 },
    });
    expect(protectionLine(true, base, [], now)?.devices).toBe(1);
  });

  it('shows a complete sync in the future as just now', () => {
    const future = { ...base, lastCompleteAt: now + 3_600 };
    expect(protectionLine(true, future, three, now)?.age).toEqual({ key: 'justNow', count: 0 });
  });
});
