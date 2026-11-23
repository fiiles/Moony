import { describe, expect, it } from 'vitest';
import { dayRelation, firstRunScreen, type FirstRunInput } from './demo-profile';

const base: FirstRunInput = {
  desktop: true,
  introDone: false,
  demoExists: false,
  wizardEntry: null,
};

describe('firstRunScreen', () => {
  it('plays the intro for a new person on the desktop, then shows the choice', () => {
    expect(firstRunScreen(base)).toBe('intro');
    expect(firstRunScreen({ ...base, introDone: true })).toBe('choice');
  });

  it('starts a returning person (a stored demo) on the choice screen without the intro', () => {
    expect(firstRunScreen({ ...base, demoExists: true })).toBe('choice');
  });

  it('shows the wizard once it was entered from the choice screen', () => {
    expect(
      firstRunScreen({ ...base, introDone: true, wizardEntry: { from: 'choice', path: 'new' } })
    ).toBe('wizard');
    expect(
      firstRunScreen({ ...base, introDone: true, wizardEntry: { from: 'choice', path: 'join' } })
    ).toBe('wizard');
  });

  it('opens the wizard straight from the demo, with no intro and no choice', () => {
    expect(
      firstRunScreen({ ...base, demoExists: true, wizardEntry: { from: 'demo', path: 'new' } })
    ).toBe('wizard');
  });

  it('returns to the choice screen when the wizard is left', () => {
    expect(firstRunScreen({ ...base, introDone: true, wizardEntry: null })).toBe('choice');
  });

  it('keeps the phone on the intro and the wizard, whatever the demo state', () => {
    const phone = { ...base, desktop: false };
    expect(firstRunScreen(phone)).toBe('intro');
    expect(firstRunScreen({ ...phone, demoExists: true })).toBe('intro');
    expect(firstRunScreen({ ...phone, introDone: true })).toBe('wizard');
  });
});

describe('dayRelation', () => {
  const now = new Date(2026, 10, 23, 10, 30);
  const at = (d: Date) => Math.floor(d.getTime() / 1000);

  it('reads an instant earlier today as today', () => {
    expect(dayRelation(at(new Date(2026, 10, 23, 0, 5)), now)).toBe('today');
  });

  it('reads the previous calendar day as yesterday, also late in the evening', () => {
    expect(dayRelation(at(new Date(2026, 10, 22, 21, 14)), now)).toBe('yesterday');
    expect(dayRelation(at(new Date(2026, 10, 22, 0, 0)), now)).toBe('yesterday');
  });

  it('reads anything older as earlier', () => {
    expect(dayRelation(at(new Date(2026, 10, 21, 23, 59)), now)).toBe('earlier');
  });

  it('crosses a month boundary', () => {
    const first = new Date(2026, 11, 1, 8, 0);
    expect(dayRelation(at(new Date(2026, 10, 30, 20, 0)), first)).toBe('yesterday');
  });
});
