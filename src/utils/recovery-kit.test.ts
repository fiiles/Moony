import { describe, expect, it } from 'vitest';
import type { RecoveryKit } from '@shared/schema';
import { recoveryKitHtml, recoveryKitText, type RecoveryKitCopy } from './recovery-kit';

const CODE = Array.from({ length: 16 }, (_, i) => `ABCD${i % 10}`).join('-');

const kit: RecoveryKit = {
  accountNumber: '1234567890123456',
  recoveryCode: CODE,
  qrSvg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0h1v1z"/></svg>',
  relayUrl: null,
};

const copy: RecoveryKitCopy = {
  title: 'Sync Recovery Kit',
  intro: 'Keep this page safe.',
  accountLabel: 'Sync account',
  accountNote: 'Identifies your sync account.',
  codeLabel: 'Recovery code',
  codeNote: 'Opens your synced data.',
  qrLabel: 'Scan to restore',
  differsFromRecoveryKey: 'This is not your recovery key.',
  created: 'Created 9 November 2026',
  relayLabel: 'Sync server',
  relayNote: 'Needed to restore from this kit',
};

describe('recoveryKitHtml', () => {
  it('builds a self-contained page with the code, account and QR', () => {
    const html = recoveryKitHtml(kit, copy);
    expect(html.startsWith('<!doctype html>')).toBe(true);
    expect(html).toContain(CODE);
    expect(html).toContain('1234 5678 9012 3456');
    expect(html).toContain('<img src="data:image/svg+xml;base64,');
    expect(html).toContain('@media print');
    expect(html).toContain('@page');
    expect(html).not.toContain('http://');
    expect(html).not.toContain('https://');
    expect(html).not.toContain('Sync server');
  });

  it('encodes the QR svg as base64', () => {
    const html = recoveryKitHtml(kit, copy);
    const b64 = /base64,([^"]+)"/.exec(html)?.[1] ?? '';
    expect(atob(b64)).toBe(kit.qrSvg);
  });

  it('escapes every copy string', () => {
    const evil = Object.fromEntries(
      Object.keys(copy).map((k) => [k, '<script>x</script>'])
    ) as unknown as RecoveryKitCopy;
    const html = recoveryKitHtml({ ...kit, relayUrl: 'https://a.example/?q=<b>' }, evil);
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
    expect(html).toContain('https://a.example/?q=&lt;b&gt;');
  });

  it('prints the sync server only when the kit carries one', () => {
    const withRelay = recoveryKitHtml({ ...kit, relayUrl: 'https://sync.example.com' }, copy);
    expect(withRelay).toContain('Sync server');
    expect(withRelay).toContain('https://sync.example.com');
    expect(withRelay).toContain('Needed to restore from this kit');
    expect(recoveryKitHtml(kit, copy)).not.toContain('Needed to restore');
  });
});

describe('recoveryKitText', () => {
  it('writes the same facts as plain text', () => {
    const text = recoveryKitText(kit, copy);
    expect(text).toContain('Sync Recovery Kit');
    expect(text).toContain('Sync account: 1234 5678 9012 3456');
    expect(text).toContain(`Recovery code: ${CODE}`);
    expect(text).toContain('This is not your recovery key.');
    expect(text).toContain('Created 9 November 2026');
    expect(text).not.toContain('Sync server');
  });

  it('adds the sync server line only for a custom server', () => {
    const text = recoveryKitText({ ...kit, relayUrl: 'https://sync.example.com' }, copy);
    expect(text).toContain('Sync server: https://sync.example.com');
    expect(text).toContain('Needed to restore from this kit');
  });
});
