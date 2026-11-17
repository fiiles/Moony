import { describe, expect, it } from 'vitest';
import { intentState, readIntent, withIntent, withoutIntent } from './row-intent';

describe('intentState', () => {
  it('carries the intent beside the back label', () => {
    expect(readIntent(intentState({ moonyBack: 'Finance' }, 'record'))).toBe('record');
    expect(intentState({ moonyBack: 'Finance' }, 'record')).toMatchObject({
      moonyBack: 'Finance',
    });
  });

  it('carries the intent without a back label', () => {
    expect(readIntent(intentState(undefined, 'buy'))).toBe('buy');
  });
});

describe('readIntent', () => {
  it('is null for no state, a foreign state or an unknown intent', () => {
    expect(readIntent(null)).toBeNull();
    expect(readIntent(undefined)).toBeNull();
    expect(readIntent('buy')).toBeNull();
    expect(readIntent({ moonyBack: 'Finance' })).toBeNull();
    expect(readIntent({ moonyIntent: 'launch' })).toBeNull();
    expect(readIntent({ moonyIntent: 3 })).toBeNull();
  });
});

describe('withoutIntent', () => {
  it('drops the intent and keeps every other key', () => {
    const state = { ...intentState({ moonyBack: 'Přehled' }, 'sell'), moonySheet: 2 };
    expect(withoutIntent(state)).toEqual({ moonyBack: 'Přehled', moonySheet: 2 });
  });

  it('leaves a state without an object as it is', () => {
    expect(withoutIntent(null)).toBeNull();
    expect(withoutIntent(undefined)).toBeUndefined();
  });
});

describe('withIntent', () => {
  it('adds the intent and keeps every other key of the entry', () => {
    const state = withIntent({ moonyBack: 'Přehled', moonySheet: '2' }, 'join');
    expect(readIntent(state)).toBe('join');
    expect(withoutIntent(state)).toEqual({ moonyBack: 'Přehled', moonySheet: '2' });
  });

  it('starts from nothing when the entry has no state object', () => {
    expect(withIntent(null, 'join')).toEqual(intentState(undefined, 'join'));
    expect(withIntent('x', 'join')).toEqual(intentState(undefined, 'join'));
  });
});
