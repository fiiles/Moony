// Bundled coin icons (@web3icons/core, branded token set, MIT). Only the branded token
// modules are globbed, each as its own lazy chunk, so none of the other variants or
// icon types reach dist/. The package ships each icon as a JS module whose default export
// is the SVG markup (there are no .svg files), so the loader turns it into a data URL
// (allowed by the CSP: img-src data:).

export type CoinIconLoaders = Readonly<Record<string, () => Promise<string>>>;

const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();

/** Lower-cased ticker → the key used in the glob map; null when the ticker has characters outside [a-z0-9]. */
export function coinIconKey(ticker: string): string | null {
  const key = ticker.trim().toLowerCase();
  return /^[a-z0-9]+$/.test(key) ? key : null;
}

/** Resolves a ticker against a loader map; null for an unknown ticker or a failed load. */
export async function resolveCoinIcon(
  loaders: CoinIconLoaders,
  ticker: string
): Promise<string | null> {
  const key = coinIconKey(ticker);
  if (key === null || !Object.hasOwn(loaders, key)) return null;
  try {
    return await loaders[key]();
  } catch {
    return null;
  }
}

function svgToDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

let bundled: CoinIconLoaders | undefined;

function bundledLoaders(): CoinIconLoaders {
  if (bundled) return bundled;
  const modules = import.meta.glob<string>(
    '../../node_modules/@web3icons/core/dist/svgs/tokens/branded/*.svg.js',
    { import: 'default' }
  );
  const loaders: Record<string, () => Promise<string>> = {};
  for (const [path, load] of Object.entries(modules)) {
    const name = path.slice(path.lastIndexOf('/') + 1).replace(/\.svg\.js$/, '');
    const key = coinIconKey(name);
    if (key !== null) loaders[key] = async () => svgToDataUrl(await load());
  }
  bundled = loaders;
  return loaders;
}

/** Resolves to the bundled icon's URL, or null when the set has no icon for the ticker. Cached per ticker. */
export function loadCoinIcon(ticker: string): Promise<string | null> {
  const key = coinIconKey(ticker);
  if (key === null) return Promise.resolve(null);
  const hit = cache.get(key);
  if (hit !== undefined) return Promise.resolve(hit);
  let pending = inflight.get(key);
  if (!pending) {
    pending = resolveCoinIcon(bundledLoaders(), key).then((url) => {
      cache.set(key, url);
      inflight.delete(key);
      return url;
    });
    inflight.set(key, pending);
  }
  return pending;
}

/** Synchronous read of an already loaded icon (undefined = not loaded yet). */
export function peekCoinIcon(ticker: string): string | null | undefined {
  const key = coinIconKey(ticker);
  return key === null ? null : cache.get(key);
}
