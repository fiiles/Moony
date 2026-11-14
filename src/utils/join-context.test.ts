import { describe, expect, it } from 'vitest';
import { joinPassword, joinPresentation } from './join-context';

describe('joinPresentation', () => {
  it('asks for the password and shows the replace warning and the impact in Settings', () => {
    expect(joinPresentation({ kind: 'settings' })).toEqual({
      askPassword: true,
      showReplaceWarning: true,
      showImpact: true,
      cancelKey: 'buttons.cancel',
    });
  });

  it('asks nothing again and warns of nothing in the first run, whose way out is Back', () => {
    expect(joinPresentation({ kind: 'firstRun', password: 'pw' })).toEqual({
      askPassword: false,
      showReplaceWarning: false,
      showImpact: false,
      cancelKey: 'buttons.back',
    });
  });
});

describe('joinPassword', () => {
  it('sends the typed password in Settings', () => {
    expect(joinPassword({ kind: 'settings' }, 'typed')).toBe('typed');
  });

  it("sends the first run's password, whatever was typed", () => {
    expect(joinPassword({ kind: 'firstRun', password: 'pw' }, '')).toBe('pw');
  });
});
