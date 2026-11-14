import { describe, expect, it } from 'vitest';
import {
  CONNECTION_CODE_PREFIX,
  decodeServerField,
  isDefaultServer,
  normalizeAddress,
  parseConnectionCode,
  payloadServer,
  wrongCodeKey,
  type CodeContext,
} from './sync-server';

const ACCOUNT = '4417938205561273';
const SERVER_FIELD = 'aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn';
const CODE = `moony-sync1:${ACCOUNT}:${SERVER_FIELD}`;
const b64 = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');

describe('parseConnectionCode', () => {
  it('reads the server and the account', () => {
    expect(CONNECTION_CODE_PREFIX).toBe('moony-sync1:');
    expect(parseConnectionCode(CODE)).toEqual({
      ok: true,
      server: 'https://sync.example.org',
      account: ACCOUNT,
    });
  });

  it('reads a code mangled by a chat app', () => {
    let mangled = '  ';
    [...CODE].forEach((c, i) => {
      if (i === 20) mangled += '\n';
      if (i > 0 && i % 8 === 0) mangled += ' ';
      if (i === 30) mangled += '\u200B';
      mangled += c;
    });
    mangled += '.  ';
    expect(parseConnectionCode(mangled)).toEqual({
      ok: true,
      server: 'https://sync.example.org',
      account: ACCOUNT,
    });
    expect(parseConnectionCode(`${CODE},`)).toMatchObject({ ok: true, account: ACCOUNT });
  });

  it('normalises http codes and upper case hosts', () => {
    expect(parseConnectionCode(`moony-sync1:${ACCOUNT}:SFRUUDovL0hvc3QuRXhhbXBsZTo4MC8`)).toEqual({
      ok: true,
      server: 'http://host.example',
      account: ACCOUNT,
    });
    expect(parseConnectionCode(`moony-sync1:${ACCOUNT}:aHR0cDovLzE5Mi4xNjguMS4yMDo4Nzg3`)).toEqual({
      ok: true,
      server: 'http://192.168.1.20:8787',
      account: ACCOUNT,
    });
  });

  it.each([
    ['15 digits', `moony-sync1:441793820556127:${SERVER_FIELD}`],
    ['a letter in the account', `moony-sync1:441793820556127a:${SERVER_FIELD}`],
    ['a bad server field', `moony-sync1:${ACCOUNT}:!!`],
    ['an empty server', `moony-sync1:${ACCOUNT}:`],
    ['a fourth field', `moony-sync1:${ACCOUNT}:${SERVER_FIELD}:x`],
    ['513 characters', `moony-sync1:${ACCOUNT}:${'A'.repeat(513)}`],
    ['version 0', `moony-sync0:${ACCOUNT}:${SERVER_FIELD}`],
    ['no prefix', ACCOUNT],
    ['nothing', ''],
  ])('refuses %s as an invalid code', (_label, text) => {
    expect(parseConnectionCode(text)).toEqual({
      ok: false,
      problem: 'validation.syncConnectionCodeInvalid',
    });
  });

  it('names a newer code', () => {
    expect(parseConnectionCode(`moony-sync2:${ACCOUNT}:${SERVER_FIELD}`)).toEqual({
      ok: false,
      problem: 'validation.syncConnectionCodeNewer',
    });
    expect(parseConnectionCode('moony-sync2:abc')).toMatchObject({
      problem: 'validation.syncConnectionCodeNewer',
    });
  });

  it('reads a code that starts with a zero-width character', () => {
    for (const lead of ['\u200B', '\u200C', '\u200D', '\uFEFF']) {
      expect(parseConnectionCode(`${lead}${CODE}`)).toMatchObject({ ok: true, account: ACCOUNT });
    }
  });

  it.each([
    'ftp://x.example',
    'https://x.example/p',
    'http://sync.moonyfinance.com',
    'http://sync.moonyfinance.com.',
    'http://sync.moonyfinance.com..',
  ])('refuses the server %s', (server) => {
    expect(parseConnectionCode(`moony-sync1:${ACCOUNT}:${b64(server)}`)).toEqual({
      ok: false,
      problem: 'validation.syncRelayUrlInvalid',
    });
  });
});

