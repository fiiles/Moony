import { describe, expect, it } from 'vitest';
import { coinIconKey, resolveCoinIcon } from './coin-icons';

describe('coinIconKey', () => {
  it('lower-cases and trims a ticker', () => {
    expect(coinIconKey('BTC')).toBe('btc');
    expect(coinIconKey(' eth ')).toBe('eth');
  });

  it('rejects tickers with characters outside [a-z0-9]', () => {
    expect(coinIconKey('BTC-USD')).toBeNull();
    expect(coinIconKey('a/b')).toBeNull();
  });

  it('rejects an empty ticker', () => {
    expect(coinIconKey('')).toBeNull();
    expect(coinIconKey('   ')).toBeNull();
  });
});

describe('resolveCoinIcon', () => {
  const map = { btc: async () => 'data:image/svg+xml;base64,QQ==' };

  it('resolves a known ticker to its URL', async () => {
    expect(await resolveCoinIcon(map, 'BTC')).toBe('data:image/svg+xml;base64,QQ==');
  });

  it('resolves an unknown ticker to null', async () => {
    expect(await resolveCoinIcon(map, 'NOPE')).toBeNull();
    expect(await resolveCoinIcon(map, 'BTC-USD')).toBeNull();
  });

  it('does not match inherited object properties', async () => {
    expect(await resolveCoinIcon(map, 'constructor')).toBeNull();
  });

  it('resolves to null when the loader fails', async () => {
    const failing = {
      eth: async () => {
        throw new Error('chunk failed');
      },
    };
    expect(await resolveCoinIcon(failing, 'eth')).toBeNull();
  });
});
