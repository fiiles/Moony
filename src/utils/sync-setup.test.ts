import { describe, expect, it } from 'vitest';
import type { SyncServerCheck } from '@shared/schema';
import {
  accountQueryEnabled,
  acknowledgedAfterEdit,
  hostOf,
  kitServerNeed,
  needsJoinAcknowledgement,
  serverContinue,
  setupSteps,
} from './sync-setup';

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

describe('accountQueryEnabled', () => {
  it('keeps a sync server unasked until the server step is over', () => {
    expect(accountQueryEnabled('server', 'own')).toBe(false);
    expect(accountQueryEnabled('kit', 'own')).toBe(true);
    expect(accountQueryEnabled('device', 'own')).toBe(true);
  });

  it('reads the cloud account on every step (the pay step polls it)', () => {
    for (const step of ['server', 'account', 'pay', 'kit', 'device'] as const) {
      expect(accountQueryEnabled(step, 'cloud')).toBe(true);
    }
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

describe('needsJoinAcknowledgement', () => {
  const view = (joinServer: string | null, joinServerInsecure: boolean) => ({
    joinServer,
    joinServerInsecure,
  });

  it('asks for a plain http server nobody acknowledged yet', () => {
    expect(needsJoinAcknowledgement(view('192.168.1.2:8787', true), null)).toBe(true);
    expect(needsJoinAcknowledgement(view('192.168.1.2:8787', true), '192.168.1.9:8787')).toBe(true);
  });

  it('does not ask again for the host the same dialog already acknowledged', () => {
    expect(needsJoinAcknowledgement(view('192.168.1.2:8787', true), '192.168.1.2:8787')).toBe(
      false
    );
  });

  it('asks for nothing without credentials or on a secure server', () => {
    expect(needsJoinAcknowledgement(view(null, false), null)).toBe(false);
    expect(needsJoinAcknowledgement(view('sync.example.com', false), null)).toBe(false);
  });
});

describe('kitServerNeed', () => {
  it('asks for the server of a typed recovery code', () => {
    expect(kitServerNeed('recoveryCode', null)).toBe('typed');
  });

  it('checks the server a Kit QR names, unless it is the Moony cloud', () => {
    expect(kitServerNeed('recover', 'http://192.168.1.2:8787')).toBe('qr');
    expect(kitServerNeed('recover', 'https://sync.moonyfinance.com')).toBe('none');
    expect(kitServerNeed('recover', null)).toBe('none');
  });

  it('needs nothing for any other text', () => {
    expect(kitServerNeed('unknown', null)).toBe('none');
    expect(kitServerNeed('pair', null)).toBe('none');
    expect(kitServerNeed('connection', null)).toBe('none');
  });
});

describe('hostOf', () => {
  it('shows host and port of an address', () => {
    expect(hostOf('http://192.168.1.2:8787')).toBe('192.168.1.2:8787');
    expect(hostOf('https://sync.example.com')).toBe('sync.example.com');
  });

  it('leaves what is no address as it is', () => {
    expect(hostOf('nonsense')).toBe('nonsense');
  });
});
