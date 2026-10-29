import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';
import { lockAfterOption, renderTextRef, securitySummary } from './settings-summary';

describe('securitySummary', () => {
  it('names the biometry and the lock delay', () => {
    expect(securitySummary({ enrolled: true, biometryType: 'faceId', lockMinutes: 1 })).toEqual([
      {
        key: 'settings.security.biometricsOn',
        params: { methodKey: 'settings.security.methodFace' },
      },
      { key: 'settings.security.lockAfter', params: { count: 1 } },
    ]);
  });

  it('says Ihned for zero minutes', () => {
    expect(securitySummary({ enrolled: false, biometryType: 'none', lockMinutes: 0 })).toEqual([
      { key: 'settings.security.biometricsOff' },
      { key: 'settings.security.lockNow' },
    ]);
  });

  it('picks the method wording by the kind of biometry', () => {
    const method = (biometryType: 'touchId' | 'biometric') =>
      securitySummary({ enrolled: true, biometryType, lockMinutes: 5 })[0];
    expect(method('touchId').params).toEqual({ methodKey: 'settings.security.methodTouch' });
    expect(method('biometric').params).toEqual({
      methodKey: 'settings.security.methodGeneric',
    });
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

  it('translates a param whose name ends in Key, which is itself a key', () => {
    expect(
      renderTextRef(t, {
        key: 'settings.security.biometricsOn',
        params: { methodKey: 'settings.security.methodFace' },
      })
    ).toBe('«settings.security.biometricsOn|methodKey=«settings.security.methodFace»»');
  });

  it('applies the same rule to any such param and leaves the others alone', () => {
    expect(
      renderTextRef(t, {
        key: 'notifications.deadline.in14',
        params: { whatKey: 'notifications.deadline.kind.loan_fixation_end', title: 'Key' },
      })
    ).toBe(
      '«notifications.deadline.in14|whatKey=«notifications.deadline.kind.loan_fixation_end»,title=Key»'
    );
  });
});
