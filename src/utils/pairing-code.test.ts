import { describe, expect, it } from 'vitest';
import type { PairingStage, PairingView } from '@shared/schema';
import {
  formatComparisonCode,
  isCompleteCode,
  joinFailureAction,
  normalizeTypedCode,
  pairingCodeProblem,
  pairingPollInterval,
  scannedCodeKind,
  type JoinFailurePoll,
} from './pairing-code';

const CODE95 = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789'.repeat(3).slice(0, 95);
const GROUPED = CODE95.match(/.{1,5}/g)!.join('-');

function view(stage: PairingStage): PairingView {
  return {
    stage,
    role: 'joiner',
    qrSvg: null,
    pairingCode: null,
    comparisonCode: null,
    contested: false,
    expiresAt: 0,
    serverHost: null,
    joinServer: null,
    joinServerInsecure: false,
    attemptsLeft: null,
  };
}

describe('scannedCodeKind', () => {
  it('tells pairing codes, kit QRs and typed recovery codes apart', () => {
    expect(scannedCodeKind('moony-pair:1:x')).toBe('pair');
    expect(scannedCodeKind('  moony-pair:1:x\n')).toBe('pair');
    expect(scannedCodeKind(' moony-recover:1:abc')).toBe('recover');
    expect(scannedCodeKind('moony-sync1:4417938205561273:aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn')).toBe(
      'connection'
    );
    expect(scannedCodeKind(' moony-sync2:abc\n')).toBe('connection');
    expect(scannedCodeKind('moony-sync:abc')).toBe('unknown');
    expect(scannedCodeKind(GROUPED)).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, ' '))).toBe('recoveryCode');
    expect(scannedCodeKind(CODE95)).toBe('recoveryCode');
    expect(scannedCodeKind(CODE95.slice(0, 94))).toBe('unknown');
    expect(scannedCodeKind(`${CODE95}A`)).toBe('unknown');
    expect(scannedCodeKind(`U${CODE95.slice(1)}`)).toBe('unknown');
    expect(scannedCodeKind('hello')).toBe('unknown');
    expect(scannedCodeKind('')).toBe('unknown');
  });
});

describe('scannedCodeKind reads a connection code the way connection_code.rs cleans it', () => {
  const CONNECTION = 'moony-sync1:4417938205561273:aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn';

  it('ignores white space and zero-width characters inside the code', () => {
    expect(scannedCodeKind(`${CONNECTION.slice(0, 20)}\n${CONNECTION.slice(20)}`)).toBe(
      'connection'
    );
    expect(scannedCodeKind(`\u200Bmoony-sync1:\u00A04417938205561273:\u200Dabc\uFEFF`)).toBe(
      'connection'
    );
    expect(scannedCodeKind(`moony-\u200Bsync1:4417938205561273:abc`)).toBe('connection');
  });

  it('keeps the other kinds as they were (their parsers only trim)', () => {
    expect(scannedCodeKind('\u200Bmoony-pair:2:x')).toBe('unknown');
    expect(scannedCodeKind('moony-\npair:2:x')).toBe('unknown');
  });
});

describe('scannedCodeKind as the Rust decoder reads it', () => {
  it('ignores case, I/L/O look-alikes and every dash the decoder ignores', () => {
    expect(scannedCodeKind(CODE95.toLowerCase())).toBe('recoveryCode');
    expect(scannedCodeKind(`I${CODE95.slice(1)}`)).toBe('recoveryCode');
    expect(scannedCodeKind(`l${CODE95.slice(1)}`)).toBe('recoveryCode');
    expect(scannedCodeKind(`o${CODE95.slice(1)}`)).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, '\u2013'))).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, '\u2010'))).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, '\u2015'))).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, '\u2016'))).toBe('unknown');
  });

  it('matches the scheme prefixes exactly, from the start, after trimming', () => {
    expect(scannedCodeKind('MOONY-PAIR:1:x')).toBe('unknown');
    expect(scannedCodeKind('xmoony-pair:1:x')).toBe('unknown');
    expect(scannedCodeKind('xmoony-recover:1:x')).toBe('unknown');
    expect(scannedCodeKind('moony-recover:garbage')).toBe('recover');
  });

  it('rejects NUL, unicode and very long input', () => {
    expect(scannedCodeKind(`${CODE95}\u0000`)).toBe('unknown');
    expect(scannedCodeKind(`\u00DF${CODE95.slice(2)}`)).toBe('unknown');
    expect(scannedCodeKind(`${CODE95.slice(1)}\u0131`)).toBe('unknown');
    expect(scannedCodeKind(`${CODE95.slice(0, 94)}\uFEFF0`)).toBe('unknown');
    expect(scannedCodeKind('A'.repeat(100000))).toBe('unknown');
  });
});

