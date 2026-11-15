import { describe, expect, it } from 'vitest';
import type { SyncPhase, SyncStatus } from '@shared/schema';
import { syncIsOn } from './safe-copy';

const statusIn = (phase: SyncPhase) => ({ phase }) as SyncStatus;

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
