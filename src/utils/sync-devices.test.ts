import { describe, expect, it } from 'vitest';
import { devicePlatformKey } from './sync-devices';

describe('devicePlatformKey', () => {
  it('maps known platforms to their label keys and the rest to other', () => {
    for (const p of ['macos', 'windows', 'linux', 'ios', 'android']) {
      expect(devicePlatformKey(p)).toBe(`devices.platform.${p}`);
    }
    expect(devicePlatformKey('freebsd')).toBe('devices.platform.other');
    expect(devicePlatformKey('')).toBe('devices.platform.other');
  });
});