describe('formatComparisonCode', () => {
  it('groups the comparison code in threes', () => {
    expect(formatComparisonCode('482686')).toBe('482 686');
    expect(formatComparisonCode('0042')).toBe('0042');
    expect(formatComparisonCode('48268a')).toBe('48268a');
  });
});

describe('normalizeTypedCode', () => {
  it('keeps digits only, at most six', () => {
    expect(normalizeTypedCode('482 686')).toBe('482686');
    expect(normalizeTypedCode('482-686')).toBe('482686');
    expect(normalizeTypedCode(' 4a8b2 ')).toBe('482');
    expect(normalizeTypedCode('4826869')).toBe('482686');
    expect(normalizeTypedCode('')).toBe('');
  });

  it('folds full-width digits to ASCII before stripping', () => {
    expect(normalizeTypedCode('４８２６８６')).toBe('482686');
    expect(normalizeTypedCode('４８２ ６８６')).toBe('482686');
  });

  it('drops other scripts instead of guessing', () => {
    expect(normalizeTypedCode('٤٨٢٦٨٦')).toBe('');
  });
});

describe('isCompleteCode', () => {
  it('is true for exactly six digits once normalised', () => {
    expect(isCompleteCode('482 686')).toBe(true);
    expect(isCompleteCode('４８２６８６')).toBe(true);
    expect(isCompleteCode('48268')).toBe(false);
    expect(isCompleteCode('')).toBe(false);
  });
});

describe('pairingPollInterval', () => {
  it('polls only while announcing, connecting or waiting for the other device', () => {
    expect(pairingPollInterval(view('announcing'))).toBe(2000);
    expect(pairingPollInterval(view('connecting'))).toBe(2000);
    expect(pairingPollInterval(view('waitingForScan'))).toBe(2000);
    expect(pairingPollInterval(view('waitingForOther'))).toBe(2000);
    expect(pairingPollInterval(view('compare'))).toBe(false);
    expect(pairingPollInterval(view('sent'))).toBe(false);
    expect(pairingPollInterval(view('received'))).toBe(false);
    expect(pairingPollInterval(undefined)).toBe(false);
  });
});

describe('joinFailureAction', () => {
  const SETUP_PENDING = 'validation.syncJoinSetupPending';
  const IN_PROGRESS = 'validation.syncChangeInProgress';
  const RELAY = 'validation.syncRelayError';
  const kept: JoinFailurePoll = { ok: true, stage: 'received' };
  const notStarted: JoinFailurePoll = { ok: false, key: 'validation.syncPairingNotStarted' };
  const expired: JoinFailurePoll = { ok: false, key: 'validation.syncPairingExpired' };

  it.each<[string, JoinFailurePoll, string | undefined, boolean, boolean]>([
    // The backend still holds the received session: the same join can run again
    ['kept after a relay failure', kept, RELAY, true, false],
    ['kept while offline (no key)', kept, undefined, true, false],
    ['kept after a rate limit', kept, 'validation.syncRateLimited', true, false],
    ['kept while another change runs', kept, IN_PROGRESS, true, false],
    ['kept behind a half-finished setup', kept, SETUP_PENDING, true, true],
    // The join ended it (or it expired meanwhile): a fresh pairing
    ['ended after a relay failure', notStarted, RELAY, false, false],
    ['ended after a wrong account', notStarted, 'validation.syncPairingFailed', false, false],
    ['expired meanwhile', expired, RELAY, false, false],
    ['expired behind a half-finished setup', expired, SETUP_PENDING, false, false],
    // Another stage is no received session to join with
    ['another stage', { ok: true, stage: 'compare' }, RELAY, false, false],
    // The poll itself failed otherwise: the next join's failure is asked again
    ['poll failed otherwise', { ok: false, key: 'validation.syncLocked' }, RELAY, true, false],
    ['poll failed without a key', { ok: false }, SETUP_PENDING, true, true],
  ])('%s', (_label, poll, errorKey, retryable, setupPending) => {
    expect(joinFailureAction(poll, errorKey)).toEqual({ retryable, setupPending });
  });
});

describe('pairingCodeProblem', () => {
  it('accepts a pairing code and names what else was typed', () => {
    expect(pairingCodeProblem('moony-pair:abc')).toBeNull();
    expect(pairingCodeProblem('  moony-pair:abc\n')).toBeNull();
    expect(pairingCodeProblem('moony-sync1:xyz')).toBe('codes.connectionNotHere');
    expect(pairingCodeProblem('hello')).toBe('phone.notAPairingCode');
  });
});
