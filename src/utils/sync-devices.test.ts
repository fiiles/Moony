import { describe, expect, it } from 'vitest';
import { QUIET_AFTER_DAYS, devicePlatformKey, quietDays } from './sync-devices';

describe('devicePlatformKey', () => {
  it('maps known platforms to their label keys and the rest to other', () => {
    for (const p of ['macos', 'windows', 'linux', 'ios', 'android']) {
      expect(devicePlatformKey(p)).toBe(`devices.platform.${p}`);
    }
    expect(devicePlatformKey('freebsd')).toBe('devices.platform.other');
    expect(devicePlatformKey('')).toBe('devices.platform.other');
  });
});

describe('quietDays', () => {
  const DAY = 86_400;
  const now = 1_800_000_000;

  it('is null when the last activity is unknown', () => {
    expect(quietDays(null, now)).toBeNull();
  });

  it('is null up to and including the threshold', () => {
    expect(quietDays(now - 29 * DAY, now)).toBeNull();
    expect(quietDays(now - QUIET_AFTER_DAYS * DAY, now)).toBeNull();
  });

  it('counts whole days beyond the threshold', () => {
    expect(quietDays(now - 31 * DAY, now)).toBe(31);
    expect(quietDays(now - 31 * DAY - 3600, now)).toBe(31);
  });

  it('is null for a last activity in the future', () => {
    expect(quietDays(now + DAY, now)).toBeNull();
  });
});
