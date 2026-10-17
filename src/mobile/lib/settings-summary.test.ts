import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';
import { lockAfterOption, renderTextRef, securitySummary } from './settings-summary';

describe('securitySummary', () => {
  it('names the biometry and the lock delay', () => {
    expect(
      securitySummary({
        enrolled: true,
        biometryType: 'faceId',
        lockMinutes: 1,
        hideOnLaunch: false,
      })
    ).toEqual([
      { key: 'settings.security.biometricsOn', params: { method: 'settings.security.methodFace' } },
      { key: 'settings.security.lockAfter', params: { count: 1 } },
    ]);
  });

  it('says Ihned for zero minutes and adds the hide-on-launch part only when on', () => {
    expect(
      securitySummary({ enrolled: false, biometryType: 'none', lockMinutes: 0, hideOnLaunch: true })
    ).toEqual([
      { key: 'settings.security.biometricsOff' },
      { key: 'settings.security.lockNow' },
      { key: 'settings.security.hideOnLaunchShort' },
    ]);
  });

  it('picks the method wording by the kind of biometry', () => {
    const method = (biometryType: 'touchId' | 'biometric') =>
      securitySummary({ enrolled: true, biometryType, lockMinutes: 5, hideOnLaunch: false })[0];
    expect(method('touchId').params).toEqual({ method: 'settings.security.methodTouch' });
    expect(method('biometric').params).toEqual({ method: 'settings.security.methodGeneric' });
  });
});

describe('lockAfterOption', () => {
  it('labels zero minutes Ihned and the rest by their count', () => {
    expect(lockAfterOption(0)).toEqual({ key: 'settings.security.optionNow' });
    expect(lockAfterOption(15)).toEqual({
      key: 'settings.security.optionMinutes',
      params: { count: 15 },
    });
  });
});

describe('renderTextRef', () => {
  // A translator that wraps what it translated in «», so the test sees which strings it was given
  const t = ((key: string, params?: Record<string, string | number>) =>
    params
      ? `«${key}|${Object.entries(params)
          .map(([name, value]) => `${name}=${value}`)
          .join(',')}»`
      : `«${key}»`) as unknown as TFunction;

  it('translates a plain reference', () => {
    expect(renderTextRef(t, { key: 'settings.security.lockNow' })).toBe(
      '«settings.security.lockNow»'
    );
  });

  it('hands number params over as they are', () => {
    expect(renderTextRef(t, lockAfterOption(5))).toBe('«settings.security.optionMinutes|count=5»');
  });

  it('translates the method param, which is itself a key', () => {
    expect(
      renderTextRef(t, {
        key: 'settings.security.biometricsOn',
        params: { method: 'settings.security.methodFace' },
      })
    ).toBe('«settings.security.biometricsOn|method=«settings.security.methodFace»»');
  });
});
