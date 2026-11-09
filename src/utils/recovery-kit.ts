import type { RecoveryKit } from '@shared/schema';

/** Every line of text the kit prints, already translated; the util only lays it out. */
export interface RecoveryKitCopy {
  title: string;
  intro: string;
  accountLabel: string;
  accountNote: string;
  codeLabel: string;
  codeNote: string;
  qrLabel: string;
  differsFromRecoveryKey: string;
  /** The finished line, e.g. "Created 9 November 2026". */
  created: string;
  /** Printed only when the kit carries a custom sync server address. */
  relayLabel: string;
  relayNote: string;
}

/** The 16-digit account number in groups of four. */
export function groupAccountNumber(accountNumber: string): string {
  return accountNumber.replace(/(\d{4})(?=\d)/g, '$1 ');
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function toBase64(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

const STYLE = `
  @page { size: A4; margin: 18mm; }
  * { box-sizing: border-box; }
  body { margin: 0; padding: 32px 20px; background: #fff; color: #1b1a17;
    font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
  main { max-width: 640px; margin: 0 auto; }
  h1 { font-size: 26px; margin: 0 0 8px; letter-spacing: -0.03em; }
  h2 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; color: #6b6860; margin: 28px 0 6px; }
  p { margin: 0 0 8px; }
  .note { color: #6b6860; font-size: 13px; }
  .value { font: 600 18px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    background: #1b1a17; color: #fff; padding: 12px 16px; border-radius: 10px; word-break: break-all; }
  .qr { text-align: center; margin: 28px 0 8px; }
  .qr img { width: 220px; height: 220px; }
  @media print {
    body { padding: 0; }
    .value { background: #fff; color: #000; border: 1.5px solid #000; }
    section { break-inside: avoid; }
  }
`;

/** The Sync Recovery Kit as one self-contained page: inline CSS, the QR as a data URI, no links. */
export function recoveryKitHtml(kit: RecoveryKit, copy: RecoveryKitCopy): string {
  const e = escapeHtml;
  const relay = kit.relayUrl
    ? `<section><h2>${e(copy.relayLabel)}</h2><div class="value">${e(kit.relayUrl)}</div><p class="note">${e(copy.relayNote)}</p></section>`
    : '';
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${e(copy.title)}</title>
<style>${STYLE}</style>
</head>
<body>
<main>
<h1>${e(copy.title)}</h1>
<p>${e(copy.intro)}</p>
<section><h2>${e(copy.accountLabel)}</h2><div class="value">${e(groupAccountNumber(kit.accountNumber))}</div><p class="note">${e(copy.accountNote)}</p></section>
<section><h2>${e(copy.codeLabel)}</h2><div class="value">${e(kit.recoveryCode)}</div><p class="note">${e(copy.codeNote)}</p></section>
${relay}
<section class="qr"><img src="data:image/svg+xml;base64,${toBase64(kit.qrSvg)}" alt="${e(copy.qrLabel)}"><p class="note">${e(copy.qrLabel)}</p></section>
<p class="note">${e(copy.differsFromRecoveryKey)}</p>
<p class="note">${e(copy.created)}</p>
</main>
</body>
</html>
`;
}

/** The same facts as plain text, for a .txt file. */
export function recoveryKitText(kit: RecoveryKit, copy: RecoveryKitCopy): string {
  const lines = [
    copy.title,
    '',
    copy.intro,
    '',
    `${copy.accountLabel}: ${groupAccountNumber(kit.accountNumber)}`,
    copy.accountNote,
    '',
    `${copy.codeLabel}: ${kit.recoveryCode}`,
    copy.codeNote,
  ];
  if (kit.relayUrl) lines.push('', `${copy.relayLabel}: ${kit.relayUrl}`, copy.relayNote);
  lines.push('', copy.differsFromRecoveryKey, '', copy.created, '');
  return lines.join('\n');
}
