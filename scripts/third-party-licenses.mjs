#!/usr/bin/env node
/**
 * Generates THIRD_PARTY_LICENSES.md: an inventory of the licences of everything Moony
 * is built from.
 *
 *   npm run licenses                      # writes ./THIRD_PARTY_LICENSES.md
 *   node scripts/third-party-licenses.mjs --out /tmp/licenses.md
 *   node scripts/third-party-licenses.mjs --no-cargo   # never invoke cargo
 *
 * npm: the production dependency closure, read from package.json and the
 * package.json of every installed package in node_modules (run `npm ci` first).
 *
 * Rust: `cargo metadata` when cargo is on PATH. Otherwise (or with --no-cargo, or when
 * cargo fails) the crate list comes from src-tauri/Cargo.lock and each crate's licence
 * from its Cargo.toml in the local cargo registry (unpacked sources or the downloaded
 * .crate archive). Crates that cannot be resolved are listed as UNKNOWN.
 *
 * The output is deterministic (no timestamps), so a re-run only changes the file when
 * the dependency set really changed. No dependencies other than Node itself.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MANIFEST = join(ROOT, 'src-tauri', 'Cargo.toml');
const LOCKFILE = join(ROOT, 'src-tauri', 'Cargo.lock');

const args = process.argv.slice(2);
const noCargo = args.includes('--no-cargo');
const outIndex = args.indexOf('--out');
const OUT = outIndex >= 0 ? resolve(args[outIndex + 1] ?? '') : join(ROOT, 'THIRD_PARTY_LICENSES.md');
if (outIndex >= 0 && !args[outIndex + 1]) {
  console.error('--out needs a file path');
  process.exit(2);
}

const warnings = [];

// ---------------------------------------------------------------------------
// helpers

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
}

/** Collapse whitespace; Cargo's legacy "MIT/Apache-2.0" becomes "MIT OR Apache-2.0". */
function normalizeLicense(value) {
  const text = String(value ?? '').replace(/\s+/g, ' ').trim();
  if (!text) return 'UNKNOWN';
  return /\b(OR|AND|WITH)\b/.test(text) ? text : text.split(/\s*\/\s*/).join(' OR ');
}

function compareEntries(a, b) {
  const byName = a.name.localeCompare(b.name, 'en', { sensitivity: 'base' });
  if (byName !== 0) return byName;
  return a.version.localeCompare(b.version, 'en', { numeric: true });
}

function tool(command, commandArgs, options = {}) {
  return execFileSync(command, commandArgs, {
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
  });
}

function commandWorks(command, versionArgs = ['--version']) {
  try {
    tool(command, versionArgs);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// npm

function npmLicense(pkg) {
  const { license, licenses } = pkg;
  if (typeof license === 'string' && license.trim()) return normalizeLicense(license);
  if (license && typeof license === 'object' && typeof license.type === 'string') {
    return normalizeLicense(license.type);
  }
  if (Array.isArray(licenses) && licenses.length > 0) {
    const types = licenses.map((l) => (typeof l === 'string' ? l : l?.type)).filter(Boolean);
    if (types.length > 0) return normalizeLicense(types.join(' OR '));
  }
  return 'UNKNOWN';
}

/** Node-style lookup of node_modules/<name>, starting in `fromDir` and walking up. */
function findInstalledPackage(name, fromDir) {
  let dir = fromDir;
  for (;;) {
    const candidate = join(dir, 'node_modules', name);
    if (existsSync(join(candidate, 'package.json'))) return realpathSync(candidate);
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

function collectNpm() {
  const rootPkg = readJson(join(ROOT, 'package.json'));
  if (!rootPkg) throw new Error('package.json not found');
  const lockfile = readJson(join(ROOT, 'package-lock.json'));
  const found = new Map();
  const visited = new Set();

  const visit = (name, fromDir, optional) => {
    const dir = findInstalledPackage(name, fromDir);
    if (!dir) {
      if (!optional) {
        const locked = lockfile?.packages?.[`node_modules/${name}`];
        warnings.push(`npm: ${name} is not installed (run "npm ci"); taken from package-lock.json`);
        const version = locked?.version ?? '?';
        found.set(`${name}@${version}`, {
          name,
          version,
          license: locked ? npmLicense(locked) : 'UNKNOWN',
        });
      }
      return;
    }
    if (visited.has(dir)) return;
    visited.add(dir);
    const pkg = readJson(join(dir, 'package.json'));
    if (!pkg) return;
    const version = pkg.version ?? '?';
    found.set(`${pkg.name ?? name}@${version}`, {
      name: pkg.name ?? name,
      version,
      license: npmLicense(pkg),
    });
    for (const dep of Object.keys(pkg.dependencies ?? {})) visit(dep, dir, false);
    for (const dep of Object.keys(pkg.optionalDependencies ?? {})) visit(dep, dir, true);
  };

  for (const dep of Object.keys(rootPkg.dependencies ?? {})) visit(dep, ROOT, false);
  return [...found.values()].sort(compareEntries);
}

// ---------------------------------------------------------------------------
// Rust

function packageTableLicense(manifestText) {
  const lines = manifestText.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === '[package]');
  if (start < 0) return 'UNKNOWN';
  let licenseFile = false;
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*\[/.test(line)) break;
    const license = line.match(/^\s*license\s*=\s*(?:"([^"]*)"|'([^']*)')/);
    if (license) return normalizeLicense(license[1] ?? license[2]);
    if (/^\s*license-file\s*=/.test(line)) licenseFile = true;
  }
  return licenseFile ? 'UNKNOWN (license file only)' : 'UNKNOWN';
}

function cargoMetadataCrates() {
  const raw = tool('cargo', ['metadata', '--format-version', '1', '--locked', '--manifest-path', MANIFEST]);
  const { packages } = JSON.parse(raw);
  return packages
    .filter((p) => p.source) // path and workspace members (Moony itself) have no source
    .map((p) => ({
      name: p.name,
      version: p.version,
      license: p.license
        ? normalizeLicense(p.license)
        : p.license_file
          ? 'UNKNOWN (license file only)'
          : 'UNKNOWN',
    }));
}

function lockfileCrates() {
  const lock = readFileSync(LOCKFILE, 'utf8');
  const crates = [];
  for (const block of lock.split('[[package]]').slice(1)) {
    const name = block.match(/^name = "([^"]+)"/m)?.[1];
    const version = block.match(/^version = "([^"]+)"/m)?.[1];
    const source = block.match(/^source = "([^"]+)"/m)?.[1];
    if (name && version && source) crates.push({ name, version, source });
  }
  return crates;
}

