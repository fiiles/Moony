import { describe, expect, it } from 'vitest';
import type { SyncServerCheck } from '@shared/schema';
import { acknowledgedAfterEdit, isInsecureAddress, serverContinue, setupSteps } from './sync-setup';

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

describe('isInsecureAddress', () => {
  it('flags plain http to anything but this machine', () => {
    expect(isInsecureAddress('http://192.168.1.2:8787')).toBe(true);
    expect(isInsecureAddress('http://nas.local')).toBe(true);
  });

  it('lets https and local http through', () => {
    expect(isInsecureAddress('https://sync.example.org')).toBe(false);
    expect(isInsecureAddress('http://localhost:8787')).toBe(false);
    expect(isInsecureAddress('http://127.0.0.1:8787')).toBe(false);
    expect(isInsecureAddress('not an address')).toBe(false);
  });
});

describe('acknowledgedAfterEdit', () => {
  it('clears the acknowledgement when the input changes', () => {
    expect(acknowledgedAfterEdit('a', 'ab', true)).toBe(false);
  });

  it('keeps it while the input is the same, and never invents one', () => {
    expect(acknowledgedAfterEdit('a', 'a', true)).toBe(true);
    expect(acknowledgedAfterEdit('a', 'a', false)).toBe(false);
  });
});
