import { describe, expect, it } from 'vitest';
import {
  HIDE_AMOUNTS_KEY,
  HIDE_AMOUNTS_ON_LAUNCH_KEY,
  MASK,
  hiddenMoneyOptions,
  maskMoney,
  readHideAmounts,
  readHideAmountsOnLaunch,
  writeHideAmounts,
  writeHideAmountsOnLaunch,
} from './hide-amounts';

/** A Map behind the Storage methods the helpers use. */
function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error('blocked');
  },
  setItem: (): void => {
    throw new Error('blocked');
  },
};

describe('hide-amounts state', () => {
  it('starts visible', () => {
    expect(readHideAmounts(memoryStorage())).toBe(false);
  });

  it('persists the toggle in both directions', () => {
    const storage = memoryStorage();
    writeHideAmounts(storage, true);
    expect(storage.getItem(HIDE_AMOUNTS_KEY)).toBe('1');
    expect(readHideAmounts(storage)).toBe(true);
    writeHideAmounts(storage, false);
    expect(storage.getItem(HIDE_AMOUNTS_KEY)).toBe('0');
    expect(readHideAmounts(storage)).toBe(false);
  });

  it('starts hidden when "hide on launch" is on, whatever was left last time', () => {
    const on = { [HIDE_AMOUNTS_ON_LAUNCH_KEY]: '1' };
    expect(readHideAmounts(memoryStorage(on))).toBe(true);
    expect(readHideAmounts(memoryStorage({ ...on, [HIDE_AMOUNTS_KEY]: '0' }))).toBe(true);
    expect(readHideAmounts(memoryStorage({ [HIDE_AMOUNTS_ON_LAUNCH_KEY]: '0' }))).toBe(false);
  });

  it('treats anything but "1" as off', () => {
    for (const raw of ['', 'true', '2', 'yes']) {
      expect(readHideAmounts(memoryStorage({ [HIDE_AMOUNTS_KEY]: raw }))).toBe(false);
      expect(readHideAmounts(memoryStorage({ [HIDE_AMOUNTS_ON_LAUNCH_KEY]: raw }))).toBe(false);
    }
  });

  it('copes with storage that is missing or blocked', () => {
    expect(readHideAmounts(null)).toBe(false);
    expect(readHideAmounts(undefined)).toBe(false);
    expect(readHideAmounts(throwing)).toBe(false);
    expect(() => writeHideAmounts(null, true)).not.toThrow();
    expect(() => writeHideAmounts(throwing, true)).not.toThrow();
  });
});

describe('hide amounts on launch', () => {
  it('is off until it is switched on', () => {
    expect(readHideAmountsOnLaunch(memoryStorage())).toBe(false);
  });

  it('persists the switch in both directions', () => {
    const storage = memoryStorage();
    writeHideAmountsOnLaunch(storage, true);
    expect(storage.getItem(HIDE_AMOUNTS_ON_LAUNCH_KEY)).toBe('1');
    expect(readHideAmountsOnLaunch(storage)).toBe(true);
    writeHideAmountsOnLaunch(storage, false);
    expect(storage.getItem(HIDE_AMOUNTS_ON_LAUNCH_KEY)).toBe('0');
    expect(readHideAmountsOnLaunch(storage)).toBe(false);
  });

  it('starts every launch hidden, whatever the eye was left at', () => {
    const storage = memoryStorage({ [HIDE_AMOUNTS_KEY]: '0' });
    writeHideAmountsOnLaunch(storage, true);
    expect(readHideAmounts(storage)).toBe(true);
    writeHideAmountsOnLaunch(storage, false);
    expect(readHideAmounts(storage)).toBe(false);
  });

  it('copes with storage that is missing or blocked', () => {
    expect(readHideAmountsOnLaunch(null)).toBe(false);
    expect(readHideAmountsOnLaunch(throwing)).toBe(false);
    expect(() => writeHideAmountsOnLaunch(null, true)).not.toThrow();
    expect(() => writeHideAmountsOnLaunch(throwing, true)).not.toThrow();
  });
});

describe('maskMoney', () => {
  it('replaces the number and keeps the unit', () => {
    expect(maskMoney('4 182 560 Kč')).toBe('•••• Kč');
    expect(maskMoney('+ 1 249,50 €')).toBe('+ •••• €');
    expect(maskMoney('1 240 €')).toBe('•••• €');
  });

  it('treats a group of digits with any separator as one number', () => {
    // no-break space (cs), narrow no-break space (fr/nb style), dot and comma (de, en), apostrophe (ch)
    expect(maskMoney('4 182 560 Kč')).toBe('•••• Kč');
    expect(maskMoney('1 240,50 €')).toBe('•••• €');
    expect(maskMoney('1.249,50 €')).toBe('•••• €');
    expect(maskMoney('$1,249.50')).toBe('$••••');
    expect(maskMoney('CHF 1’240.50')).toBe('CHF ••••');
  });

  it('keeps signs, symbols and the code of the currency', () => {
    expect(maskMoney('−4 200 Kč')).toBe('−•••• Kč');
    expect(maskMoney('-12,50 USD')).toBe('-•••• USD');
    expect(maskMoney('€ 12')).toBe('€ ••••');
    expect(maskMoney('¥1,200')).toBe('¥••••');
  });

  it('keeps the words around the number, even those of a compact amount', () => {
    // which is why the phone formats an amount in full while it is hidden (`hiddenMoneyOptions`)
    expect(maskMoney('6,78 mil. Kč')).toBe('•••• mil. Kč');
    expect(maskMoney('+ 1,15 mil. Kč')).toBe('+ •••• mil. Kč');
    expect(maskMoney('$1.2M')).toBe('$••••M');
  });

  it('masks every number of a string that holds several', () => {
    expect(maskMoney('4 200 Kč → 5 100 Kč')).toBe('•••• Kč → •••• Kč');
  });

  it('leaves text without digits alone', () => {
    expect(maskMoney('—')).toBe('—');
    expect(maskMoney('')).toBe('');
    expect(maskMoney('beze změny')).toBe('beze změny');
  });

  it('formats in full while hidden, so "mil." and "tis." never give the size away', () => {
    expect(hiddenMoneyOptions({ compact: true, decimals: 2 })).toEqual({
      compact: false,
      decimals: 2,
    });
    expect(hiddenMoneyOptions({ signed: true, decimals: 0 })).toEqual({
      signed: true,
      decimals: 0,
      compact: false,
    });
    expect(hiddenMoneyOptions(undefined)).toEqual({ compact: false });
  });

  it('exports the mask a chart axis uses', () => {
    expect(MASK).toBe('••••');
  });
});