function registryRoots(kind) {
  const base = join(process.env.CARGO_HOME ?? join(homedir(), '.cargo'), 'registry', kind);
  if (!existsSync(base)) return [];
  return readdirSync(base).map((entry) => join(base, entry));
}

/** Cargo.toml text of a registry crate from the local cargo cache, or null. */
function localManifest(name, version) {
  const id = `${name}-${version}`;
  for (const dir of registryRoots('src')) {
    const manifest = join(dir, id, 'Cargo.toml');
    if (existsSync(manifest)) return readFileSync(manifest, 'utf8');
  }
  for (const dir of registryRoots('cache')) {
    const archive = join(dir, `${id}.crate`);
    if (!existsSync(archive)) continue;
    try {
      return tool('tar', ['-xzOf', archive, `${id}/Cargo.toml`]);
    } catch {
      // try the next candidate
    }
  }
  return null;
}

function collectRust() {
  if (!noCargo && commandWorks('cargo')) {
    try {
      const crates = cargoMetadataCrates().sort(compareEntries);
      console.log(`Rust: ${crates.length} crates via cargo metadata`);
      return crates;
    } catch (error) {
      warnings.push(
        `cargo metadata failed (${String(error.message).split('\n')[0]}); falling back to Cargo.lock`
      );
    }
  }
  const crates = lockfileCrates().map(({ name, version, source }) => {
    const manifest = source.startsWith('registry+') ? localManifest(name, version) : null;
    return { name, version, license: manifest ? packageTableLicense(manifest) : 'UNKNOWN' };
  });
  const unresolved = crates.filter((c) => c.license === 'UNKNOWN').length;
  console.log(
    `Rust: ${crates.length} crates from Cargo.lock + local cargo registry (${unresolved} unresolved)`
  );
  if (unresolved > 0) {
    warnings.push(
      `${unresolved} crate(s) have no licence in the local cargo registry; run "cargo fetch" or ` +
        'regenerate with cargo available'
    );
  }
  return crates.sort(compareEntries);
}

// ---------------------------------------------------------------------------
// rendering

const PERMISSIVE = new Set(
  [
    '0BSD',
    'Apache-2.0',
    'BlueOak-1.0.0',
    'BSD-2-Clause',
    'BSD-3-Clause',
    'BSL-1.0',
    'CC-BY-4.0',
    'CC0-1.0',
    'CDLA-Permissive-2.0',
    'ISC',
    'MIT',
    'MIT-0',
    'NCSA',
    'OpenSSL',
    'Python-2.0',
    'Unicode-3.0',
    'Unicode-DFS-2016',
    'Unlicense',
    'WTFPL',
    'Zlib',
  ].map((id) => id.toLowerCase())
);

