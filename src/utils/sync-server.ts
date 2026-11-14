import { scannedCodeKind, stripInvisible } from '@/utils/pairing-code';

/**
 * The connection code of a self-hosted sync server and the server addresses it carries. The
 * mirror of `services/sync/connection_code.rs`: the backend has the last word on every address,
 * these only name the problem early (and show the server before anything is sent).
 */

export const CONNECTION_CODE_PREFIX = 'moony-sync1:';

/** The Moony cloud: plain http is never allowed to it (`DEFAULT_RELAY_HOST` in Rust). */
const DEFAULT_SYNC_HOST = 'sync.moonyfinance.com';
/** `MAX_CONNECTION_CODE_CHARS` and `MAX_RELAY_URL_CHARS` in Rust. */
const MAX_CODE_CHARS = 512;
const MAX_ADDRESS_CHARS = 256;

export type ConnectionCodeRead =
  | { ok: true; server: string; account: string }
  | {
      ok: false;
      problem:
        | 'validation.syncConnectionCodeInvalid'
        | 'validation.syncConnectionCodeNewer'
        | 'validation.syncRelayUrlInvalid';
    };

/** Trimmed, every White_Space and zero-width character removed, one trailing `.` or `,` dropped. */
function cleaned(text: string): string {
  const code = stripInvisible(text);
  return /[.,]$/.test(code) ? code.slice(0, -1) : code;
}

/** `moony-sync<digits>:` and what follows it, or `null`. */
function splitPrefix(code: string): { version: string; rest: string } | null {
  const match = /^moony-sync(\d+):([\s\S]*)$/.exec(code);
  return match ? { version: match[1], rest: match[2] } : null;
}

/** Whether `text` looks like a connection code of any version. */
export function isConnectionCode(text: string): boolean {
  return splitPrefix(cleaned(text)) !== null;
}

/** base64url-without-padding of a UTF-8 address, decoded; `null` when it is anything else. */
export function decodeServerField(encoded: string): string | null {
  if (!/^[A-Za-z0-9_-]+$/.test(encoded)) return null;
  try {
    const binary = atob(encoded.replace(/-/g, '+').replace(/_/g, '/'));
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** The address in one spelling (`scheme://host[:port]`), or `null` when the app would refuse it. */
export function normalizeAddress(address: string): string | null {
  if ([...address].length > MAX_ADDRESS_CHARS || /[?#]/.test(address)) return null;
  let url: URL;
  try {
    url = new URL(address);
  } catch {
    return null;
  }
  const host = url.hostname.replace(/\.+$/, '');
  const bare = url.username === '' && url.password === '' && url.pathname === '/';
  if (!bare || host === '') return null;
  if (url.protocol === 'http:' && host === DEFAULT_SYNC_HOST) return null;
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
  const port = url.port === '' ? '' : `:${url.port}`;
  const normal = `${url.protocol}//${host}${port}`;
  return normal.length > MAX_ADDRESS_CHARS ? null : normal;
}

/** Whether an address is the Moony cloud's: its normalised origin is the default server's. */
export function isDefaultServer(address: string): boolean {
  return normalizeAddress(address) === `https://${DEFAULT_SYNC_HOST}`;
}

/** Reads a connection code the way the backend does. */
export function parseConnectionCode(text: string): ConnectionCodeRead {
  const invalid = { ok: false, problem: 'validation.syncConnectionCodeInvalid' } as const;
  const code = cleaned(text);
  if ([...code].length > MAX_CODE_CHARS) return invalid;
  const prefix = splitPrefix(code);
  if (!prefix) return invalid;
  if (prefix.version !== '1') {
    return prefix.version.startsWith('0')
      ? invalid
      : { ok: false, problem: 'validation.syncConnectionCodeNewer' };
  }
  const fields = prefix.rest.split(':');
  if (fields.length !== 2) return invalid;
  const [account, encoded] = fields;
  if (!/^\d{16}$/.test(account)) return invalid;
  const address = decodeServerField(encoded);
  if (address === null || address === '') return invalid;
  const server = normalizeAddress(address);
  if (server === null) return { ok: false, problem: 'validation.syncRelayUrlInvalid' };
  return { ok: true, server, account };
}

/**
 * The server a pairing code (fourth field), a Recovery Kit QR (fourth field) or a connection code
 * names, in its normalized spelling; `null` when the text names none or an unusable one.
 */
export function payloadServer(text: string): string | null {
  const trimmed = text.trim();
  let encoded: string | undefined;
  if (trimmed.startsWith('moony-pair:')) {
    // moony-pair:<version>:<mailbox>:<key>[:<server>]
    const fields = trimmed.slice('moony-pair:'.length).split(':');
    encoded = fields.length === 4 ? fields[3] : undefined;
  } else if (trimmed.startsWith('moony-recover:')) {
    // moony-recover:<version>:<account>:<code>[:<server>]
    const fields = trimmed.slice('moony-recover:'.length).split(':');
    encoded = fields.length === 4 ? fields[3] : undefined;
  } else {
    const read = parseConnectionCode(text);
    return read.ok ? read.server : null;
  }
  const address = encoded === undefined ? null : decodeServerField(encoded);
  return address === null ? null : normalizeAddress(address);
}

/** Where a code is typed or scanned. */
export type CodeContext = 'setup' | 'server';

/**
 * A `deviceSync` key naming what is wrong when `text` is the wrong kind of code for `context`, or
 * `null` when it fits or the backend should judge it.
 */
export function wrongCodeKey(context: CodeContext, text: string): string | null {
  const kind = scannedCodeKind(text);
  switch (context) {
    case 'setup':
      if (kind === 'pair') return 'codes.pairNotForSetup';
      if (kind === 'recover' || kind === 'recoveryCode') return 'codes.kitNotForSetup';
      return null;
    case 'server':
      return kind === 'pair' || kind === 'recover' || kind === 'recoveryCode'
        ? 'codes.notAServerAddress'
        : null;
  }
}
