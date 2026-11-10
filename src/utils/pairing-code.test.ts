import { describe, expect, it } from 'vitest';
import type { PairingStage, PairingView } from '@shared/schema';
import { formatComparisonCode, pairingPollInterval, scannedCodeKind } from './pairing-code';

const CODE80 = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789'.repeat(3).slice(0, 80);
const GROUPED = CODE80.match(/.{1,5}/g)!.join('-');

function view(stage: PairingStage): PairingView {
  return {
    stage,
    role: 'joiner',
    qrSvg: null,
    pairingCode: null,
    comparisonCode: null,
    contested: false,
    expiresAt: 0,
  };
}

describe('scannedCodeKind', () => {
  it('tells pairing codes, kit QRs and typed recovery codes apart', () => {
    expect(scannedCodeKind('moony-pair:1:x')).toBe('pair');
    expect(scannedCodeKind('  moony-pair:1:x\n')).toBe('pair');
    expect(scannedCodeKind(' moony-recover:1:abc')).toBe('recover');
    expect(scannedCodeKind(GROUPED)).toBe('recoveryCode');
    expect(scannedCodeKind(GROUPED.replace(/-/g, ' '))).toBe('recoveryCode');
    expect(scannedCodeKind(CODE80)).toBe('recoveryCode');
    expect(scannedCodeKind(CODE80.slice(0, 79))).toBe('unknown');
    expect(scannedCodeKind(`${CODE80}A`)).toBe('unknown');
    expect(scannedCodeKind(`U${CODE80.slice(1)}`)).toBe('unknown');
    expect(scannedCodeKind('hello')).toBe('unknown');
    expect(scannedCodeKind('')).toBe('unknown');
  });
});

describe('formatComparisonCode', () => {
  it('groups the comparison code in threes', () => {
    expect(formatComparisonCode('482686')).toBe('482 686');
    expect(formatComparisonCode('0042')).toBe('0042');
    expect(formatComparisonCode('48268a')).toBe('48268a');
  });
});

describe('pairingPollInterval', () => {
  it('polls only while announcing or waiting for the other device', () => {
    expect(pairingPollInterval(view('announcing'))).toBe(2000);
    expect(pairingPollInterval(view('waitingForScan'))).toBe(2000);
    expect(pairingPollInterval(view('waitingForOther'))).toBe(2000);
    expect(pairingPollInterval(view('compare'))).toBe(false);
    expect(pairingPollInterval(view('sent'))).toBe(false);
    expect(pairingPollInterval(view('received'))).toBe(false);
    expect(pairingPollInterval(undefined)).toBe(false);
  });
});
