#!/usr/bin/env node
/**
 * Capture the README and website screenshots from a running dev app (`npm run tauri dev`
 * with the browser dev bridge, see CONTRIBUTING.md → "Browser dev bridge" and "Screenshots")
 * using headless Chrome.
 *
 *   node scripts/screenshots.mjs [--base http://localhost:1420] [--out docs/screenshots]
 *                                [--lang en|cs] [--theme system|light|dark]
 *                                [--suffix <s>] [--set readme|site|all]
 *                                [--account <bankAccountId>] [--fixture <csv path>]
 *                                [--width 1440] [--height 900] [--port 9333]
 *
 *   --lang     UI language of the shots (any shipped code, default `en`). An unlocked app
 *              takes its language from the profile, not from the browser: the script reads
 *              it through the bridge, switches it with `update_user_profile` when it differs
 *              and puts the original back when it finishes (also on Ctrl-C or an error).
 *   --theme    `system` (default), `light` or `dark`. `system` follows the browser, which
 *              headless Chrome reports as light, so the default shots are the light ones. Like
 *              the language, an unlocked app takes its theme from the profile: the script
 *              switches it with `update_user_profile` when it differs and puts the original
 *              back when it finishes. It also sets the device mirror `moony-theme` that the
 *              lock screen and the first paint read. The dark set, for example:
 *              `--theme dark --out docs/screenshots/dark`.
 *   --suffix   Appended to every file name; default empty for `en` and `-<lang>` otherwise
 *              (`dashboard.png`, `dashboard-cs.png`).
 *   --set      `readme` (default): dashboard, cashflow, account-detail, stocks-analysis, settings
 *              and, with --fixture, import. `site`: dashboard, account-detail, cashflow,
 *              planning, stocks, stocks-analysis, crypto, real-estate, settings. `all`: both.
 *   --account  Bank account for account-detail and import; default: the first account with a
 *              balance history (two or more balance records), else the first account.
 *   --fixture  CSV for the import shot; English only (`--lang en`), skipped otherwise.
 *
 * The app must be unlocked (the bridge relays commands to the Tauri window) and the profile
 * must be the demo or the anonymized one: before anything is captured or written the script
 * loads `/` and aborts unless the page shows "Alex" (and aborts at once on "Filip").
 * Each page is loaded in a fresh headless profile, waited for until its data is on screen
 * (DOM structure only: charts, table rows, no `.skeleton`, `<html lang>`; never UI words, so
 * the same checks hold in every language) and captured at 1440 × 900 CSS px with a 2× scale.
 * No dependencies: it talks to Chrome over the DevTools protocol with Node's WebSocket.
 */
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const LANGUAGES = ['en', 'cs', 'de', 'es', 'it', 'nb', 'pl', 'sv', 'ja', 'zh'];
const THEMES = ['system', 'light', 'dark'];
const SETS = ['readme', 'site', 'all'];
/** The demo and the anonymized profile are both "Alex Novák"; the owner's real one is not. */
const EXPECTED_NAME = 'Alex';
const FORBIDDEN_NAME = 'Filip';

const args = process.argv.slice(2);
const opt = {
  base: 'http://localhost:1420',
  out: 'docs/screenshots',
  account: null,
  fixture: null,
  width: 1440,
  height: 900,
  port: 9333,
  lang: 'en',
  theme: 'system',
  suffix: null,
  set: 'readme',
};
for (let i = 0; i < args.length; i++) {
  const key = args[i].replace(/^--/, '');
  if (!args[i].startsWith('--') || !(key in opt)) {
    console.error(`Unknown option ${args[i]} (see the header of scripts/screenshots.mjs)`);
    process.exit(2);
  }
  if (args[i + 1] === undefined) {
    console.error(`Option ${args[i]} needs a value`);
    process.exit(2);
  }
  opt[key] = args[++i];
}
if (!LANGUAGES.includes(opt.lang)) {
  console.error(`--lang must be one of ${LANGUAGES.join(', ')} (got "${opt.lang}")`);
  process.exit(2);
}
if (!THEMES.includes(opt.theme)) {
  console.error(`--theme must be one of ${THEMES.join(', ')} (got "${opt.theme}")`);
  process.exit(2);
}
if (!SETS.includes(opt.set)) {
  console.error(`--set must be one of ${SETS.join(', ')} (got "${opt.set}")`);
  process.exit(2);
}
opt.suffix ??= opt.lang === 'en' ? '' : `-${opt.lang}`;
// The backend reads the fixture from its own working directory, so hand it an absolute path.
if (opt.fixture) opt.fixture = resolve(opt.fixture);

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

