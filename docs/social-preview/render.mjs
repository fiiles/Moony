// Run from any directory: node docs/social-preview/render.mjs
// Requires Google Chrome (or CHROME_PATH); no npm dependencies or app server.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const chrome = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
].find((path) => path && existsSync(path));
if (!chrome) throw new Error('Set CHROME_PATH to your Chrome or Chromium executable.');

const output = fileURLToPath(new URL('./moony-social-preview-html.png', import.meta.url));
const source = new URL('./moony-social-preview.html', import.meta.url).href;
const profile = mkdtempSync(join(tmpdir(), 'moony-social-preview-'));
const capture = join(profile, 'capture.png');
try {
  const child = spawn(chrome, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    '--hide-scrollbars',
    '--force-device-scale-factor=1',
    '--window-size=1280,640',
    '--virtual-time-budget=2000',
    `--user-data-dir=${profile}`,
    `--screenshot=${capture}`,
    source,
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let diagnostic = '';
  child.stderr.on('data', (chunk) => { diagnostic += chunk; });
  await new Promise((resolve, reject) => {
    let captured = false;
    // Chrome can remain open after capturing. Close only our temporary process
    // once the PNG's final IEND chunk confirms the image is completely written.
    const poll = setInterval(() => {
      if (!existsSync(capture) || captured) return;
      const png = readFileSync(capture);
      if (png.length < 24 || png.toString('ascii', png.length - 8, png.length - 4) !== 'IEND') return;
      if (png.readUInt32BE(16) !== 1280 || png.readUInt32BE(20) !== 640) return;
      captured = true;
      writeFileSync(output, png);
      child.kill('SIGTERM');
    }, 100);
    const timeout = setTimeout(() => { child.kill('SIGKILL'); }, 30000);
    const cleanup = () => { clearInterval(poll); clearTimeout(timeout); };
    child.once('error', (error) => { cleanup(); reject(error); });
    child.once('exit', (code, signal) => {
      cleanup();
      if (captured) resolve();
      else reject(new Error(`Chrome exited (${signal || code}): ${diagnostic}`));
    });
  });
  console.log(output);
} finally {
  rmSync(profile, { recursive: true, force: true, maxRetries: 3 });
}
