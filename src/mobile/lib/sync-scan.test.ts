import { describe, expect, it } from 'vitest';
import type { PairingStage, PairingView } from '@shared/schema';
import {
  isScanCancelled,
  kitCodeProblem,
  pairingCodeProblem,
  pairingSheetStep,
  phoneDeviceKey,
  scanRoute,
} from './sync-scan';

const CODE80 = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789'.repeat(3).slice(0, 80);
const PAIR = 'moony-pair:1:abc:def';
const KIT_QR = `moony-recover:1:1234567812345678:${CODE80}`;

function view(stage: PairingStage, contested = false): PairingView {
  return {
    stage,
    role: 'joiner',
    qrSvg: null,
    pairingCode: null,
    comparisonCode: '482686',
    contested,
    expiresAt: 0,
  };
}

describe('scanRoute', () => {
  it('pairs on a pairing code whether sync is on or off', () => {
    expect(scanRoute(PAIR, 'join')).toBe('pair');
    expect(scanRoute(`  ${PAIR}\n`, 'add')).toBe('pair');
  });

  it('joins with a Recovery Kit QR only while sync is off', () => {
    expect(scanRoute(KIT_QR, 'join')).toBe('kitJoin');
    expect(scanRoute(KIT_QR, 'add')).toBe('alreadyOn');
  });

  it('refuses a bare recovery code (no account in a QR) and anything else', () => {
    expect(scanRoute(CODE80, 'join')).toBe('unknown');
    expect(scanRoute('https://example.com', 'join')).toBe('unknown');
    expect(scanRoute('', 'add')).toBe('unknown');
  });
});

describe('kitCodeProblem', () => {
  it('names a pairing code as not a Recovery Kit code', () => {
    expect(kitCodeProblem(PAIR)).toBe('phone.notAKitCode');
    expect(kitCodeProblem(` ${PAIR}`)).toBe('phone.notAKitCode');
  });

  it('accepts a Kit QR text, a typed code, and leaves the rest to the backend', () => {
    expect(kitCodeProblem(KIT_QR)).toBeNull();
    expect(kitCodeProblem(CODE80)).toBeNull();
    expect(kitCodeProblem('')).toBeNull();
    expect(kitCodeProblem('ABCDE-')).toBeNull();
  });
});

describe('pairingCodeProblem', () => {
  it('accepts only a pairing code', () => {
    expect(pairingCodeProblem(PAIR)).toBeNull();
    expect(pairingCodeProblem(`\n${PAIR} `)).toBeNull();
    expect(pairingCodeProblem(KIT_QR)).toBe('phone.notAPairingCode');
    expect(pairingCodeProblem(CODE80)).toBe('phone.notAPairingCode');
    expect(pairingCodeProblem('hello')).toBe('phone.notAPairingCode');
  });
});

describe('phoneDeviceKey', () => {
  it('names an Android build by its platform, anything else an iPhone', () => {
    expect(phoneDeviceKey('android')).toBe('android');
    expect(phoneDeviceKey('androideabi')).toBe('android');
    expect(phoneDeviceKey('ios')).toBe('ios');
    expect(phoneDeviceKey('')).toBe('ios');
  });
});

describe('isScanCancelled', () => {
  it('reads the plugin rejection of a cancelled scan, as a string or an Error', () => {
    expect(isScanCancelled('cancelled')).toBe(true);
    expect(isScanCancelled(new Error('cancelled'))).toBe(true);
  });

  it('keeps every other failure an error', () => {
    expect(isScanCancelled('No camera available on this device (e.g., iOS Simulator)')).toBe(false);
    expect(isScanCancelled('Camera permission denied or not yet requested')).toBe(false);
    expect(isScanCancelled(undefined)).toBe(false);
  });
});

describe('pairingSheetStep', () => {
  it('shows a failed poll before anything else', () => {
    expect(pairingSheetStep(view('compare'), { failed: true, expired: false })).toBe('failed');
    expect(pairingSheetStep(view('compare', true), { failed: true, expired: true })).toBe('failed');
  });

  it('shows an expired pairing only while it was still waiting', () => {
    for (const stage of ['announcing', 'waitingForScan', 'compare', 'waitingForOther'] as const) {
      expect(pairingSheetStep(view(stage), { failed: false, expired: true })).toBe('expired');
    }
    expect(pairingSheetStep(view('sent'), { failed: false, expired: true })).toBe('sent');
    expect(pairingSheetStep(view('received'), { failed: false, expired: true })).toBe('received');
  });

  it('shows a contested scan as its own step (its own digits, a warning, only Cancel)', () => {
    expect(pairingSheetStep(view('compare', true), { failed: false, expired: false })).toBe(
      'contested'
    );
  });

  it('otherwise follows the stage', () => {
    for (const stage of [
      'announcing',
      'waitingForScan',
      'compare',
      'waitingForOther',
      'sent',
      'received',
    ] as const) {
      expect(pairingSheetStep(view(stage), { failed: false, expired: false })).toBe(stage);
    }
  });
});
