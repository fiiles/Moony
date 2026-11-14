import { describe, expect, it } from 'vitest';
import type { SyncServerCheck } from '@shared/schema';
import { serverContinue, setupSteps } from './sync-setup';

const check = (insecure: boolean): SyncServerCheck => ({
  address: 'http://192.168.1.2:8787',
  host: '192.168.1.2:8787',
  version: '1.0.0',
  insecure,
});

describe('setupSteps', () => {
  it('has three steps on an own server: no account step, no payment', () => {
    expect(setupSteps('own')).toEqual(['server', 'kit', 'device']);
  });

  it('keeps the account and payment steps for the cloud', () => {
    expect(setupSteps('cloud')).toEqual(['server', 'account', 'pay', 'kit', 'device']);
  });
});

describe('serverContinue', () => {
  it('waits for a server that answered', () => {
    expect(serverContinue(undefined, true)).toBe(false);
    expect(serverContinue(undefined, false)).toBe(false);
  });

  it('lets a secure server through whatever the checkbox says', () => {
    expect(serverContinue(check(false), false)).toBe(true);
  });

  it('holds a plain http server until it is acknowledged', () => {
    expect(serverContinue(check(true), false)).toBe(false);
    expect(serverContinue(check(true), true)).toBe(true);
  });
});
