import { MIN_PASSWORD_LENGTH } from '@shared/schema';

/**
 * Simple length and variety strength hint under a new password (the first run's Účet, Zabezpečení →
 * Změnit heslo): guidance, not a gate. 'weak' means "too short" (its copy,
 * `auth:onboarding.account.strength.weak`, says so), so it applies only below the minimum length;
 * 'strong' is 14 characters with three of lower case, upper case, digits and symbols. As the
 * desktop wizard's.
 */
export function passwordStrength(password: string): 'weak' | 'fair' | 'strong' | null {
  if (!password) return null;
  const variety = [/[a-z]/, /[A-Z]/, /\d/, /[^\w\s]/].filter((re) => re.test(password)).length;
  if (password.length < MIN_PASSWORD_LENGTH) return 'weak';
  if (password.length >= 14 && variety >= 3) return 'strong';
  return 'fair';
}
