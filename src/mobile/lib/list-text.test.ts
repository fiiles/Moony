import { describe, expect, it } from 'vitest';
import { listText, sentences } from './list-text';

describe('listText', () => {
  it('joins a list as the language does (B9)', () => {
    expect(listText('cs-CZ', ['USD', 'EUR', 'GBP'])).toBe('USD, EUR, GBP');
    expect(listText('de-DE', ['Akcie', 'Krypto', 'Dluhopisy'])).toBe('Akcie, Krypto und Dluhopisy');
    expect(listText('ja-JP', ['株式', '暗号資産'])).toBe('株式、暗号資産');
    expect(listText('zh-CN', ['股票', '加密货币'])).toBe('股票、加密货币');
  });

  it('returns a single item or nothing as it is', () => {
    expect(listText('en-US', ['AAPL'])).toBe('AAPL');
    expect(listText('en-US', [])).toBe('');
  });
});

describe('sentences', () => {
  it('runs whole sentences on with a space, or without one in Japanese and Chinese (B9)', () => {
    expect(sentences('cs-CZ', ['První věta.', 'Druhá věta.'])).toBe('První věta. Druhá věta.');
    expect(sentences('ja-JP', ['最初の文。', '次の文。'])).toBe('最初の文。次の文。');
    expect(sentences('zh-CN', ['第一句。', '第二句。'])).toBe('第一句。第二句。');
  });
});