// Readiness building blocks. They look at DOM structure only, never at words, so they hold in
// every language. Loading states are `.skeleton` blocks (design system §6).
const all = (...parts) => parts.map((p) => `(${p})`).join(' && ');
const loaded = `!!document.querySelector('main h1') && !document.querySelector('.skeleton')`;
const rows = (min = 1) => `document.querySelectorAll('main table tbody tr').length >= ${min}`;
const charts = (min = 1) => `document.querySelectorAll('main .recharts-surface').length >= ${min}`;

// Ids the pages need, asked from the app through the bridge's `invoke` (set below).
let invokeInPage = async () => {
  throw new Error('page not ready');
};
const ids = {
  // `--account`, else the first account with a balance history to draw (two or more dated
  // balance records), else simply the first account.
  account: async () => {
    if (opt.account) return opt.account;
    const accounts = await invokeInPage('get_all_bank_accounts');
    if (!accounts?.length) throw new Error('the profile has no bank account (pass --account)');
    for (const { id } of accounts) {
      const records = await invokeInPage('get_balance_records', { accountId: id });
      if (records?.length >= 2) return (opt.account = id);
    }
    return (opt.account = accounts[0].id);
  },
  realEstate: async () => (await invokeInPage('get_all_real_estate'))?.[0]?.id ?? null,
};

/**
 * Pages. `path`: string or async function. `ready`: JS predicate waited for (30 s).
 * `steps`: `run` = JS executed in the page, `until` = predicate waited for afterwards.
 * `fallback`: another page to capture when `when` holds on the first one.
 * `skip`: reason string when the page cannot be captured with these options.
 */