/**
 * Evaluate an SPDX expression against the permissive list. Returns the identifiers that
 * stand in the way (empty when the expression can be satisfied with permissive licences
 * only). `OR` needs one satisfiable alternative, `AND` needs all of them.
 */
function blockers(expression) {
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  let pos = 0;
  const parseOr = () => {
    const parts = [parseAnd()];
    while (tokens[pos]?.toUpperCase() === 'OR') {
      pos++;
      parts.push(parseAnd());
    }
    return parts.some((part) => part.length === 0) ? [] : parts.flat();
  };
  const parseAnd = () => {
    const parts = [parseTerm()];
    while (tokens[pos]?.toUpperCase() === 'AND') {
      pos++;
      parts.push(parseTerm());
    }
    return parts.flat();
  };
  const parseTerm = () => {
    const token = tokens[pos++];
    if (token === '(') {
      const inner = parseOr();
      if (tokens[pos] === ')') pos++;
      return inner;
    }
    if (tokens[pos]?.toUpperCase() === 'WITH') pos += 2; // licence exception
    return token && PERMISSIVE.has(token.toLowerCase()) ? [] : [token ?? 'UNKNOWN'];
  };
  return [...new Set(parseOr())];
}

/** Why an entry needs a human look, or null when permissive terms are enough. */
function needsReview(license) {
  if (license.startsWith('UNKNOWN')) return 'licence not declared';
  const offenders = blockers(license);
  return offenders.length === 0 ? null : `needs ${offenders.join(', ')}`;
}

const esc = (text) => String(text).replace(/\|/g, '\\|');

function countByLicense(entries) {
  const counts = new Map();
  for (const { license } of entries) counts.set(license, (counts.get(license) ?? 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], 'en'));
}

function summaryTable(entries) {
  const rows = countByLicense(entries).map(([license, count]) => `| ${esc(license)} | ${count} |`);
  return ['| Licence | Packages |', '| --- | ---: |', ...rows].join('\n');
}

function packageTable(entries) {
  const rows = entries.map((e) => `| ${esc(e.name)} | ${esc(e.version)} | ${esc(e.license)} |`);
  return ['| Package | Version | Licence |', '| --- | --- | --- |', ...rows].join('\n');
}

function reviewList(label, entries) {
  const flagged = entries
    .map((e) => ({ ...e, reason: needsReview(e.license) }))
    .filter((e) => e.reason);
  if (flagged.length === 0) return `- ${label}: none.`;
  return [
    `- ${label}:`,
    ...flagged.map((e) => `  - \`${e.name}\` ${e.version}: ${esc(e.license)} (${e.reason})`),
  ].join('\n');
}

function render(npm, rust) {
  return `# Third-party licences

Moony is released under the [GNU Affero General Public License v3.0](./LICENSE). It is built
from the open-source components inventoried below.

This file is **generated** by \`npm run licenses\` (\`scripts/third-party-licenses.mjs\`) — do not
edit it by hand; regenerate it whenever dependencies change. It lists the licence each package
declares (SPDX identifiers). The full texts and copyright notices are in the packages
themselves: \`node_modules/<package>/\` for npm and the crate sources for Rust. Other bundled
components not covered by the package managers: SQLCipher (BSD-3-Clause, compiled into the
\`libsqlite3-sys\` crate) and, in the Windows build, a statically linked OpenSSL (Apache-2.0).

- **npm**: the production dependency closure of \`package.json\` (dependencies, optional
  dependencies and everything they pull in); development tooling is not listed.
- **Rust**: every crate in \`src-tauri/Cargo.lock\`. That is a superset of what ships in any
  single build, because it includes build-time, test-time and other-platform crates.

## Summary

| Ecosystem | Packages |
| --- | ---: |
| npm | ${npm.length} |
| Rust | ${rust.length} |

### npm — packages per licence

${summaryTable(npm)}

### Rust — crates per licence

${summaryTable(rust)}

## Needs review

Entries without a declared licence, or where no alternative is a plainly permissive licence
(for example weak copyleft such as MPL-2.0, which applies to modifications of those files).

${reviewList('npm', npm)}
${reviewList('Rust', rust)}

## npm packages

${packageTable(npm)}

## Rust crates

${packageTable(rust)}
`;
}

// ---------------------------------------------------------------------------

const npmPackages = collectNpm();
console.log(`npm: ${npmPackages.length} production packages`);
const rustCrates = collectRust();

writeFileSync(OUT, render(npmPackages, rustCrates));
console.log(`Wrote ${OUT}`);
for (const warning of warnings) console.warn(`warning: ${warning}`);
