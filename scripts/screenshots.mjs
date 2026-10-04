#!/usr/bin/env node
/**
 * Capture the README screenshots from a running dev app (`npm run tauri dev` with the
 * browser dev bridge, see CONTRIBUTING.md → "Browser dev bridge") using headless Chrome.
 *
 *   node scripts/screenshots.mjs [--base http://localhost:1420] [--out docs/screenshots]
 *                                [--account <bankAccountId>] [--fixture <csv path>]
 *
 * The app must be unlocked (the bridge relays commands to the Tauri window). Each page is
 * loaded in a fresh headless profile with the analytics consent already answered, waited
 * for until its data is on screen, and captured at 1440 × 900 CSS px with a 2× scale.
 * No dependencies: it talks to Chrome over the DevTools protocol with Node's WebSocket.
 */
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const opt = {
  base: 'http://localhost:1420',
  out: 'docs/screenshots',
  account: null,
  fixture: null,
  width: 1440,
  height: 900,
  port: 9333,
};
for (let i = 0; i < args.length; i++) {
  const key = args[i].replace(/^--/, '');
  if (key in opt) opt[key] = args[++i];
}

const CHROME_CANDIDATES = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
].filter(Boolean);
const chromePath = CHROME_CANDIDATES.find((p) => existsSync(p));
if (!chromePath) {
  console.error('Chrome not found; set CHROME_PATH');
  process.exit(1);
}

// Pages to capture: [file name, path, readiness predicate (JS), actions (JS) run before the shot]
const notLoading = `!document.body.innerText.includes('Loading')`;
const pages = [
  [
    'dashboard',
    '/',
    `document.body.innerText.includes('Asset allocation') && ${notLoading}`,
    [],
  ],
  [
    'bank-accounts',
    '/bank-accounts',
    `document.querySelectorAll('table tbody tr').length > 0 && ${notLoading}`,
    [],
  ],
  [
    'stocks',
    '/stocks',
    `document.querySelectorAll('table tbody tr').length > 0 && ${notLoading}`,
    [],
  ],
  ['settings', '/settings/general', `document.body.innerText.includes('Language and formats')`, []],
];
if (opt.account) {
  pages.push([
    'account-detail',
    `/bank-accounts/${opt.account}`,
    `document.body.innerText.includes('Transactions') && ${notLoading}`,
    [
      `[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Show all')?.click();`,
    ],
  ]);
  if (opt.fixture) {
    pages.push([
      'import',
      `/bank-accounts/${opt.account}`,
      `document.body.innerText.includes('Import CSV') && ${notLoading}`,
      [
        `[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Import CSV').click();`,
        // The native file dialog cannot open in headless Chrome: answer it with the fixture path.
        `const i = window.__TAURI_INTERNALS__; const orig = i.invoke.bind(i);
         i.invoke = (cmd, a, o) => cmd === 'plugin:dialog|open' ? Promise.resolve(${JSON.stringify(opt.fixture)}) : orig(cmd, a, o);
         [...document.querySelectorAll('button')].find(b => b.textContent.trim().startsWith('Choose file')).click();`,
      ],
    ]);
  }
}

const seed = {
  'moony-analytics-consent': 'false',
  'moony-analytics-consent-asked-version': '1',
  'moony-language': 'en',
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const profile = mkdtempSync(join(tmpdir(), 'moony-shots-'));
const chrome = spawn(
  chromePath,
  [
    '--headless=new',
    '--disable-gpu',
    '--hide-scrollbars',
    '--no-first-run',
    `--remote-debugging-port=${opt.port}`,
    `--user-data-dir=${profile}`,
    `--window-size=${opt.width},${opt.height}`,
    'about:blank',
  ],
  { stdio: 'ignore' }
);

let id = 0;
const pending = new Map();
let ws;
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const msgId = ++id;
    pending.set(msgId, { resolve, reject });
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
async function evaluate(expression) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 300));
  return r.result?.value;
}
async function waitUntil(predicate, timeoutMs) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      if (await evaluate(`(() => { try { return !!(${predicate}); } catch { return false; } })()`))
        return true;
    } catch {
      /* page is navigating */
    }
    await sleep(250);
  }
  return false;
}

try {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${opt.port}/json/version`)).ok) break;
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  const target = await (
    await fetch(`http://127.0.0.1:${opt.port}/json/new?${opt.base}/auth`, { method: 'PUT' })
  ).json();
  ws = new WebSocket(target.webSocketDebuggerUrl);
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    const p = m.id && pending.get(m.id);
    if (!p) return;
    pending.delete(m.id);
    m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result);
  };
  await new Promise((r) => (ws.onopen = r));
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', {
    width: Number(opt.width),
    height: Number(opt.height),
    deviceScaleFactor: 2,
    mobile: false,
  });
  await waitUntil('document.readyState === "complete"', 10000);
  await evaluate(
    `(() => { ${Object.entries(seed)
      .map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)});`)
      .join('')} return true; })()`
  );
  mkdirSync(opt.out, { recursive: true });
  for (const [name, path, ready, actions] of pages) {
    await send('Page.navigate', { url: opt.base + path });
    await waitUntil('document.readyState === "complete"', 10000);
    if (!(await waitUntil(ready, 30000)))
      console.warn(`${name}: readiness check timed out, capturing anyway`);
    for (const action of actions) {
      await evaluate(`(async () => { ${action} })()`);
      await sleep(3000);
    }
    await sleep(2000);
    const shot = await send('Page.captureScreenshot', { format: 'png' });
    const file = join(opt.out, `${name}.png`);
    writeFileSync(file, Buffer.from(shot.data, 'base64'));
    console.log('wrote', file);
  }
} finally {
  try {
    ws?.close();
  } catch {
    /* already closed */
  }
  chrome.kill('SIGKILL');
}