const PAGES = {
  dashboard: { path: '/', ready: all(loaded, charts()) },
  'bank-accounts': { path: '/bank-accounts', ready: all(loaded, rows()) },
  stocks: { path: '/stocks', ready: all(loaded, rows()) },
  // The language row holds the page's only combobox.
  settings: {
    path: '/settings/general',
    ready: all(loaded, `!!document.querySelector('main [role="combobox"]')`),
  },
  // The balance history hero and the monthly flows chart above the ledger.
  'account-detail': {
    path: async () => `/bank-accounts/${await ids.account()}`,
    ready: all(loaded, charts()),
  },
  import: {
    skip: !opt.fixture
      ? 'needs --fixture <csv>'
      : opt.lang !== 'en'
        ? 'the import flow is only captured in English (--lang en)'
        : null,
    // `?import=1` opens the dialog; the dev bridge takes the CSV path from localStorage
    // instead of the native file dialog (`moony-dev-csv-path`, CsvImportDialog).
    path: async () => `/bank-accounts/${await ids.account()}?import=1`,
    ready: all(loaded, `!!document.querySelector('[role="dialog"] .border-dashed button')`),
    steps: [
      {
        run: `localStorage.setItem('moony-dev-csv-path', ${JSON.stringify(opt.fixture)});
              document.querySelector('[role="dialog"] .border-dashed button').click();`,
        // Step 2 (columns) replaces the drop zone.
        until: `!!document.querySelector('[role="dialog"]') && !document.querySelector('[role="dialog"] .border-dashed')`,
      },
    ],
  },
  cashflow: { path: '/reports/cashflow', ready: all(loaded, charts(), rows()) },
  // Stats, the flow card and the plan sections: no chart, no table.
  planning: {
    path: '/reports/cashflow-planning',
    ready: all(loaded, `document.querySelectorAll('main h3').length >= 1`),
  },
  'stocks-analysis': { path: '/reports/stocks-analysis', ready: all(loaded, charts(), rows()) },
  crypto: { path: '/crypto', ready: all(loaded, rows()) },
  // The list; a list without any chart looks empty, so open the first property instead.
  'real-estate': {
    path: '/real-estate',
    ready: all(loaded, rows()),
    fallback: {
      when: `!(${charts()})`,
      path: async () => {
        const id = await ids.realEstate();
        return id ? `/real-estate/${id}` : null;
      },
      ready: loaded,
    },
  },
};
const SET_PAGES = {
  readme: ['dashboard', 'cashflow', 'account-detail', 'stocks-analysis', 'settings', 'import'],
  site: [
    'dashboard',
    'account-detail',
    'cashflow',
    'planning',
    'stocks',
    'stocks-analysis',
    'crypto',
    'real-estate',
    'settings',
  ],
};
SET_PAGES.all = [...new Set([...SET_PAGES.readme, ...SET_PAGES.site])];

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
  if (r.exceptionDetails) {
    const d = r.exceptionDetails;
    const detail = d.exception?.description ?? d.exception?.value ?? d.text ?? '';
    throw new Error(String(detail).slice(0, 300));
  }
  return r.result?.value;
}
invokeInPage = (cmd, cmdArgs = {}) =>
  evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(cmdArgs)})`);
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
async function goto(path) {
  await send('Page.navigate', { url: opt.base + path });
  await waitUntil('document.readyState === "complete"', 10000);
}

// The language the profile had before this run; `cleanup` puts it back.
let originalLanguage = null;
async function restoreLanguage() {
  if (originalLanguage === null) return;
  const language = originalLanguage;
  originalLanguage = null;
  try {
    const p = await invokeInPage('update_user_profile', { updates: { language } });
    console.log(`profile language restored to "${p.language}"`);
  } catch (e) {
    console.error(
      `Could not restore the profile language to "${language}" (${e.message}); set it back in Settings → General.`
    );
  }
}

// The theme the profile had before this run; `cleanup` puts it back.
let originalTheme = null;
async function restoreTheme() {
  if (originalTheme === null) return;
  const theme = originalTheme;
  originalTheme = null;
  try {
    const p = await invokeInPage('update_user_profile', { updates: { theme } });
    console.log(`profile theme restored to "${p.theme}"`);
  } catch (e) {
    console.error(
      `Could not restore the profile theme to "${theme}" (${e.message}); set it back in Settings → General.`
    );
  }
}

let cleanedUp = false;
async function cleanup() {
  if (cleanedUp) return;
  cleanedUp = true;
  await restoreLanguage();
  await restoreTheme();
  try {
    ws?.close();
  } catch {
    /* already closed */
  }
  chrome.kill('SIGKILL');
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 3 });
  } catch {
    /* Chrome may still hold files; it is a temp dir */
  }
}
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => void cleanup().finally(() => process.exit(130)));
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
  // Only the lock screen reads these keys; once unlocked, the profile's language and theme win
  // (step 2).
  await evaluate(
    `(() => { localStorage.setItem('moony-language', ${JSON.stringify(opt.lang)}); localStorage.setItem('moony-theme', ${JSON.stringify(opt.theme)}); return true; })()`
  );

  // 1. Whose data is this? Nothing is written or captured before the check passes.
  await goto('/');
  const named = await waitUntil(
    `/${EXPECTED_NAME}|${FORBIDDEN_NAME}/.test(document.body.innerText)`,
    30000
  );
  const text = await evaluate('document.body.innerText');
  if (text.includes(FORBIDDEN_NAME)) {
    throw new Error(
      `The page shows "${FORBIDDEN_NAME}": this is not the demo or anonymized profile (another session's real database?). Aborting.`
    );
  }
  if (!named || !text.includes(EXPECTED_NAME)) {
    const locked = await evaluate(`location.pathname.startsWith('/auth')`);
    throw new Error(
      locked
        ? 'The app is locked: unlock it (demo password in CONTRIBUTING.md) and run again.'
        : `The page does not show "${EXPECTED_NAME}": is ${opt.base} the demo profile? Aborting.`
    );
  }
  console.log(`profile ok ("${EXPECTED_NAME}"), language ${opt.lang}, theme ${opt.theme}`);

  // 2. The language and the theme of an unlocked app are the profile's: switch them, restore
  // them at the end.
  const current = await invokeInPage('get_user_profile');
  let switched = false;
  if (current?.language !== opt.lang) {
    originalLanguage = current.language;
    await invokeInPage('update_user_profile', { updates: { language: opt.lang } });
    console.log(`profile language "${originalLanguage}" -> "${opt.lang}" (restored at the end)`);
    switched = true;
  }
  if (current?.theme !== opt.theme) {
    originalTheme = current.theme;
    await invokeInPage('update_user_profile', { updates: { theme: opt.theme } });
    console.log(`profile theme "${originalTheme}" -> "${opt.theme}" (restored at the end)`);
    switched = true;
  }
  if (switched) await goto('/');
  if (!(await waitUntil(`document.documentElement.lang === ${JSON.stringify(opt.lang)}`, 15000))) {
    throw new Error(`The UI did not switch to "${opt.lang}".`);
  }
  // `system` follows the browser, so there is nothing to wait for; light and dark are stamped.
  if (
    opt.theme !== 'system' &&
    !(await waitUntil(
      `document.documentElement.dataset.theme === ${JSON.stringify(opt.theme)}`,
      15000
    ))
  ) {
    throw new Error(`The UI did not switch to the ${opt.theme} theme.`);
  }

  // 3. Capture.
  mkdirSync(opt.out, { recursive: true });
  for (const name of SET_PAGES[opt.set]) {
    const page = PAGES[name];
    if (page.skip) {
      console.log(`${name}: skipped, ${page.skip}`);
      continue;
    }
    try {
      await goto(typeof page.path === 'function' ? await page.path() : page.path);
      if (!(await waitUntil(page.ready, 30000)))
        console.warn(`${name}: readiness check timed out, capturing anyway`);
      if (page.fallback && (await evaluate(`!!(${page.fallback.when})`))) {
        const alt = await page.fallback.path();
        if (alt) {
          console.log(`${name}: the list looks empty, capturing ${alt} instead`);
          await goto(alt);
          if (!(await waitUntil(page.fallback.ready, 30000)))
            console.warn(`${name}: readiness check timed out, capturing anyway`);
        } else {
          console.warn(`${name}: the list looks empty and there is nothing to open`);
        }
      }
      for (const step of page.steps ?? []) {
        await evaluate(`(async () => { ${step.run} })()`);
        if (step.until && !(await waitUntil(step.until, 30000)))
          console.warn(`${name}: step did not finish, capturing anyway`);
        await sleep(1000);
      }
      // Charts animate in, and late data must not leave a skeleton in the shot.
      await sleep(2000);
      await waitUntil(`!document.querySelector('.skeleton')`, 10000);
      const shot = await send('Page.captureScreenshot', { format: 'png' });
      const file = join(opt.out, `${name}${opt.suffix}.png`);
      writeFileSync(file, Buffer.from(shot.data, 'base64'));
      console.log('wrote', file);
    } catch (e) {
      console.error(`${name}: failed (${e.message})`);
      process.exitCode = 1;
    }
  }
} catch (e) {
  console.error(e.message);
  process.exitCode = 1;
} finally {
  await cleanup();
}
