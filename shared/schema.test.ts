import { describe, it, expect } from 'vitest';
import { MIN_PASSWORD_LENGTH, setupSchema, recoverSchema, changePasswordSchema } from './schema';

const firstMessage = (result: { success: boolean; error?: { issues: { message: string }[] } }) =>
  result.error?.issues[0]?.message;

describe('MIN_PASSWORD_LENGTH', () => {
  it('is 8', () => {
    expect(MIN_PASSWORD_LENGTH).toBe(8);
  });
});

describe('password policy is identical for setup, recover and change password', () => {
  const short = 'a'.repeat(MIN_PASSWORD_LENGTH - 1);
  const ok = 'a'.repeat(MIN_PASSWORD_LENGTH);

  const setup = (password: string) =>
    setupSchema.safeParse({
      name: 'Jan',
      surname: 'Novak',
      email: '',
      password,
      confirmPassword: password,
    });
  const recover = (newPassword: string) =>
    recoverSchema.safeParse({
      recoveryKey: 'ABCD-EFGH-JKLM-NPQR-STUV-WXYZ',
      newPassword,
      confirmPassword: newPassword,
    });
  const change = (newPassword: string) =>
    changePasswordSchema.safeParse({
      currentPassword: 'current-password',
      newPassword,
      confirmPassword: newPassword,
    });

  it.each([
    ['setup', setup],
    ['recover', recover],
    ['change password', change],
  ])('%s rejects a password shorter than the minimum', (_name, parse) => {
    const result = parse(short);
    expect(result.success).toBe(false);
    expect(firstMessage(result)).toBe('validation.passwordMinLength');
  });

  it.each([
    ['setup', setup],
    ['recover', recover],
    ['change password', change],
  ])('%s accepts a password of exactly the minimum length', (_name, parse) => {
    expect(parse(ok).success).toBe(true);
  });

  it('change password no longer accepts the old 6-character minimum', () => {
    expect(change('abcdef').success).toBe(false);
  });

  it('change password still reports mismatching confirmation', () => {
    const result = changePasswordSchema.safeParse({
      currentPassword: 'current-password',
      newPassword: ok,
      confirmPassword: ok + 'x',
    });
    expect(result.success).toBe(false);
    expect(firstMessage(result)).toBe('validation.passwordMismatch');
  });

  it('change password requires the current password', () => {
    const result = changePasswordSchema.safeParse({
      currentPassword: '',
      newPassword: ok,
      confirmPassword: ok,
    });
    expect(result.success).toBe(false);
    expect(firstMessage(result)).toBe('validation.currentPasswordRequired');
  });
});

describe('recoverSchema recovery key input', () => {
  it('trims surrounding whitespace pasted with the key', () => {
    const result = recoverSchema.safeParse({
      recoveryKey: '  ABCD-EFGH-JKLM-NPQR-STUV-WXYZ \n',
      newPassword: 'a'.repeat(MIN_PASSWORD_LENGTH),
      confirmPassword: 'a'.repeat(MIN_PASSWORD_LENGTH),
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.recoveryKey).toBe('ABCD-EFGH-JKLM-NPQR-STUV-WXYZ');
    }
  });

  it('rejects a whitespace-only recovery key', () => {
    const result = recoverSchema.safeParse({
      recoveryKey: '   ',
      newPassword: 'a'.repeat(MIN_PASSWORD_LENGTH),
      confirmPassword: 'a'.repeat(MIN_PASSWORD_LENGTH),
    });
    expect(result.success).toBe(false);
    expect(firstMessage(result)).toBe('validation.recoveryKeyRequired');
  });
});