describe('decodeServerField', () => {
  it('decodes base64url without padding and refuses the rest', () => {
    expect(decodeServerField(SERVER_FIELD)).toBe('https://sync.example.org');
    expect(decodeServerField('')).toBeNull();
    expect(decodeServerField('!!')).toBeNull();
    expect(decodeServerField(`${SERVER_FIELD}==`)).toBeNull();
    expect(decodeServerField('_w')).toBeNull(); // not UTF-8
  });
});

describe('payloadServer', () => {
  const CODE80 = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789'.repeat(3).slice(0, 80);
  const key = 'e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM';

  it('names the server of a pairing code with a fourth field', () => {
    expect(payloadServer(`moony-pair:2:MzMzMzMzMzMzMzMzMzMzMw:${key}:${SERVER_FIELD}`)).toBe(
      'https://sync.example.org'
    );
    expect(payloadServer(`moony-pair:2:MzMzMzMzMzMzMzMzMzMzMw:${key}`)).toBeNull();
  });

  it('names the server of a Recovery Kit QR', () => {
    expect(payloadServer(`moony-recover:1:${ACCOUNT}:${CODE80}:${SERVER_FIELD}`)).toBe(
      'https://sync.example.org'
    );
    expect(payloadServer(`moony-recover:1:${ACCOUNT}:${CODE80}`)).toBeNull();
  });

  it('names the server of a connection code, and none for a code without one', () => {
    expect(payloadServer(CODE)).toBe('https://sync.example.org');
    expect(payloadServer(`moony-sync1:${ACCOUNT}`)).toBeNull();
    expect(payloadServer('https://sync.example.org')).toBeNull();
  });
});

describe('wrongCodeKey', () => {
  const PAIR = 'moony-pair:2:a:b';
  const RECOVER = `moony-recover:1:${ACCOUNT}:x`;
  const CODE80 = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789'.repeat(3).slice(0, 80);
  const table: [CodeContext, string, string | null][] = [
    ['setup', PAIR, 'codes.pairNotForSetup'],
    ['setup', RECOVER, 'codes.kitNotForSetup'],
    ['setup', CODE80, 'codes.kitNotForSetup'],
    ['setup', CODE, null],
    ['setup', 'https://sync.example.org', null],
    ['pairing', CODE, 'codes.connectionNotHere'],
    ['pairing', PAIR, null],
    ['pairing', RECOVER, null],
    ['kit', CODE, 'codes.connectionNotHere'],
    ['kit', RECOVER, null],
    ['kit', CODE80, null],
    ['server', PAIR, 'codes.notAServerAddress'],
    ['server', RECOVER, 'codes.notAServerAddress'],
    ['server', CODE80, 'codes.notAServerAddress'],
    ['server', CODE, null],
    ['server', 'https://sync.example.org', null],
  ];
  it.each(table)('%s with %s gives %s', (context, text, key) => {
    expect(wrongCodeKey(context, text)).toBe(key);
  });
});

describe('normalizeAddress', () => {
  it('strips every trailing dot, so the cloud host cannot hide behind them', () => {
    expect(normalizeAddress('https://sync.moonyfinance.com..')).toBe(
      'https://sync.moonyfinance.com'
    );
    expect(normalizeAddress('http://sync.moonyfinance.com...:80/')).toBeNull();
  });
});

describe('isDefaultServer', () => {
  it('is the Moony cloud origin, however it is spelled, and nothing else', () => {
    expect(isDefaultServer('https://sync.moonyfinance.com')).toBe(true);
    expect(isDefaultServer('https://sync.moonyfinance.com/')).toBe(true);
    expect(isDefaultServer('https://sync.moonyfinance.com.')).toBe(true);
    expect(isDefaultServer('https://sync.moonyfinance.com:8443')).toBe(false);
    expect(isDefaultServer('http://sync.moonyfinance.com')).toBe(false);
    expect(isDefaultServer('https://sync.example.org')).toBe(false);
    expect(isDefaultServer('nope')).toBe(false);
  });
});
